## What's new in 0.11.0

Music and sound play in the preview, there's a 68000 disassembler, and the Atari 2600 is supported from end to end: disassemble a cartridge, write your own, and run it in Stella or MAME.

**Music and sound**
- **Tracker modules play**, with Limoniter's own player:
  - ProTracker `.mod` files from the Amiga
  - the PC's Scream Tracker 3 (`.s3m`), FastTracker 2 (`.xm`) and Impulse Tracker (`.it`) modules
  - each with its own tracker's effects
- **The player** has Play, Pause, Stop, Loop and a slider to move through the song. The pattern scrolls past as it plays, and clicking a channel's number switches it off or on. The music keeps playing while you work on other files.
- **Sound files play** too: MP3, WAV, FLAC, Ogg Vorbis and AIFF, and the Amiga's 8SVX samples, Creative VOC files and Sun and NeXT `.au` files. Their tags (title, artist, album) are shown beside them.

**The 68000**
- **A 68000 disassembler** (Tools, Disassemble) works out what a file is:
  - Amiga programs come back as sections, with their relocations as labels, so they can be changed and rebuilt
  - Atari ST programs, with their symbols
  - Amiga boot blocks and Atari ST boot sectors
  - Mega Drive cartridges, followed from their vectors
  - Kickstart ROMs
- **What the code does is named beside it:** the Amiga's library calls (`jsr -552(a6) ; exec OpenLibrary`) and custom chip registers, and the ST's hardware and GEMDOS, BIOS and XBIOS calls.
- **The source always assembles back** to exactly the same bytes, with Limoniter's own assembler or vasm.

**The Atari 2600**
- **Cartridges** (`.a26`) are recognised with their bank switching worked out: F8, F6, F4 and the Superchip's, FA, E0, E7, FE, 3F and DPC.
- **Disassembled into DASM source** that rebuilds the cartridge:
  - code followed from bank to bank
  - the TIA's and RIOT's registers named
  - each instruction's cycles beside it
  - code that runs past a scanline's 76 cycles pointed out
  - graphics drawn as pixels
- **DASM source** is made into a cartridge, and **F9** runs it in **Stella** or **MAME**.
- **File, New as, Atari 2600** starts a whole frame that runs straight away.
- **Get an emulator** now installs Stella and MAME.

**Also**
- **The assembler** reads DASM's `processor`, `seg.u`, `rorg` and `.w`, and the 68000's MOVEP.
- **Amiga programs** with debug hunks or Kickstart 2.0's short relocation tables are read.
- **The website's home page** shows what Limoniter does for writing new software for older machines, and its figures are up to date.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
