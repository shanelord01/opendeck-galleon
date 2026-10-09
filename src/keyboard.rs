//! A uinput virtual keyboard that sends the same key codes the module's own
//! numpad mode does.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Mutex;

/// Linux input event codes (linux/input-event-codes.h) the Key action can send.
pub const KEYS: &[(&str, u16)] = &[
	("KEY_KP0", 82),
	("KEY_KP1", 79),
	("KEY_KP2", 80),
	("KEY_KP3", 81),
	("KEY_KP4", 75),
	("KEY_KP5", 76),
	("KEY_KP6", 77),
	("KEY_KP7", 71),
	("KEY_KP8", 72),
	("KEY_KP9", 73),
	("KEY_KPDOT", 83),
	("KEY_KPENTER", 96),
	("KEY_KPSLASH", 98),
	("KEY_KPASTERISK", 55),
	("KEY_KPMINUS", 74),
	("KEY_KPPLUS", 78),
	("KEY_KPEQUAL", 117),
	("KEY_PREVIOUSSONG", 165),
	("KEY_PLAYPAUSE", 164),
	("KEY_NEXTSONG", 163),
	("KEY_STOPCD", 166),
	("KEY_MICMUTE", 248),
	("KEY_MUTE", 113),
	("KEY_VOLUMEDOWN", 114),
	("KEY_VOLUMEUP", 115),
];
const KEY_NUMLOCK: u16 = 69;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const UI_SET_EVBIT: libc::Ioctl = 0x4004_5564;
const UI_SET_KEYBIT: libc::Ioctl = 0x4004_5565;
const UI_DEV_CREATE: libc::Ioctl = 0x5501;
const BUS_VIRTUAL: u16 = 0x06;

pub fn code(name: &str) -> Option<u16> {
	KEYS.iter()
		.find(|(key, _)| *key == name)
		.map(|(_, code)| *code)
}

/// Keypad keys that turn into navigation keys while NumLock is off.
pub fn needs_numlock(code: u16) -> bool {
	matches!(code, 71..=73 | 75..=77 | 79..=83)
}

pub fn numlock_on() -> bool {
	let Ok(leds) = fs::read_dir("/sys/class/leds") else {
		return false;
	};
	leds.flatten()
		.filter(|led| led.file_name().to_string_lossy().ends_with("::numlock"))
		.any(|led| fs::read_to_string(led.path().join("brightness")).is_ok_and(|v| v.trim() != "0"))
}

pub struct VirtualKeyboard {
	file: Mutex<File>,
}

impl VirtualKeyboard {
	pub fn new() -> io::Result<Self> {
		let file = OpenOptions::new()
			.write(true)
			.custom_flags(libc::O_NONBLOCK)
			.open("/dev/uinput")?;
		let fd = file.as_raw_fd();
		let ioctl = |request, value: libc::c_int| {
			// SAFETY: these uinput requests take a plain int argument.
			if unsafe { libc::ioctl(fd, request, value) } < 0 {
				Err(io::Error::last_os_error())
			} else {
				Ok(())
			}
		};
		ioctl(UI_SET_EVBIT, EV_KEY as _)?;
		ioctl(UI_SET_EVBIT, EV_SYN as _)?;
		for (_, code) in KEYS {
			ioctl(UI_SET_KEYBIT, *code as _)?;
		}
		ioctl(UI_SET_KEYBIT, KEY_NUMLOCK as _)?;

		// struct uinput_user_dev: name[80], input_id (bustype, vendor, product,
		// version), ff_effects_max, then four 64-entry axis arrays left at zero.
		let mut device = vec![0u8; 80 + 8 + 4 + 4 * 64 * 4];
		let name = b"Corsair Galleon 100 SD (OpenDeck)";
		device[..name.len()].copy_from_slice(name);
		for (i, value) in [BUS_VIRTUAL, 0x1b1c, 0x2b18, 1].into_iter().enumerate() {
			device[80 + i * 2..82 + i * 2].copy_from_slice(&value.to_ne_bytes());
		}
		(&file).write_all(&device)?;
		// SAFETY: UI_DEV_CREATE takes no argument.
		if unsafe { libc::ioctl(fd, UI_DEV_CREATE) } < 0 {
			return Err(io::Error::last_os_error());
		}
		// Give the compositor a moment to pick up the new device.
		std::thread::sleep(std::time::Duration::from_millis(200));
		Ok(Self {
			file: Mutex::new(file),
		})
	}

	fn event(kind: u16, code: u16, value: i32) -> [u8; size_of::<libc::input_event>()] {
		let event = libc::input_event {
			time: libc::timeval {
				tv_sec: 0,
				tv_usec: 0,
			},
			type_: kind,
			code,
			value,
		};
		// SAFETY: input_event is plain old data.
		unsafe { std::mem::transmute(event) }
	}

	pub fn key(&self, code: u16, pressed: bool) -> io::Result<()> {
		let mut events = Vec::with_capacity(2 * size_of::<libc::input_event>());
		events.extend(Self::event(EV_KEY, code, pressed as i32));
		events.extend(Self::event(EV_SYN, 0, 0));
		self.file.lock().unwrap().write_all(&events)
	}

	pub fn tap(&self, code: u16) -> io::Result<()> {
		self.key(code, true)?;
		self.key(code, false)
	}

	pub fn tap_numlock(&self) -> io::Result<()> {
		self.tap(KEY_NUMLOCK)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn numlock_applies_to_digits_and_dot_only() {
		assert!(needs_numlock(code("KEY_KP0").unwrap()));
		assert!(needs_numlock(code("KEY_KPDOT").unwrap()));
		assert!(!needs_numlock(code("KEY_KPPLUS").unwrap()));
		assert!(!needs_numlock(code("KEY_KPENTER").unwrap()));
	}
}
