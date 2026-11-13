Limoniter is an open-source text editor inspired by editors such as UltraEdit and Notepad++ with a special emphasis on support for legacy and retro formats such as 8-bit and 16-bit system character sets.

## Installing

Download the Windows installer or the macOS app from [Releases](https://github.com/jolyonralph/limoniter/releases).
Neither is signed yet, so Windows' SmartScreen and macOS's Gatekeeper will ask before running them the first time.
A Microsoft Store version (signed by the Store) and `winget install Limoniter` are on the way: see
`packaging/windows-store` and `packaging/winget`.

## Building

Limoniter is written in Rust. `cargo build --release` builds it on Windows, Linux and macOS.

- **Windows** is the main platform: everything works, including file types in Explorer, Windows' spell checker and printing through the Windows print dialog.
- **Linux** builds and runs, and passes the same tests. Spell checking uses hunspell or aspell if either is installed, printing goes to the default printer through CUPS (`lp`), and kept SFTP passwords go in the system keyring. Building needs a C compiler, `perl`, `make` and ALSA's headers (`libasound2-dev` on Debian and Ubuntu, `alsa-lib-devel` on Fedora) for the built-in Spectrum's sound. OpenSSL is built in. `packaging/linux/limoniter.desktop` adds it to the desktop's menus.
- **macOS** builds and passes all the tests (they run on GitHub's Macs for every change), and each release has a macOS app to download (`packaging/macos/make-app.sh`). It uses Finder, the keychain, hunspell or aspell, and CUPS in the same way.

Offering Limoniter as the editor for chosen file types is only done on Windows so far.

## Code signing policy

Windows releases are to be signed through [SignPath Foundation](https://signpath.org)'s free code signing for
open-source projects: free code signing provided by [SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org). Until that's in place, releases aren't signed.

- **What's signed:** only the Windows installer and program built from this repository by its release workflow on
  GitHub's own computers, from a tagged version. Nothing built elsewhere is signed.
- **Who:** the author and maintainer, [jolyonralph](https://github.com/jolyonralph), writes and reviews every change and
  approves each signing request. There are no other committers.
- **Privacy:** Limoniter sends nothing anywhere unless you ask it to: it connects only to servers and devices you
  choose (SFTP, FTP, Send to device), and the coding assistant hands your code to Claude Code or Codex only when you
  use it. It collects no usage data.
