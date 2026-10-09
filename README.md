# OpenDeck plugin for the Corsair Galleon 100 SD

The Corsair Galleon 100 SD keyboard has a Stream Deck built in: 12 LCD keys, two dials and a screen above them. The module speaks Elgato's Stream Deck protocol under Corsair's USB vendor ID, so [OpenDeck](https://github.com/nekename/OpenDeck) doesn't find it on its own. This plugin adds it to OpenDeck on Linux.

## What it does

- The 12 keys appear in OpenDeck as a deck of 4 rows by 3 columns, numbered from the top left.
- The two dials appear as encoders: turn, press and release all reach their actions.
- Each dial's image is drawn on its half of the 720×384 screen, the left half for the left dial and the right half for the right.
- OpenDeck's brightness setting controls the module.
- While the profile selected for the deck in OpenDeck is empty, the keyboard keeps its own hardware mode, with its themes, numpad pages and shortcut keys. As soon as that profile has an action, OpenDeck takes the deck over, usually within a second. Switching back to an empty profile hands it back.
- A **Key** action sends numpad, media and volume keys through a virtual keyboard, the same key codes the hardware mode sends.

## Requirements

- Linux on x86-64
- OpenDeck 2.14 or later, either the native package or the Flathub app
- Access to the module's hidraw device (see below)
- For the Key action only: access to `/dev/uinput`

## Installing

From OpenDeck: open **Plugins**, find **Corsair Galleon 100 SD** and install it.

From a release: download `io.github.shanelord01.galleon100sd.sdPlugin.zip` from the [releases page](https://github.com/shanelord01/opendeck-galleon/releases) and choose **Install from file** in OpenDeck's plugin manager.

If the deck doesn't appear in the device list, restart OpenDeck.

### Device access

The plugin talks to the module (USB `1b1c:2b18`) through `/dev/hidraw*`. Some distributions, such as Bazzite, already let the logged-in user open every hidraw device. Elsewhere, install the udev rule from this repository and reload the rules:

```sh
sudo cp 70-galleon-100-sd.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

The Key action creates a virtual keyboard through `/dev/uinput`. If your distribution doesn't give the logged-in user access to it, the Key action logs an error and does nothing; everything else keeps working.

## Using it

1. In OpenDeck, select **Galleon 100 SD** in the device list.
2. Create a profile and put actions on the keys and dials. The deck switches from the keyboard's hardware mode to OpenDeck when the profile has its first action.
3. To go back to hardware mode, select an empty profile (for example, one named "Hardware").

In hardware mode the keyboard handles the keys and dials itself, so a dial can't switch you out of it. Choose a profile in OpenDeck instead.

OpenDeck's editor always draws a device's dials below its keys. On the keyboard they sit above the screen, but the mapping is the same: the left circle in the editor is the left dial.

### Key action

Put a **Key** action on a key and choose a key in its settings: numpad digits and operators, Enter, play/pause, previous and next track, stop, microphone mute, mute and volume up or down. Choosing a key sets a matching icon. Numpad digits and the decimal point work whether NumLock is on or off.

## Building from source

The plugin is written in Rust and built as a static binary, so the same build runs on any distribution and inside the OpenDeck Flatpak.

```sh
rustup target add x86_64-unknown-linux-musl
scripts/package.sh
```

This writes `dist/io.github.shanelord01.galleon100sd.sdPlugin` and a zip of it. Copy the folder into OpenDeck's `plugins` folder (`~/.config/opendeck/plugins`, or `~/.var/app/me.amankhanna.opendeck/config/opendeck/plugins` for the Flatpak) and restart OpenDeck. Its log is in OpenDeck's `logs/plugins/io.github.shanelord01.galleon100sd.sdPlugin.log`.

`tools/make_icons.py` redraws the key icons and the plugin icon (it needs Python and Pillow).

## Protocol notes

Interface 0 of the module takes the Stream Deck gen 2 reports. A feature report `03 27` every 500 ms keeps it in host mode; without it the firmware returns to the keyboard's hardware mode. Key images are 160×160 JPEG (output report `02 07`) and screen regions are JPEG on the 720×384 panel (output report `02 0c`). The comments at the top of `src/deck.rs` list every report the plugin uses.

The protocol details come from [node-elgato-stream-deck](https://github.com/Julusian/node-elgato-stream-deck) and [galleon-deck](https://github.com/NLMP-DDHS/galleon-deck).

This plugin was written with AI assistance. It is not affiliated with Corsair or Elgato.

## Licence

GPL-3.0-or-later, the same as OpenDeck. See [LICENSE](LICENSE).
