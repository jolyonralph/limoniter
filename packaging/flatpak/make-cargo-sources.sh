#!/bin/sh
# Lists every crate in Cargo.lock as a download for flatpak-builder, in packaging/flatpak/cargo-sources.json, so the
# Flatpak builds with no network, as Flathub requires. Make it again whenever Cargo.lock changes.
# Run from the app folder:  sh packaging/flatpak/make-cargo-sources.sh
# Uses flatpak-builder-tools' flatpak-cargo-generator, at a version known to work, with Python 3 and aiohttp, PyYAML
# and tomlkit.
set -eu

work=target/flatpak
mkdir -p "$work"
generator="$work/flatpak-cargo-generator.py"
sum=0a2db6be87d75910facef28ab46d4d6460802e8419ab850d0caa6a364d26b380
if ! echo "$sum  $generator" | sha256sum -c --status 2>/dev/null; then
  curl -fsSL -o "$generator" \
    https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/9d86d336a39b9af38b86f114b6ce5820fe260f91/cargo/flatpak-cargo-generator.py
  echo "$sum  $generator" | sha256sum -c --quiet
fi
# Flathub's build image has them already, on its PYTHONPATH; elsewhere they go in a Python of their own, in
# target/flatpak.
python=python3
if ! python3 -c 'import aiohttp, tomlkit, yaml' 2>/dev/null; then
  python="$work/python/bin/python"
  if ! "$python" -c 'import aiohttp, tomlkit, yaml' 2>/dev/null; then
    python3 -m venv "$work/python"
    "$python" -m pip install --quiet 'aiohttp>=3.9.5,<4' 'PyYAML>=6.0.2,<7' 'tomlkit>=0.13.3,<1'
  fi
fi
"$python" "$generator" Cargo.lock -o packaging/flatpak/cargo-sources.json
echo "Made packaging/flatpak/cargo-sources.json"
