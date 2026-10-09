//! The Stream Deck module inside the Galleon 100 SD, driven through hidraw.
//!
//! The module is USB 1b1c:2b18, interface 0. It speaks Elgato's gen 2 Stream
//! Deck HID protocol under Corsair's vendor ID (all values little endian):
//!
//! - feature `03 27`: keep-alive, every 500 ms. Without it the module ignores
//!   every other command and runs the keyboard's own hardware mode.
//! - feature `03 08 <0-100>`: brightness
//! - feature `14` (get): module serial
//! - output `02 07`: key image, 160x160 JPEG, in 1024-byte packets
//! - output `02 0c`: top screen region, 720x384 JPEG, in 1024-byte packets
//! - input `01 00 0c 00` + 12 bytes: key states
//! - input `01 03 03 00 00 <d0> <d1>`: dial press states
//! - input `01 03 03 00 01 <d0> <d1>`: dial rotation, signed clicks

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

pub const KEY_COUNT: usize = 12;
pub const ROWS: u8 = 4;
pub const COLUMNS: u8 = 3;
pub const ENCODERS: usize = 2;
pub const KEY_PX: u32 = 160;
pub const LCD_W: u32 = 720;
pub const LCD_H: u32 = 384;
pub const SEGMENT_W: u32 = LCD_W / ENCODERS as u32;

const VENDOR_PRODUCT: &str = "0003:00001B1C:00002B18";
const PACKET: usize = 1024;
const FEATURE_LEN: usize = 32;

const fn hid_feature_ioctl(nr: u32) -> libc::Ioctl {
	// _IOC(_IOC_READ | _IOC_WRITE, 'H', nr, len); libc::Ioctl is a signed int on musl.
	((3 << 30) | ((FEATURE_LEN as u32) << 16) | ((b'H' as u32) << 8) | nr) as libc::Ioctl
}
const HIDIOCSFEATURE: libc::Ioctl = hid_feature_ioctl(0x06);
const HIDIOCGFEATURE: libc::Ioctl = hid_feature_ioctl(0x07);

/// One input report from the module, already decoded.
pub enum Input {
	Keys([bool; KEY_COUNT]),
	DialPress([bool; ENCODERS]),
	DialTurn([i8; ENCODERS]),
}

/// The hidraw node of the module's Stream Deck interface, if it is plugged in.
pub fn find() -> Option<PathBuf> {
	let mut nodes: Vec<_> = fs::read_dir("/sys/class/hidraw").ok()?.flatten().collect();
	nodes.sort_by_key(|entry| entry.file_name());
	for node in nodes {
		let device = node.path().join("device");
		let Ok(uevent) = fs::read_to_string(device.join("uevent")) else {
			continue;
		};
		if !uevent.to_uppercase().contains(VENDOR_PRODUCT) {
			continue;
		}
		let Ok(real) = fs::canonicalize(&device) else {
			continue;
		};
		let interface = real
			.parent()
			.and_then(|p| p.file_name())
			.map(|n| n.to_string_lossy().into_owned());
		if interface.is_some_and(|name| name.ends_with(":1.0")) {
			return Some(PathBuf::from("/dev").join(node.file_name()));
		}
	}
	None
}

pub struct Deck {
	file: File,
	/// Held per packet, so keep-alives can go out between image packets.
	write_lock: Mutex<()>,
	closed: AtomicBool,
}

impl Deck {
	pub fn open(path: &PathBuf) -> io::Result<Self> {
		let file = OpenOptions::new().read(true).write(true).open(path)?;
		Ok(Self {
			file,
			write_lock: Mutex::new(()),
			closed: AtomicBool::new(false),
		})
	}

	pub fn close(&self) {
		self.closed.store(true, Ordering::SeqCst);
	}

	pub fn is_closed(&self) -> bool {
		self.closed.load(Ordering::SeqCst)
	}

	fn feature(&self, request: libc::Ioctl, buf: &mut [u8; FEATURE_LEN]) -> io::Result<()> {
		let _guard = self.write_lock.lock().unwrap();
		// SAFETY: buf is FEATURE_LEN bytes, matching the length encoded in the request.
		if unsafe { libc::ioctl(self.file.as_raw_fd(), request, buf.as_mut_ptr()) } < 0 {
			return Err(io::Error::last_os_error());
		}
		Ok(())
	}

	fn set_feature(&self, payload: &[u8]) -> io::Result<()> {
		let mut buf = [0u8; FEATURE_LEN];
		buf[..payload.len()].copy_from_slice(payload);
		self.feature(HIDIOCSFEATURE, &mut buf)
	}

	/// The module serial, e.g. `GK10044OAA06635`. Readable in hardware mode too.
	pub fn serial(&self) -> io::Result<String> {
		let mut buf = [0u8; FEATURE_LEN];
		buf[0] = 0x14;
		self.feature(HIDIOCGFEATURE, &mut buf)?;
		let len = (buf[1] as usize).min(FEATURE_LEN - 2);
		let serial = String::from_utf8_lossy(&buf[2..2 + len])
			.trim_matches('\0')
			.to_owned();
		Ok(if serial.is_empty() {
			"unknown".to_owned()
		} else {
			serial
		})
	}

