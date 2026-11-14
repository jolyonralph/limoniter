## What's new in 0.7.0

Limoniter now writes to the disks and tapes it reads, builds sjasmplus projects as sjasmplus does, and reads many more machines' media.

**Changing disk and tape images**
- **Add, delete and rename files** (or drop them on the tab) on Spectrum TAP/TZX tapes and +3 disks, Commodore D64/D71/D81, PC and Atari ST FAT disks, Amiga ADF, Atari 8-bit DOS 2 and Color Computer RS-DOS images; make folders, reorder a tape. Each change is saved at once and can be undone.
- **File › New disk or tape image**: blank ones, formatted as their machine formats them.
- Every writer is checked against its machine's own tools (c1541, mtools and fsck, amitools, atrcopy, ToolShed, cpmtools, fuse-utils).

**For the Spectrum Next**
- **sjasmplus projects build as sjasmplus builds them**: devices and paged memory (MMU, SLOT, PAGE), SAVENEX with all its commands, SAVESNA, SAVEBIN, SAVETAP and the rest, written beside the source; MODULE, STRUCT, DEFINE, IFUSED, RELOCATE, fake instructions and more. 52 real projects from GitHub give the same bytes as sjasmplus.
- **.nex files open** as their loading screen, palette and banks; Next Layer 2 and LoRes screens are pictures.
- **Dot commands** build, and go on a NextZXOS SD card image from the browser.

**OpticOS**
- **OpticBASIC**, the BASIC of the OpticOS replacement ROM, lists and tokenises exactly as the ROM does, and **F9 runs it** on the built-in Spectrum with the OpticOS ROM (included, MIT licence).

**More media read**
- **Floppy flux images** from a Greaseweazle, SuperCard Pro or KryoFlux (`.scp`, `.hfe`, KryoFlux streams): PC, Atari ST, Amiga, C64, Apple II and more, decoded to their files.
- **Mac**: HFS+ disks and `.dmg` images; MacBinary, BinHex and StuffIt archives.
- **Apple II WOZ** images; **Atari 8-bit** XEX programs and pictures.
- **TRS-80** BASIC and tapes, **ZX81** programs, **MSX** tapes, **SAM Coupé** disks and BASIC, **Oric** tapes and BASIC.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
