#!/bin/sh
# Local install (no AUR needed). Run from the project folder:  ./install.sh
# Use  PREFIX=$HOME/.local ./install.sh  for a per-user install (no sudo).
set -e
PREFIX="${PREFIX:-/usr/local}"
if [ "$PREFIX" = "/usr/local" ] || [ "$PREFIX" = "/usr" ]; then SUDO=sudo; else SUDO=; fi

cargo build --release
$SUDO install -Dm755 target/release/snowdraw "$PREFIX/bin/snowdraw"
$SUDO install -Dm644 snowdraw.desktop "$PREFIX/share/applications/snowdraw.desktop"
$SUDO install -Dm644 snowdraw.svg "$PREFIX/share/icons/hicolor/scalable/apps/snowdraw.svg"
for size in 16 32 48 64 128 256; do
  $SUDO install -Dm644 "icons/snowdraw-$size.png" \
    "$PREFIX/share/icons/hicolor/${size}x${size}/apps/snowdraw.png"
done
$SUDO gtk-update-icon-cache -f -t "$PREFIX/share/icons/hicolor" 2>/dev/null || true
$SUDO update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
echo "Installed. Launch SnowDraw from your app menu (log out/in if the icon is still missing)."
