#!/usr/bin/env bash
# Builds the plugin and assembles dist/net.shasam.galleon100sd.sdPlugin and a
# zip of it that OpenDeck can install from file.
set -euo pipefail
cd "$(dirname "$0")/.."

PLUGIN=net.shasam.galleon100sd.sdPlugin
TARGET=x86_64-unknown-linux-musl
# OpenDeck picks the binary by its own build triple; the static musl binary
# runs under any glibc and inside the Flatpak sandbox.
OPENDECK_TRIPLE=x86_64-unknown-linux-gnu

cargo build --release --target "$TARGET"

rm -rf "dist/$PLUGIN" "dist/$PLUGIN.zip"
mkdir -p "dist/$PLUGIN/$OPENDECK_TRIPLE/bin"
cp -r assets/. "dist/$PLUGIN/"
cp "target/$TARGET/release/opendeck-galleon" "dist/$PLUGIN/$OPENDECK_TRIPLE/bin/"
cp LICENSE "dist/$PLUGIN/" 2>/dev/null || true
(cd dist && zip -qr "$PLUGIN.zip" "$PLUGIN")
echo "dist/$PLUGIN.zip"
