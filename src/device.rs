//! Device lifecycle: finding the module, keep-alives, input, drawing, and
//! handing the deck back to the keyboard's hardware mode.
//!
//! Nothing here runs on the websocket read loop. OpenDeck's events only drop
//! work into `pending` or update a flag, so a slow device or a busy socket can
//! never stall the plugin's connection to OpenDeck.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::deck::{self, COLUMNS, Deck, ENCODERS, Input, KEY_COUNT, LCD_H, LCD_W, ROWS, SEGMENT_W};
use crate::{images, profiles};

pub const NAMESPACE: &str = "cg";
const DEVICE_NAME: &str = "Galleon 100 SD";
/// Stream Deck + is the closest device type: keys plus dials with an LCD strip.
const DEVICE_TYPE: u8 = 7;
const PING_INTERVAL: Duration = Duration::from_millis(500);
const SCAN_INTERVAL: Duration = Duration::from_secs(2);
const MODE_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// What a device thread asks the websocket side to send, in order.
pub enum Outgoing {
	Register(String),
	Deregister(String),
	Rerender(String),
	KeyDown(String, u8),
	KeyUp(String, u8),
	DialTurn(String, u8, i16),
	DialDown(String, u8),
	DialUp(String, u8),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
	Clear,
	Key(u8),
	Dial(u8),
}

struct State {
	deck: Mutex<Option<Arc<Deck>>>,
	device_id: Mutex<Option<String>>,
	/// Whether the plugin, rather than the keyboard's hardware mode, drives the deck.
	host_mode: AtomicBool,
	brightness: AtomicU8,
	/// The newest image for each slot that hasn't been drawn yet. Older ones
	/// are replaced, never queued.
	pending: Mutex<BTreeMap<Slot, Option<String>>>,
	pending_ready: Condvar,
	outgoing: Mutex<Option<UnboundedSender<Outgoing>>>,
}

static STATE: LazyLock<State> = LazyLock::new(|| State {
	deck: Mutex::new(None),
	device_id: Mutex::new(None),
	host_mode: AtomicBool::new(false),
	brightness: AtomicU8::new(50),
	pending: Mutex::new(BTreeMap::new()),
	pending_ready: Condvar::new(),
	outgoing: Mutex::new(None),
});

fn send(message: Outgoing) {
	if let Some(sender) = STATE.outgoing.lock().unwrap().as_ref() {
		let _ = sender.send(message);
	}
}

pub fn device_id() -> Option<String> {
	STATE.device_id.lock().unwrap().clone()
}

fn current_deck() -> Option<Arc<Deck>> {
	STATE.deck.lock().unwrap().clone()
}

/// Start the device threads. The returned receiver carries what they need sent
/// to OpenDeck.
pub fn start() -> UnboundedReceiver<Outgoing> {
	let (sender, receiver) = unbounded_channel();
	*STATE.outgoing.lock().unwrap() = Some(sender);
	thread::Builder::new()
		.name("scan".into())
		.spawn(scan_loop)
		.unwrap();
	thread::Builder::new()
		.name("keep-alive".into())
		.spawn(ping_loop)
		.unwrap();
	thread::Builder::new()
		.name("mode".into())
		.spawn(mode_loop)
		.unwrap();
	thread::Builder::new()
		.name("draw".into())
		.spawn(draw_loop)
		.unwrap();
	receiver
}

// events from OpenDeck (called on the websocket loop: must not block)

pub fn set_image(
	device: &str,
	controller: Option<&str>,
	position: Option<u8>,
	image: Option<String>,
) {
	if device_id().as_deref() != Some(device) {
		return;
	}
	let slot = match (controller, position) {
		(None, None) => Slot::Clear,
		(Some("Encoder"), Some(p)) if (p as usize) < ENCODERS => Slot::Dial(p),
		(_, Some(p)) if (p as usize) < KEY_COUNT => Slot::Key(p),
		_ => return,
	};
	let mut pending = STATE.pending.lock().unwrap();
	if slot == Slot::Clear {
		pending.clear();
	}
	pending.insert(slot, image);
	STATE.pending_ready.notify_one();
}

pub fn set_brightness(device: &str, brightness: u8) {
	if device_id().as_deref() != Some(device) {
		return;
	}
	STATE.brightness.store(brightness, Ordering::SeqCst);
	if STATE.host_mode.load(Ordering::SeqCst)
		&& let Some(deck) = current_deck()
	{
		let _ = deck.set_brightness(brightness);
	}
}

// threads

fn scan_loop() {
	loop {
		if current_deck().is_none()
			&& let Some(path) = deck::find()
		{
			connect(path);
		}
		thread::sleep(SCAN_INTERVAL);
	}
}

fn connect(path: PathBuf) {
	let deck = match Deck::open(&path) {
		Ok(deck) => Arc::new(deck),
		Err(error) => return log::warn!("could not open {}: {error}", path.display()),
	};
	// No keep-alive yet: the serial reads fine in hardware mode, and a ping
	// would take the deck over before we know whether we should.
	let serial = match deck.serial() {
		Ok(serial) => serial,
		Err(error) => {
			return log::warn!("could not read the serial from {}: {error}", path.display());
		}
	};
	let id = format!("{NAMESPACE}-{serial}");
	log::info!("connected {} as {id}", path.display());
	STATE.host_mode.store(false, Ordering::SeqCst);
	*STATE.device_id.lock().unwrap() = Some(id.clone());
	*STATE.deck.lock().unwrap() = Some(deck.clone());
	send(Outgoing::Register(id.clone()));
	check_mode(true);
	thread::Builder::new()
		.name("input".into())
		.spawn(move || read_loop(deck, id))
		.unwrap();
}

