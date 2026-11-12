#!/bin/sh
# Builds Limoniter and wraps it as Limoniter.app in target/macos/, with its icon made from the logo.
# Run on a Mac, from the app folder:  sh packaging/macos/make-app.sh
# The app isn't signed: macOS asks before opening it the first time (Control-click it and choose Open).
set -eu

version=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)
cargo build --release --locked

out=target/macos
app="$out/Limoniter.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/limoniter "$app/Contents/MacOS/limoniter"

# The icon, at every size macOS asks for, from the logo.
iconset="$out/Limoniter.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" data/logo-source.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" data/logo-source.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Limoniter.icns"
rm -rf "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Limoniter</string>
  <key>CFBundleDisplayName</key><string>Limoniter</string>
  <key>CFBundleIdentifier</key><string>com.limoniter.Limoniter</string>
  <key>CFBundleExecutable</key><string>limoniter</string>
  <key>CFBundleIconFile</key><string>Limoniter</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <!-- Limoniter can open any file, so Finder offers it under Open With; it never takes a kind of file for itself. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Any file</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>public.data</string>
        <string>public.content</string>
        <string>public.item</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
PLIST

# A zip that keeps the app's permissions and structure, as Finder makes them.
ditto -c -k --keepParent "$app" "$out/Limoniter-$version-macOS.zip"
echo "Made $app and $out/Limoniter-$version-macOS.zip"
