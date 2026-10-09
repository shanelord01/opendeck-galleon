//! Turning OpenDeck's data URIs into the JPEGs the module displays.

use std::io::Cursor;
use std::sync::LazyLock;

use base64::Engine as _;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, RgbImage};

use crate::deck::{KEY_PX, LCD_H, LCD_W, SEGMENT_W};

/// OpenDeck renders dial images at 200x100 for every device.
const ENCODER_IMG_W: u32 = 200;
const ENCODER_IMG_H: u32 = 100;
const QUALITY: u8 = 92;

pub static BLACK_KEY: LazyLock<Vec<u8>> = LazyLock::new(|| jpeg(&RgbImage::new(KEY_PX, KEY_PX)));
pub static BLACK_SCREEN: LazyLock<Vec<u8>> = LazyLock::new(|| jpeg(&RgbImage::new(LCD_W, LCD_H)));

fn jpeg(img: &RgbImage) -> Vec<u8> {
	let mut out = Vec::new();
	JpegEncoder::new_with_quality(&mut out, QUALITY)
		.encode_image(img)
		.expect("encoding to memory cannot fail");
	out
}

pub fn decode_data_uri(uri: &str) -> Result<DynamicImage, String> {
	let (_, data) = uri.split_once(',').ok_or("not a data URI")?;
	let bytes = base64::engine::general_purpose::STANDARD
		.decode(data.trim())
		.map_err(|e| e.to_string())?;
	image::load(
		Cursor::new(&bytes),
		image::guess_format(&bytes).map_err(|e| e.to_string())?,
	)
	.map_err(|e| e.to_string())
}

pub fn key_jpeg(img: &DynamicImage) -> Vec<u8> {
	jpeg(
		&img.resize_exact(KEY_PX, KEY_PX, FilterType::Lanczos3)
			.to_rgb8(),
	)
}

/// One dial's half of the top screen: OpenDeck's 200x100 image for that dial,
/// scaled to the segment's width and centred vertically on black.
pub fn segment_jpeg(img: Option<&DynamicImage>) -> Vec<u8> {
	let mut canvas = RgbImage::new(SEGMENT_W, LCD_H);
	if let Some(img) = img {
		let h = SEGMENT_W * ENCODER_IMG_H / ENCODER_IMG_W;
		let scaled = img
			.resize_exact(SEGMENT_W, h, FilterType::Lanczos3)
			.to_rgb8();
		image::imageops::replace(&mut canvas, &scaled, 0, ((LCD_H - h) / 2) as i64);
	}
	jpeg(&canvas)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decodes_png_data_uri() {
		let mut png = Vec::new();
		RgbImage::new(4, 4)
			.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
			.unwrap();
		let uri = format!(
			"data:image/png;base64,{}",
			base64::engine::general_purpose::STANDARD.encode(&png)
		);
		assert_eq!(decode_data_uri(&uri).unwrap().width(), 4);
	}

	#[test]
	fn jpegs_start_with_soi_marker() {
		assert_eq!(&BLACK_KEY[..2], &[0xff, 0xd8]);
		assert_eq!(&segment_jpeg(None)[..2], &[0xff, 0xd8]);
	}
}