fn disconnect(deck: &Arc<Deck>) {
	let mut current = STATE.deck.lock().unwrap();
	if !current.as_ref().is_some_and(|d| Arc::ptr_eq(d, deck)) {
		return;
	}
	*current = None;
	drop(current);
	deck.close();
	STATE.host_mode.store(false, Ordering::SeqCst);
	if let Some(id) = STATE.device_id.lock().unwrap().take() {
		log::info!("disconnected {id}");
		send(Outgoing::Deregister(id));
	}
}

fn ping_loop() {
	loop {
		if STATE.host_mode.load(Ordering::SeqCst)
			&& let Some(deck) = current_deck()
			&& deck.ping().is_err()
		{
			disconnect(&deck);
		}
		thread::sleep(PING_INTERVAL);
	}
}

fn mode_loop() {
	loop {
		thread::sleep(MODE_CHECK_INTERVAL);
		check_mode(false);
	}
}

/// Take the deck over while OpenDeck's selected profile for it has actions,
/// and hand it back to the keyboard's hardware mode while it is empty.
fn check_mode(log_unchanged: bool) {
	let (Some(deck), Some(id)) = (current_deck(), device_id()) else {
		return;
	};
	let wanted = !profiles::selected_profile_is_empty(&id);
	let previous = STATE.host_mode.swap(wanted, Ordering::SeqCst);
	let changed = previous != wanted;
	if changed || log_unchanged {
		log::info!(
			"{}",
			if wanted {
				"OpenDeck mode"
			} else {
				"hardware mode: the selected profile is empty"
			}
		);
	}
	if !changed || !wanted {
		return;
	}
	if deck.ping().is_err() {
		return disconnect(&deck);
	}
	let _ = deck.set_brightness(STATE.brightness.load(Ordering::SeqCst));
	// The module dropped every image while it was in hardware mode.
	set_image(&id, None, None, None);
	send(Outgoing::Rerender(id));
}

fn read_loop(deck: Arc<Deck>, id: String) {
	let mut keys = [false; KEY_COUNT];
	let mut dials = [false; ENCODERS];
	while !deck.is_closed() {
		let input = match deck.read(1000) {
			Ok(Some(input)) => input,
			Ok(None) => continue,
			Err(_) => break,
		};
		// In hardware mode the firmware handles keys and dials itself.
		if !STATE.host_mode.load(Ordering::SeqCst) {
			continue;
		}
		match input {
			Input::Keys(states) => {
				for (i, (&now, was)) in states.iter().zip(keys.iter_mut()).enumerate() {
					if now != *was {
						send(if now {
							Outgoing::KeyDown(id.clone(), i as u8)
						} else {
							Outgoing::KeyUp(id.clone(), i as u8)
						});
						*was = now;
					}
				}
			}
			Input::DialPress(states) => {
				for (i, (&now, was)) in states.iter().zip(dials.iter_mut()).enumerate() {
					if now != *was {
						send(if now {
							Outgoing::DialDown(id.clone(), i as u8)
						} else {
							Outgoing::DialUp(id.clone(), i as u8)
						});
						*was = now;
					}
				}
			}
			Input::DialTurn(ticks) => {
				for (i, &t) in ticks.iter().enumerate() {
					if t != 0 {
						send(Outgoing::DialTurn(id.clone(), i as u8, t.into()));
					}
				}
			}
		}
	}
	disconnect(&deck);
}

fn draw_loop() {
	loop {
		let batch = {
			let mut pending = STATE.pending.lock().unwrap();
			while pending.is_empty() {
				pending = STATE.pending_ready.wait(pending).unwrap();
			}
			std::mem::take(&mut *pending)
		};
		let Some(deck) = current_deck() else { continue };
		if !STATE.host_mode.load(Ordering::SeqCst) {
			continue;
		}
		for (slot, image) in batch {
			if let Err(error) = draw(&deck, slot, image.as_deref()) {
				log::warn!("drawing failed: {error}");
			}
		}
	}
}

fn draw(deck: &Deck, slot: Slot, image: Option<&str>) -> Result<(), String> {
	let decoded = image.map(images::decode_data_uri).transpose()?;
	let result = match slot {
		Slot::Clear => (0..KEY_COUNT as u8)
			.try_for_each(|i| deck.key_image(i, &images::BLACK_KEY))
			.and_then(|_| deck.lcd_image(&images::BLACK_SCREEN, 0, 0, LCD_W as u16, LCD_H as u16)),
		Slot::Key(i) => deck.key_image(
			i,
			&decoded
				.as_ref()
				.map(images::key_jpeg)
				.unwrap_or_else(|| images::BLACK_KEY.clone()),
		),
		Slot::Dial(i) => deck.lcd_image(
			&images::segment_jpeg(decoded.as_ref()),
			(i as u32 * SEGMENT_W) as u16,
			0,
			SEGMENT_W as u16,
			LCD_H as u16,
		),
	};
	result.map_err(|e| e.to_string())
}

/// Used by the device registration message.
pub fn layout() -> (String, u8, u8, u8, u8) {
	(
		DEVICE_NAME.to_owned(),
		ROWS,
		COLUMNS,
		ENCODERS as u8,
		DEVICE_TYPE,
	)
}
