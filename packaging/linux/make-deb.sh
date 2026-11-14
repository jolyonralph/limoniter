#!/bin/sh
# Builds Limoniter and packages it as a .deb in target/linux/, for Debian, Ubuntu and the systems made from them.
# Run on Debian or Ubuntu, from the app folder:  sh packaging/linux/make-deb.sh
# Needs dpkg-dev (for dpkg-shlibdeps), which works out the packages the program needs from the libraries it links.
# Releases build it on Ubuntu 22.04, so it installs on Ubuntu 22.04 and Debian 12 and later.
set -eu

version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
target=${CARGO_TARGET_DIR:-target}
cargo build --release --locked

out=target/linux
root="$out/deb"
id=com.limoniter.Limoniter
rm -rf "$root"
mkdir -p "$root/DEBIAN" "$root/usr/bin" "$root/usr/share/applications" "$root/usr/share/metainfo" \
  "$root/usr/share/mime/packages" "$root/usr/share/doc/limoniter"
install -m 755 "$target/release/limoniter" "$root/usr/bin/limoniter"
install -m 644 packaging/linux/$id.desktop "$root/usr/share/applications/"
install -m 644 packaging/linux/$id.metainfo.xml "$root/usr/share/metainfo/"
install -m 644 packaging/linux/$id.xml "$root/usr/share/mime/packages/"
for icon in packaging/linux/icons/*/"$id".png; do
  size=$(basename "$(dirname "$icon")")
  install -D -m 644 "$icon" "$root/usr/share/icons/hicolor/$size/apps/$id.png"
done
{
  echo "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/"
  echo "Upstream-Name: Limoniter"
  echo "Source: https://github.com/jolyonralph/limoniter"
  echo
  echo "Files: *"
  echo "License: Apache-2.0"
  echo " See /usr/share/common-licenses/Apache-2.0, or the text that follows."
  echo " ."
  sed 's/^$/./; s/^/ /' LICENSE
} > "$root/usr/share/doc/limoniter/copyright"
chmod 644 "$root/usr/share/doc/limoniter/copyright"
# The licences of everything built into the program, which the About box points to.
python3 -I tools/gen_notices.py . "$root/usr/share/doc/limoniter/THIRD-PARTY-NOTICES.txt" x86_64-unknown-linux-gnu
chmod 644 "$root/usr/share/doc/limoniter/THIRD-PARTY-NOTICES.txt"

# The libraries the program links (glibc, ALSA, zlib), as packages, from dpkg-shlibdeps, which wants a debian
# folder to work in.
work="$out/shlibdeps"
rm -rf "$work"
mkdir -p "$work/debian"
printf 'Source: limoniter\n\nPackage: limoniter\nArchitecture: any\n' > "$work/debian/control"
linked=$(cd "$work" && dpkg-shlibdeps -O "$OLDPWD/$root/usr/bin/limoniter" | sed -n 's/^shlibs:Depends=//p')
rm -rf "$work"

# The window system's libraries are loaded as the program starts, not linked, so they're named here. EGL draws
# the window and xkbcommon reads the keyboard; the rest are for X11 or Wayland, whichever the desktop uses. File
# dialogs go through the desktop portal (or zenity), and kept passwords to the Secret Service (GNOME Keyring or
# KWallet).
size=$(du -sk --exclude=DEBIAN "$root" | cut -f1)
cat > "$root/DEBIAN/control" <<CONTROL
Package: limoniter
Version: $version
Architecture: amd64
Maintainer: Limoniter <https://github.com/jolyonralph/limoniter/issues>
Installed-Size: $size
Depends: $linked, libegl1, libxkbcommon0
Recommends: libxkbcommon-x11-0, libx11-6, libxcursor1, libxi6, libxrandr2, libwayland-client0, libwayland-cursor0, libwayland-egl1, xdg-desktop-portal | zenity
Suggests: hunspell, cups-client, gnome-keyring | kwalletmanager
Section: editors
Priority: optional
Homepage: https://limoniter.com/
Description: Text and hex editor that also reads older computers' files and disks
 Limoniter is a text and hex editor for everyday work that also understands
 the files, character sets and disks of older computers: PETSCII and the ZX
 Spectrum's characters, BASIC programs, and disk and tape images from the
 Spectrum, Commodore, Amiga, Atari, Apple, BBC Micro and more.
CONTROL

deb="$out/limoniter_${version}_amd64.deb"
rm -f "$deb"
dpkg-deb --root-owner-group -Zxz --build "$root" "$deb"
rm -rf "$root"
echo "Made $deb"
