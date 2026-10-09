//! OpenDeck device plugin for the Stream Deck built into the Corsair Galleon
//! 100 SD keyboard.
//!
//! While the profile OpenDeck has selected for the deck is empty, the plugin
//! sends no keep-alives and the keyboard runs its own hardware mode. As soon
//! as that profile has an action, the plugin takes the deck over. It also
//! provides a Key action that sends numpad, media and volume key codes
//! through a virtual keyboard.

mod deck;
mod device;
mod images;
mod keyboard;
mod profiles;

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use base64::Engine as _;
use openaction::global_events::{
	GlobalEventHandler, SetBrightnessEvent, SetImageEvent, set_global_event_handler,
};
use openaction::*;
use serde::{Deserialize, Serialize};

use device::Outgoing;
use keyboard::VirtualKeyboard;

struct Logger;

impl log::Log for Logger {
	fn enabled(&self, metadata: &log::Metadata) -> bool {
		metadata.level() <= log::Level::Info
	}

	fn log(&self, record: &log::Record) {
		// OpenDeck writes the plugin's stderr to its plugin log.
		if self.enabled(record.metadata()) && record.target().starts_with(env!("CARGO_CRATE_NAME"))
		{
			eprintln!("[galleon] {}", record.args());
		}
	}

	fn flush(&self) {}
}

struct Events;

#[async_trait]
impl GlobalEventHandler for Events {
	/// Runs once the plugin is registered. Anything sent before then is
	/// dropped, so the device threads start here.
	async fn plugin_ready(&self) -> OpenActionResult<()> {
		tokio::spawn(forward(device::start()));
		Ok(())
	}

	async fn device_plugin_set_image(&self, event: SetImageEvent) -> OpenActionResult<()> {
		device::set_image(
			&event.device,
			event.controller.as_deref(),
			event.position,
			event.image,
		);
		Ok(())
	}

	async fn device_plugin_set_brightness(
		&self,
		event: SetBrightnessEvent,
	) -> OpenActionResult<()> {
		device::set_brightness(&event.device, event.brightness);
		Ok(())
	}
}

/// Created on first use, so a missing /dev/uinput permission only affects the Key action.
static KEYBOARD: LazyLock<Option<VirtualKeyboard>> = LazyLock::new(|| {
	VirtualKeyboard::new()
		.inspect_err(|error| {
			log::error!("cannot create a virtual keyboard through /dev/uinput: {error}")
		})
		.ok()
});
/// Key instances whose press turned NumLock on, to turn it off again on release.
static NUMLOCK_RESTORE: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

#[derive(Serialize, Deserialize, Default)]
struct KeySettings {
	key: Option<String>,
}

/// Icon for each key, set when the Key action's key is changed.
fn key_icon(name: &str) -> Option<&'static str> {
	Some(match name {
		"KEY_KP0" => "kp-0",
		"KEY_KP1" => "kp-1",
		"KEY_KP2" => "kp-2",
		"KEY_KP3" => "kp-3",
		"KEY_KP4" => "kp-4",
		"KEY_KP5" => "kp-5",
		"KEY_KP6" => "kp-6",
		"KEY_KP7" => "kp-7",
		"KEY_KP8" => "kp-8",
		"KEY_KP9" => "kp-9",
		"KEY_KPDOT" => "kp-dot",
		"KEY_KPENTER" => "kp-enter",
		"KEY_KPSLASH" => "kp-slash",
		"KEY_KPASTERISK" => "kp-asterisk",
		"KEY_KPMINUS" => "kp-minus",
		"KEY_KPPLUS" => "kp-plus",
		"KEY_KPEQUAL" => "kp-equal",
		"KEY_PREVIOUSSONG" => "previous",
		"KEY_PLAYPAUSE" => "play-pause",
		"KEY_NEXTSONG" => "next",
		"KEY_MICMUTE" => "mic-mute",
		"KEY_MUTE" | "KEY_VOLUMEDOWN" | "KEY_VOLUMEUP" => "volume",
		_ => return None,
	})
}

