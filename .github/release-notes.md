## What's new in 0.14.0

Limoniter 0.14 is a workshop for the older machines as much as an editor. How To Code v8, the Amiga demo coders' guide, is rewritten, tested and built in. An Atari 2600, a ZX81 and a Jupiter Ace are built in beside the Spectrum. You can now build for the Game Boy, the NES, Sega's consoles and the ColecoVision, compile Boriel's ZX Basic and C for the retro machines, and debug Amiga code in Copperline, stepping backwards as well as forwards. Crunched Amiga files are unpacked. For everyday work there's a start page, an optional ribbon, Mermaid diagrams and PDFs from Markdown, SFTP and FTP projects, and VS Code extensions' themes and grammars.

**How To Code v8**
- **The Amiga demo coders' guide of 1993, rewritten** for today's tools and released as CC0. It has 18 chapters: the startup, interrupts, the copper, the blitter, the display, AGA, chunky to planar (processor, blitter and Akiko), input, sound, 3D vectors, the processors from the 68000 to the 68060, loading and memory, debugging, releasing and further reading. Open it from **Help** or **Tools, Amiga**.
- **Every fact checked** against the Hardware Reference Manual and the NDK 3.2. Its 31 example programs were run on emulated A500s (Kickstart 1.3 and 3.1), faster processors, A1200s and a CD32, both ways, each giving the machine back. Where v7 was wrong, it says so.
- **One startup for every demo**: it takes the machine, or keeps the system running so files can load, chosen by one setting. It puts back everything it changed, the audio filter and AGA's BPLCON4 included, and can check for AGA first.
- **File, New as, Amiga demo** starts from the guide's startup, all in one file. **Copy How To Code's examples to a folder** puts them in a project of your own.
- `limoniter --assemble SOURCE OUTPUT [NAME=VALUE…]` builds an Amiga program from the command line, as F9 does.
- The guide is on limoniter.com too, and the Markdown preview now follows links to files beside a page and shows their PNG and SVG pictures.

**Built-in machines**
- **An Atari 2600**, beside Stella and MAME: a cycle-exact 6507 with every undocumented opcode, the TIA's picture and sound, the RIOT, and bank switching from 2K to F4, FA, E0, E7 and 3F. Debugging works at the source line, and its frames match MAME's pixel for pixel.
- **A ZX81**, its picture made by the Z80 as on the real one, in SLOW and FAST modes. It runs `.p` files, BASIC tokenised from text, and machine code in a REM.
- **A Jupiter Ace**, with redefinable characters, tapes and Forth typed in.
- The ZX81's and Ace's ROMs aren't Limoniter's to give away: choose your own once, and it's checked by its CRC.

**New machines to build for**
- **Game Boy**: the SM83 in RGBDS's syntax, cartridges made as rgbfix makes them, and F9 in SameBoy, mGBA, Gambatte, BGB, Mesen or MAME.
- **NES**: iNES and NES 2.0 files, run in Mesen, FCEUX, Nestopia or MAME. The 65C02, the HuC6280 (PC Engine) and the SNES's SPC700 are assembled and disassembled too.
- **Sega Mega Drive / Genesis**: its ROM header and checksum fixed, run in BlastEm or MAME. The starter has the Z80 play a note.
- **Sega Master System, Game Gear and SG-1000, and the ColecoVision**: their cartridge headers and checksums written, run in Emulicious, Gearsystem, Gearcoleco or MAME.
- **PICO-8 and TIC-80 carts**: their sprite sheet, map and sounds shown beside the code, and F9 runs them.
- Each has a starter under **File, New as**.

**Compilers**
- **Boriel's ZX Basic**: coloured, its SUBs and FUNCTIONs in the outline, completion and hover, and F9 compiling to a tape and running it.
- **C for the older machines**: cc65, llvm-mos, z88dk, SDCC, SGDK, vbcc and amiga-gcc found, with starters. Their errors are marked on their lines, and F9 compiles and runs the result. SAS/C sources are built with amiga-gcc or vbcc through a compatibility header.

**Amiga**
- **Copperline**, the cycle-driven Amiga emulator, joins FS-UAE and WinUAE. F9 boots the program straight away. With copperline-ctl, it's debugged at your source lines, with **Step back** (Shift+F10), memory, a CPU profile by line, and its warnings about chip misuse and self-modifying code.
- **Crunched files unpacked**: PowerPacker, Imploder, StoneCracker, CrunchMania and ByteKiller, as data or as programs. Programs are rebuilt with their hunks and relocations. Disassemble shows the real code, and **Save the decrunched program** writes it out.
- The 68000 assembler now refuses the 68020's forms (`MULU.L`, `TST` of a PC-relative address and the like) instead of quietly making different instructions.

**Tools for retro work**
- **Tile map and level editor**: characters, tiles and maps for the C64, the Spectrum and 1 to 8 bits a pixel. It reads and writes CharPad projects (versions 5 to 9), and writes maps out as source.
- **Raster and copper designer**: bars and splits dragged on a picture of the screen, made into an Amiga copper list, C64 raster interrupts or Atari ST Timer B code.
- **Unit tests for assembly**: Z80 and 6502 routines run with registers and memory set, checked, and their cycles counted.
- **Instruction reference** and **call graph** panels.
- **Serial terminal**, for serial ports and the Ultimate 64, with XMODEM and YMODEM. **EPROM programming** with TL866 and T48 programmers, through minipro.
- **Patches, checksums and entropy**: IPS, BPS and UPS patches made and applied; CRCs and sums, with ROM header checksums put right; a file's entropy shown along it.
- Z80 lines show their T-states, and microcontrollers can be flashed with PlatformIO, avrdude or ipecmd.

**Everyday work**
- **A start page** when every tab is closed: Start, Recent, Learn and a tip.
- **A ribbon**, switched on beside Find a command, made from the very same commands as the menus.
- **VS Code extensions**: their themes, grammars, snippets, file icons and language settings, installed from a .vsix or from Open VSX.
- **Markdown**: Mermaid diagrams drawn in the preview, the preview saved as a PDF, a table of contents kept up to date, lists carried on, tables lined up, and markdownlint's problems marked.
- **Remote work**: SFTP and FTP folders opened as projects, kept in step with the server. FTP and FTPS files are opened and saved directly. `sftp://` addresses open from anywhere, and hosts come from `~/.ssh/config`.
- **Writing code**:
  - indentation guides, problems shown at the end of their line, colours shown under where they're written, and TODO and FIXME coloured with a panel listing them;
  - comments hidden on request, and Find that skips comments and strings;
  - HTML tags closed and renamed in pairs, file paths finished as you type them, and doc comments written from a function's signature;
  - external formatters run on save, and a regular expression tester.
- **Code as a picture**, a **live server** for web pages, **file icons**, the **time spent** on each project (only if switched on), and SVG drawn in the preview.
- **Your own layout**: the window's own font, the status bar's items, panels that hide themselves, and a note of the keys for commands you choose from menus.
- **Colour as** is now a box you can search, by kind, by machine or A to Z. Amiga E is coloured.
- Every new piece of text is in German, Spanish, French and Italian.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
