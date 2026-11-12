Limoniter is an open-source text editor inspired by editors such as UltraEdit and Notepad++ with a special emphasis on support for legacy and retro formats such as 8-bit and 16-bit system character sets.

## Building

Limoniter is written in Rust. `cargo build --release` builds it on Windows, Linux and macOS.

- **Windows** is the main platform: everything works, including file types in Explorer, Windows' spell checker and printing through the Windows print dialog.
- **Linux** builds and runs, and passes the same tests. Spell checking uses hunspell or aspell if either is installed, printing goes to the default printer through CUPS (`lp`), and kept SFTP passwords go in the system keyring. Building needs a C compiler, `perl` and `make` (OpenSSL is built in). `packaging/linux/limoniter.desktop` adds it to the desktop's menus.
- **macOS** compiles and passes clippy, but hasn't yet been built into an app or run on a Mac. It uses Finder, the keychain, hunspell or aspell, and CUPS in the same way.

Offering Limoniter as the editor for chosen file types is only done on Windows so far.