struct KeyAction;

#[async_trait]
impl Action for KeyAction {
	const UUID: ActionUuid = "io.github.shanelord01.galleon100sd.key";
	type Settings = KeySettings;

	async fn key_down(
		&self,
		instance: &Instance,
		settings: &Self::Settings,
	) -> OpenActionResult<()> {
		let (Some(code), Some(keyboard)) = (
			settings.key.as_deref().and_then(keyboard::code),
			KEYBOARD.as_ref(),
		) else {
			return Ok(());
		};
		if keyboard::needs_numlock(code) && !keyboard::numlock_on() {
			let _ = keyboard.tap_numlock();
			NUMLOCK_RESTORE
				.lock()
				.unwrap()
				.insert(instance.instance_id.clone());
		}
		let _ = keyboard.key(code, true);
		Ok(())
	}

	async fn key_up(&self, instance: &Instance, settings: &Self::Settings) -> OpenActionResult<()> {
		let (Some(code), Some(keyboard)) = (
			settings.key.as_deref().and_then(keyboard::code),
			KEYBOARD.as_ref(),
		) else {
			return Ok(());
		};
		let _ = keyboard.key(code, false);
		if NUMLOCK_RESTORE
			.lock()
			.unwrap()
			.remove(&instance.instance_id)
		{
			let _ = keyboard.tap_numlock();
		}
		Ok(())
	}

	async fn did_receive_settings(
		&self,
		instance: &Instance,
		settings: &Self::Settings,
	) -> OpenActionResult<()> {
		let Some(icon) = settings.key.as_deref().and_then(key_icon) else {
			return Ok(());
		};
		let Ok(png) = std::fs::read(
			profiles::PLUGIN_DIR
				.join("icons/keys")
				.join(format!("{icon}.png")),
		) else {
			return Ok(());
		};
		let image = format!(
			"data:image/png;base64,{}",
			base64::engine::general_purpose::STANDARD.encode(png)
		);
		// Sent from a separate task so the websocket read loop never waits on a send.
		let id = instance.instance_id.clone();
		tokio::spawn(async move {
			if let Some(instance) = get_instance(id).await {
				let _ = instance.set_image(Some(image), None).await;
			}
		});
		Ok(())
	}
}

/// Send what the device threads ask for, one message at a time and in order.
async fn forward(mut outgoing: tokio::sync::mpsc::UnboundedReceiver<Outgoing>) {
	use openaction::device_plugin as dp;
	while let Some(message) = outgoing.recv().await {
		let result = match message {
			Outgoing::Register(id) => {
				let (name, rows, columns, encoders, kind) = device::layout();
				dp::register_device(id, name, rows, columns, encoders, kind).await
			}
			Outgoing::Deregister(id) => dp::unregister_device(id).await,
			Outgoing::Rerender(id) => dp::rerender_images(id).await,
			Outgoing::KeyDown(id, p) => dp::key_down(id, p).await,
			Outgoing::KeyUp(id, p) => dp::key_up(id, p).await,
			Outgoing::DialTurn(id, p, ticks) => dp::encoder_change(id, p, ticks).await,
			Outgoing::DialDown(id, p) => dp::encoder_down(id, p).await,
			Outgoing::DialUp(id, p) => dp::encoder_up(id, p).await,
		};
		if let Err(error) = result {
			log::warn!("sending to OpenDeck failed: {error}");
		}
	}
}

#[tokio::main]
async fn main() -> OpenActionResult<()> {
	log::set_logger(&Logger).expect("no other logger is set");
	log::set_max_level(log::LevelFilter::Info);

	set_global_event_handler(Box::leak(Box::new(Events)));
	register_action(KeyAction).await;

	let result = run(std::env::args().collect()).await;
	// Without keep-alives the module goes back to the keyboard's hardware mode by itself.
	log::info!("OpenDeck closed the connection");
	result
}
