#!/bin/sh
# Builds Limoniter and wraps it as an AppImage in target/linux/: one file that runs on most Linux desktops.
# Run on Linux, from the app folder:  sh packaging/linux/make-appimage.sh
# Releases build it on Ubuntu 22.04, so it runs on systems from then on (glibc 2.35 or later).
#
# Everything Limoniter needs at run time (fonts, syntaxes, character sets, the logo) is inside the program itself,
# so the AppImage holds just the program, its desktop entry and the licences. The program links only glibc, ALSA
# and zlib, which every desktop has; the window system's libraries (EGL, X11 or Wayland, xkbcommon) are loaded as
# it starts, from the system, as they must be to match its graphics driver.
set -eu

version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
target=${CARGO_TARGET_DIR:-target}
cargo build --release --locked

out=target/linux
tools="$out/tools"
appdir="$out/Limoniter.AppDir"
id=com.limoniter.Limoniter
mkdir -p "$tools"

# appimagetool, and the AppImage runtime it puts at the front, at versions known to work, checked before use.
fetch() {
  if ! echo "$3  $tools/$1" | sha256sum -c --status 2>/dev/null; then
    curl -fsSL -o "$tools/$1" "$2"
    echo "$3  $tools/$1" | sha256sum -c --quiet
  fi
}
fetch appimagetool https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage \
  ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
fetch runtime https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64 \
  2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
chmod +x "$tools/appimagetool"

rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/metainfo" "$appdir/usr/share/mime/packages"
cp "$target/release/limoniter" "$appdir/usr/bin/limoniter"
cp packaging/linux/$id.desktop "$appdir/usr/share/applications/"
cp packaging/linux/$id.metainfo.xml "$appdir/usr/share/metainfo/"
cp packaging/linux/$id.xml "$appdir/usr/share/mime/packages/"
# The licences of everything built into the program, which the About box points to.
python3 -I tools/gen_notices.py . "$appdir/usr/share/doc/limoniter/THIRD-PARTY-NOTICES.txt" x86_64-unknown-linux-gnu
cp LICENSE "$appdir/usr/share/doc/limoniter/"
for icon in packaging/linux/icons/*/"$id".png; do
  size=$(basename "$(dirname "$icon")")
  mkdir -p "$appdir/usr/share/icons/hicolor/$size/apps"
  cp "$icon" "$appdir/usr/share/icons/hicolor/$size/apps/"
done

# What an AppImage has at its top: the desktop entry, the icon, and AppRun, which it starts.
cp packaging/linux/$id.desktop "$appdir/"
cp packaging/linux/icons/256x256/$id.png "$appdir/$id.png"
ln -s $id.png "$appdir/.DirIcon"
ln -s usr/bin/limoniter "$appdir/AppRun"

# APPIMAGE_EXTRACT_AND_RUN: appimagetool is an AppImage itself, and this way it runs without FUSE.
appimage="$out/Limoniter-$version-x86_64.AppImage"
rm -f "$appimage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tools/appimagetool" --no-appstream --runtime-file "$tools/runtime" \
  "$appdir" "$appimage"
rm -rf "$appdir"
echo "Made $appimage"
