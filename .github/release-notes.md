## What's new in 0.15.0

Limoniter 0.15 is a release for retro coders: the whole 680x0 family, How To Code for the ZX Spectrum and a games chapter for the Amiga, the ZX80, the Vectrex and three of the first computers ever built, the Sinclair QL, five more machines for F9, structure templates for the hex view, tape audio, and Limoniter updating itself.

**The 680x0 family, complete**
- Limoniter's own assembler and disassembler now take every 680x0: the **68010, 68020, 68030, 68040, 68060 and CPU32** as well as the 68000 and 68008, the **68881/68882 maths coprocessor** with floating-point constants in every size, and the **68030's and 68851's MMUs**. `MACHINE 68020` (or `MC68030`, `FPU 1`, `OPT P=68020/68881`), or a setting, chooses; an instruction a processor hasn't got is refused, naming the one that has. Everything matches vasm byte for byte, and every opcode the disassembler reads assembles back to the same bytes.
- **Warnings as vasm gives them**: `moveq #143,d7`, an offset that changes sign, an immediate too big for its size, all shown beside their lines without stopping the build.

**How To Code**
- **How To Code: the ZX Spectrum** (Help, and Tools, ZX Spectrum and Next), a second guide by Comrade J and Mnemonic of Share and Enjoy, CC0: 14 chapters on the 48K, the 128K and the Next, from contended memory and the floating bus to border effects timed to the T-state, masked sprites, beeper and AY music, paging, and the Next's copper, Layer 2 and sprites, with 19 examples (and Catch, a small whole game), each run on Limoniter's built-in Spectrum, in Fuse and in ZEsarUX.
- **Chapter 17 of How To Code v8, Writing games**, built round Gem Cave, a small whole Amiga game: eight-way scrolling over a tile map, sprites and bobs, collisions, the CD32 pad, sound effects over music, and staying friendly to WHDLoad, each example run on Kickstart 1.3 and 3.1, a 68020, a 68040, an A1200 and a CD32.
- Both guides read like a book in Limoniter: the contents page links every chapter, a link turns the page in the same tab, a bar at the top has Back, Forward, Contents and Close (Alt+Left and Alt+Right work too), and the examples' names open them.

**New machines**
- **A built-in ZX80**, its picture going blank while a program runs as the real one's did, with `.o` files loaded, saved, listed and tokenised, and File, New as, ZX80 BASIC.
- **A built-in Vectrex**: its 6809, VIA and AY, and the vector screen drawn as glowing lines that fade like phosphor, with breakpoints and stepping; you choose its BIOS once.
- **Early computing** (Tools): the **Manchester Baby** (1948), with Kilburn's first program; **EDSAC** (1949), its real Initial Orders reading your source as paper tape, with Wilkes's table of squares; and the **PDP-8** (1965), with its front panel and Teletype, PAL source and paper tapes.
- **Sinclair QL programming**: QDOS programs with their headers, microdrive cartridges made and browsed, and F9 into sQLux, Q-emuLator or MAME.
- **F9 on more machines**: the **Amstrad CPC**, **Apple II**, **Atari 8-bit computers**, **Atari 7800** and **SNES**, with their emulators found or installed for you.
- **Control hints** on every built-in machine, with keys you can change and a Kempston joystick on the Spectrum.

**Tools**
- **Structure templates**: a Structure panel beside the hex view names a file's fields, colours their bytes and follows the cursor both ways; 16 formats built in (Amiga hunks, IFF, ADF, TAP, TZX, D64, PNG, ZIP, ELF and more), and templates of your own in a simple language.
- **Tape audio**: Spectrum, CPC, ZX80 and ZX81, Commodore, BBC and MSX tapes made into the sound of the tape (WAV), to load on a real machine.
- **PHP tools** (Tools, PHP): short tags made long, the "headers already sent" fixer, `array()` to `[]`, a PHP 7/8 upgrade check, old files made UTF-8, `php -l` on save, and F9 serving a page.
- **TRSE** (Turbo Rascal) and **RGC BASIC**, coloured and run with F9.
- **Open a ZIP, or a web address, as a project.**
- The **68000 disassembler** reads real demos properly (since 0.14.4), and Limoniter's Spectrum now keeps the Z80's H'L' right in snapshots it hands to other emulators.

**Limoniter itself**
- **Limoniter updates itself**: Help, Check for a newer Limoniter offers Install now, checking every download against the release's signed list first, and it never restarts with unsaved work. The previous version is kept, to go back to. (Copies before 0.15 need one last download by hand.)
- **A portable Windows download**: unzip `Limoniter-…-Windows-portable.zip` anywhere, and it keeps everything in a folder beside itself.
- **Menus and boxes follow the window**: a menu that had to scroll in a small window now grows again when the window does.
- **Settings** has the shortcut everyone expects, **Ctrl+,** (Cmd+, on a Mac), and **File, Quit** is there with **Ctrl+Q** (Cmd+Q). The About box says what Limoniter is today, and its scroller bends each letter round the wave, as demos did, apostrophes and all.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