	pub fn ping(&self) -> io::Result<()> {
		self.set_feature(&[0x03, 0x27])
	}

	pub fn set_brightness(&self, percent: u8) -> io::Result<()> {
		self.set_feature(&[0x03, 0x08, percent.min(100)])
	}

	fn write_packets(&self, packets: impl Iterator<Item = [u8; PACKET]>) -> io::Result<()> {
		for packet in packets {
			let _guard = self.write_lock.lock().unwrap();
			(&self.file).write_all(&packet)?;
		}
		Ok(())
	}

	pub fn key_image(&self, index: u8, jpeg: &[u8]) -> io::Result<()> {
		let chunks = jpeg.chunks(PACKET - 8);
		let count = chunks.len();
		self.write_packets(chunks.enumerate().map(|(part, chunk)| {
			let mut packet = [0u8; PACKET];
			packet[..4].copy_from_slice(&[0x02, 0x07, index, (part + 1 == count) as u8]);
			packet[4..6].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
			packet[6..8].copy_from_slice(&(part as u16).to_le_bytes());
			packet[8..8 + chunk.len()].copy_from_slice(chunk);
			packet
		}))
	}

	pub fn lcd_image(&self, jpeg: &[u8], x: u16, y: u16, w: u16, h: u16) -> io::Result<()> {
		let chunks = jpeg.chunks(PACKET - 16);
		let count = chunks.len();
		self.write_packets(chunks.enumerate().map(|(part, chunk)| {
			let mut packet = [0u8; PACKET];
			packet[..2].copy_from_slice(&[0x02, 0x0c]);
			for (i, value) in [x, y, w, h].into_iter().enumerate() {
				packet[2 + i * 2..4 + i * 2].copy_from_slice(&value.to_le_bytes());
			}
			packet[10] = (part + 1 == count) as u8;
			packet[11..13].copy_from_slice(&(part as u16).to_le_bytes());
			packet[13..15].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
			packet[16..16 + chunk.len()].copy_from_slice(chunk);
			packet
		}))
	}

	/// Wait up to `timeout_ms` for an input report. `Ok(None)` on timeout or
	/// for reports this plugin doesn't use; an error once the device is gone.
	pub fn read(&self, timeout_ms: i32) -> io::Result<Option<Input>> {
		let mut poll = libc::pollfd {
			fd: self.file.as_raw_fd(),
			events: libc::POLLIN,
			revents: 0,
		};
		// SAFETY: one valid pollfd.
		let ready = unsafe { libc::poll(&mut poll, 1, timeout_ms) };
		if ready < 0 {
			let error = io::Error::last_os_error();
			return if error.kind() == io::ErrorKind::Interrupted {
				Ok(None)
			} else {
				Err(error)
			};
		}
		if ready == 0 {
			return Ok(None);
		}
		if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
			return Err(io::Error::new(
				io::ErrorKind::NotConnected,
				"device removed",
			));
		}
		let mut data = [0u8; 512];
		let len = (&self.file).read(&mut data)?;
		Ok(parse_input(&data[..len]))
	}
}

fn parse_input(data: &[u8]) -> Option<Input> {
	if data.len() < 5 || data[0] != 0x01 {
		return None;
	}
	match data[1] {
		0x00 if data.len() >= 4 + KEY_COUNT => {
			let mut keys = [false; KEY_COUNT];
			for (key, state) in keys.iter_mut().zip(&data[4..4 + KEY_COUNT]) {
				*key = *state != 0;
			}
			Some(Input::Keys(keys))
		}
		0x03 if data.len() >= 5 + ENCODERS => {
			let values = [data[5], data[6]];
			match data[4] {
				0x00 => Some(Input::DialPress(values.map(|v| v != 0))),
				0x01 => Some(Input::DialTurn(values.map(|v| v as i8))),
				_ => None,
			}
		}
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_key_report() {
		let mut report = vec![0x01, 0x00, 0x0c, 0x00];
		report.extend([0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
		let Some(Input::Keys(keys)) = parse_input(&report) else {
			panic!("not a key report")
		};
		assert!(keys[1] && keys[11] && !keys[0]);
	}

	#[test]
	fn parses_dial_turn_as_signed() {
		let Some(Input::DialTurn(ticks)) = parse_input(&[0x01, 0x03, 0x03, 0x00, 0x01, 0xff, 0x02])
		else {
			panic!("not a dial report")
		};
		assert_eq!(ticks, [-1, 2]);
	}

	#[test]
	fn ignores_keep_alive_acknowledgement() {
		assert!(parse_input(&[0x01, 0x27, 0x01, 0x00, 0x01]).is_none());
	}
}
