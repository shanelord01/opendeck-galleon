#!/usr/bin/env python3
"""Draws the key and dial images for the default layouts into the plugin's
icons/ folder. Run it again after changing a glyph; the PNGs are committed."""

import math
import os
import sys

from PIL import Image, ImageDraw, ImageFont

SIZE = 144
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "icons")
FONT = "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Bold.ttf"
FG = (255, 255, 255)
ACCENT = (250, 200, 40)


def canvas():
	img = Image.new("RGB", (SIZE, SIZE), (0, 0, 0))
	return img, ImageDraw.Draw(img)


def text(label, size=64, colour=FG):
	img, d = canvas()
	d.text((SIZE / 2, SIZE / 2), label, fill=colour, font=ImageFont.truetype(FONT, size), anchor="mm")
	return img


def triangle(d, x, y, w, h, left=False):
	if left:
		d.polygon([(x + w, y), (x + w, y + h), (x, y + h / 2)], fill=FG)
	else:
		d.polygon([(x, y), (x, y + h), (x + w, y + h / 2)], fill=FG)


def previous():
	img, d = canvas()
	d.rectangle((44, 50, 52, 94), fill=FG)
	triangle(d, 54, 50, 44, 44, left=True)
	return img


def play_pause():
	img, d = canvas()
	triangle(d, 36, 50, 36, 44)
	d.rectangle((82, 50, 90, 94), fill=FG)
	d.rectangle((98, 50, 106, 94), fill=FG)
	return img


def next_track():
	img, d = canvas()
	triangle(d, 46, 50, 44, 44)
	d.rectangle((92, 50, 100, 94), fill=FG)
	return img


def mic():
	img, d = canvas()
	d.rounded_rectangle((58, 34, 86, 82), 14, fill=FG)
	d.arc((46, 50, 98, 98), 0, 180, fill=FG, width=6)
	d.rectangle((69, 97, 75, 110), fill=FG)
	d.rectangle((56, 108, 88, 114), fill=FG)
	return img






def speaker():
	img, d = canvas()
	d.rectangle((30, 58, 50, 86), fill=FG)
	d.polygon([(50, 58), (74, 38), (74, 106), (50, 86)], fill=FG)
	d.arc((60, 46, 100, 98), -50, 50, fill=FG, width=6)
	d.arc((60, 30, 124, 114), -50, 50, fill=FG, width=6)
	return img




ICONS = {
	"previous": previous(),
	"play-pause": play_pause(),
	"next": next_track(),
	"mic-mute": mic(),
	"volume": speaker(),
	"kp-enter": text("ENT", 46),
	"kp-dot": text("•", 64),
	"kp-slash": text("/", 70),
	"kp-asterisk": text("*", 80),
	"kp-minus": text("−", 70),
	"kp-plus": text("+", 70),
	"kp-equal": text("=", 70),
	**{f"kp-{n}": text(str(n), 70) for n in range(10)},
	"key": text("KEY", 40),
}

def plugin_icon(size):
	"""The plugin and store icon: a keyboard's Stream Deck module, a 4x3 grid
	of lit keys under a screen with two dials, drawn at 512 and scaled down."""
	big = 512
	img = Image.new("RGBA", (big, big), (0, 0, 0, 0))
	d = ImageDraw.Draw(img)
	d.rounded_rectangle((40, 16, 472, 496), 64, fill=(24, 24, 30, 255), outline=(70, 70, 80, 255), width=6)
	for x in (150, 362):
		d.ellipse((x - 34, 40, x + 34, 108), fill=(120, 120, 130, 255), outline=(200, 200, 210, 255), width=5)
	d.rounded_rectangle((84, 126, 428, 206), 14, fill=(60, 40, 120, 255))
	for r in range(4):
		for c in range(3):
			x, y = 96 + c * 112, 222 + r * 66
			d.rounded_rectangle((x, y, x + 96, y + 54), 12, fill=ACCENT + (255,))
	return img.resize((size, size), Image.LANCZOS)


if __name__ == "__main__":
	for name, size in (("plugin.png", 72), ("plugin@2x.png", 144), ("store.png", 512)):
		plugin_icon(size).save(os.path.join(OUT, name))
	os.makedirs(os.path.join(OUT, "keys"), exist_ok=True)
	for name, img in ICONS.items():
		img.save(os.path.join(OUT, "keys", f"{name}.png"))
	print(f"wrote {len(ICONS)} icons to {os.path.normpath(OUT)}/keys", file=sys.stderr)
