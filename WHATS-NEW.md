# What's new in Limoniter

Every release since 0.5.0, newest first. The latest five are described in full; older ones in short, each with a link to its full notes.

## 0.13.0

Released 18 November 2026.

Limoniter is now a full IDE for a folder of code: open the folder and work across all of it, with the Explorer, Go to file and symbol, a Search panel, split editors, terminals in tabs, more from language servers, and debugging for Python, C, C++ and Rust. For retro work there are x86 and ARM assemblers, the Hitachi 6309, Vectrex cartridges, Dragon DOS disks and Amiga boot disks.

**Working on a folder of code**
- **Open folder** (File, Open folder): any folder becomes a project, with an **Explorer** like VS Code's. Files are created, renamed, moved by dragging and deleted in place, and Git's status shows beside each one. Every file is indexed in the background, keeping to `.gitignore`.
- **Go to file** (Ctrl+E) finds any file from a few letters of its name. **Go to symbol** finds a function or label in the folder (Ctrl+T), or in this file (Ctrl+Shift+O).
- **Search panel** (Ctrl+Shift+F): results as you type, as a tree of files and lines. With text to put in its place, each match shows the change before it's made. Replace in one file or all, after a question.
- **Split editor:** another file beside the one in front (Ctrl+\, or Ctrl+Enter in Go to file). Clicking either pane brings it to the front.
- **Breadcrumbs** above the text show the folders and the function you're in, each a menu. **Sticky scroll** keeps the function you've scrolled into at the top.
- **Peek definition** (Alt+F12) shows a definition without leaving your place. F12 now finds definitions by name across the folder too.
- **Terminals in tabs:** as many as you like, kept running when the panel's hidden.
- **.editorconfig** files are followed: indentation, line endings, trailing spaces and the final newline.

**Language servers**
- **Quick fixes** (Ctrl+.), **Format document** (Shift+Alt+F) and format on save, **parameter hints** as you type a call, and **inline type hints**.
- **Offered when you need them:** a language server that's installed is offered for its files. One that isn't installed is installed in a terminal, then started.

**Debugging** (Tools, Run, Debug, Ctrl+F5)
- **Python** with debugpy, and **C, C++ and Rust** with gdb or lldb-dap, through the Debug Adapter Protocol.
- **Breakpoints** beside the line numbers, and the stopped line marked. The Debug panel has the call stack, variables to open level by level, and a console that works out expressions. F5, F10, F11 and Shift+F11 step as in VS Code.

**Retro machines**
- **x86 assembler** in NASM's syntax, for the 8086 to the Pentium and later. `.COM` programs and boot sectors run in DOSBox or QEMU.
- **ARM:** an assembler and disassembler for the Archimedes's ARM2 and ARM3, with RISC OS's SWI names. Later ARM code (ARMv7, Thumb and AArch64, Raspberry Pi kernels included) is disassembled too.
- **Hitachi 6309:** its registers and instructions in the 6809 assembler, as lwasm takes them (PRAGMA 6809 turns them off), and in the disassembler.
- **Vectrex cartridges:** a starter with the cartridge header and the BIOS's routine names. F9 runs it in MAME or ParaJVE. Disassembled cartridges name the BIOS calls.
- **Dragon 32 and 64:** `; machine: dragon32` makes F9 build a Dragon DOS binary and run it in XRoar. The Dragon DOS disk window (Tools, Dragon and Vectrex) makes, reads and changes disks.
- **Amiga boot disks with no file system** (Tools, Amiga): code assembled to chosen sectors, a map of the disk, and a boot block that loads them, checked for size.

**Also**
- **Several clipboards** (Ctrl+0 to Ctrl+9 choose one).
- **Music saved as WAV**, and a module's samples as 8SVX.
- The **DEC Technical** character set.
- The ripper finds **15-sample Soundtracker modules** by their instruments' st-NN: names.
- **Translations completed:** about 1,800 more pieces of text in each language.
- A **What's new** page on limoniter.com.

## 0.12.0

Released 17 November 2026.

Limoniter now speaks French, German, Italian and Spanish. It also makes sound effects for four sound chips, finds the music and graphics inside old games and disks, and shows where a program sits in memory after every build. There are new graphics editors for the Spectrum, the Next and the Atari 2600.

**Languages**
- **The whole window in French, German, Italian or Spanish** (Settings, Appearance, Language): the menus, settings, buttons, messages, and the comments in the source Limoniter writes for you.
- **Each language is listed under its own name** (Deutsch, Español, Français, Italiano), with a globe beside the setting, so you can always find your way back.

**Sound effects** (Tools, Sound effects)
- **Effects for four sound chips:** the **AY** (ZX Spectrum 128, Amstrad CPC, MSX), the C64's **SID**, the Atari 2600's **TIA** and the Spectrum's **beeper**.
- **Draw the notes and the volume** on a graph, type each frame's values, or start from a preset (pickup, laser, explosion, power-up, hit, jump, blip, siren) and change its shape. Random and Vary make new ones.
- **Each change plays straight away.** The AY uses the built-in Spectrum's own emulation, and the beeper runs its player on a Z80, so what you hear is what the player does.
- **AYFX banks** from AYFX Editor open, and are saved the way it saves them.
- **Out come the effects with a player to call** once a frame. **Program to run** opens a program that plays each one in turn, for F9 on the built-in Spectrum, VICE, Stella or MAME.

**Ripper** (Tools, Ripper)
- **Music found in any file:** a game, a demo, a snapshot, or a disk, tape or archive image, including every file inside it and a DMS disk's raw tracks.
  - Tracker modules (ProTracker and its kin, S3M, XM, IT), SID, SNDH (Pack-Ice packed too), YM, PT3 and AY files.
  - Each is measured from its own header. Open it in a tab to hear it, or save one or all of them.
- **Graphics found in any bytes:** see them as characters, C64 multicolour and sprites, NES and Game Boy tiles, Amiga and Atari ST bitplanes, a Spectrum screen, or the Next's 8-bit colours, at any offset and width. Save as a PNG, copy as source, or open in the right editor.

**Building programs**
- **A memory map after every build** for the Spectrum, C64, BBC Micro, Color Computer and Atari 2600. A bar over the Output panel shows the program among the machine's ROM, screen, system areas and I/O, and how much memory is still free. It warns when code lands somewhere it can't go or shouldn't, such as ROM, the C64's I/O, the screen or contended memory.

**Graphics editors**
- **Spectrum screen designer:** lines, boxes, ellipses, fill, text in the ROM's font, selections, scrolling, an attribute lock, redo and zoom.
- **Picture converter** for the Spectrum, Timex hi-res, Next, CPC, MSX and BBC Micro.
- **Spectrum Next Layer 2 painter:** every Layer 2 mode and LoRes, with palette editing. The Next's tilemap can be made from a picture, and sprites cut from a sheet, each with a `.nex` that shows them.
- **Atari 2600 sprites and playfield:** sprite frames with a colour on each line, playfields made from pictures, out as DASM tables and programs that show them.

**Also**
- **AMOS and STOS programs** are listed, and tokenised back from a listing.
- **Column mode with Word wrap on** now offers to turn Word wrap off for you.

## 0.11.0

Released 16 November 2026.

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

## 0.10.0

Released 16 November 2026.

Lots of everyday editing improvements, previews of web pages, pictures and old documents, a value inspector for hex, two new disassemblers, and support for the Cambridge Z88 and the Amstrad NC100, NC150 and NC200.

**Editing**
- **Surround with** (Edit): puts the selection in brackets, quotes, a tag or a snippet, and leaves it selected.
- **Bookmarks** (Search): go to any bookmark from a list; copy, cut or delete the bookmarked lines, or delete every other line; swap bookmarked and not; clear them all.
- **Tabs and spaces** (Format): whether Tab types spaces, the tab size, and converting tabs and spaces, including in the indent only.
- **Convert** (Format): Base64 encode and decode, and JSON to XML and back.
- **Column markers** at chosen columns (such as 72 or 80), and **line spacing**.
- **Your own highlighting grammars:** put Sublime Text `.sublime-syntax` files in the grammars folder (Settings, Editing).

**Files and windows**
- **Window, Layouts** saves which panels are open under a name, so you can switch between them. **Window, Gather windows** brings every box back onto the screen.
- **Move to the Recycle Bin** (File, This file), or to the Trash on Mac and Linux.
- **Changes made by other programs** can be checked for every two seconds. Files with no unsaved changes can be reloaded without asking, staying at the end to follow a growing log.
- **File properties** shows the file's owner, the program it opens with, and its lines of code.
- **Save and restore settings** (Tools) puts all of Limoniter's settings in one file, to keep or to move to another computer.
- **Check for a newer Limoniter** (Help) asks GitHub. It also runs once a day by itself, which can be turned off.

**Previews and conversions**
- **Web pages** (`.html`) are laid out in the preview, without their scripts or styles.
- **PNG and JPEG** open as pictures.
- **Old word processors' documents** opened by themselves are shown converted: WordStar, WordPerfect, Protext, 1st Word, Signum!, Final Writer, Wordworth, ProWrite and others. **Format, Save the document as** saves them as text, Markdown, RTF or CSV.

**Hex and character sets**
- **The value inspector** (View, Hex) reads the bytes at the cursor as numbers:
  - integers and IEEE floats, in either byte order
  - IBM hexadecimal floats
  - VAX F, D, G and H floats
  - packed decimal
  - RADIX-50
- **New character sets:**
  - the national versions of ASCII (ISO 646)
  - DEC Special Graphics
  - ASCII with the top bit set, as Prime computers kept text
  - the Amstrad PCW's text and screen sets, the Cambridge Z88's, and the Amstrad NC's

**Machine code**
- **An x86 disassembler** covers everything from the 8086 to AVX-512. It reads DOS `.COM` and `.EXE` files, Windows programs and DLLs, and Linux programs, and follows their code from where it starts.
- **An ICL 2900 disassembler** lists the code in SFL, from ICL's *2900 Order Code*.
- **6502 source** runs on a BBC Micro when it calls the MOS, and on a C64 when it uses the KERNAL. When the code doesn't say, F9 asks, or a `; machine:` line in the source can say.

**Cambridge Z88 and Amstrad NC100, NC150 and NC200**
- **BBC BASIC (Z80)** programs are listed, and tokenised from a listing.
- **Z88 card images** (`.epr`) open in the browser, and **PipeDream** documents and spreadsheets are converted.
- **The Z80 disassembler** names the Z88's operating system calls when asked.
- **NC memory card images** (`.card`) in ZCN's format open in the browser, and program cards are recognised.

**Also**
- **Get an emulator** (File, Send to device, Manage devices) installs any emulator Limoniter runs programs in:
  - on Windows, it downloads and installs ZEsarUX, b2, openMSX, FS-UAE and VICE itself
  - on Linux and Mac, it runs apt or Homebrew in the Terminal panel
  - otherwise, it opens the emulator's download page
- **The table generator and the character and sprite editor** write their data in the form you choose:
  - assemblers: 68000 `dc` lines, Z80 `db`, ACME or 64tass for the 6502, 6809 `fcb`, NASM
  - BASIC DATA lines
  - arrays for C, PHP, Python, JavaScript or Rust
  - Spectrum BASIC that sets the UDGs, for characters
- **Apple II Integer BASIC** programs are listed.
- **A file formats page** on the website lists every format Limoniter reads or writes.

**Fixed**
- The Amiga, Atari ST and C64 hardware guides' lists of registers ran across the window on top of each other.
- Assembling for a machine that runs from a disk said a tape was made.

## 0.9.1

Released 15 November 2026.

The Mac app now runs on Intel Macs as well as Apple Silicon ones: one app for both, in the same download.

## 0.9.0

Released 15 November 2026.

Limoniter now compares files and whole folders, keeps folders in sync, and merges three versions of a file, and Git can use it for diffs and conflicts. There are also new tools for writing demos and intros on the Amiga, Atari ST and Commodore 64.

**Compare and merge**
- **Compare files…** opens two files side by side in a tab, and both sides can be edited. Changed lines and the words within them are marked. An arrow copies a difference across, and Undo takes it back. Spaces, case and line endings can be ignored, and the differences can be saved as a patch. Binary files are compared byte by byte in hex, and pictures as pictures, with where they differ shown.
- **Compare folders…** compares two folders as a tree. Either side can be a folder, a Git revision, a disk image or archive, or a folder on an SFTP server. You can compare by size and time or by contents, and leave out what `.gitignore` leaves out.
- **Update** and **Mirror** keep folders in sync. You see every copy and deletion before anything happens, deletions are confirmed a second time, and each file that's replaced or deleted is backed up first. Files changed on both sides since the last sync are shown as conflicts and left alone.
- **Saved comparisons** keep their sides, filters and rules. They're saved in your project or on their own. **Rules** set how each kind of file is compared (as text that ignores timestamps, as bytes, as pictures, or skipped), and by default build folders such as `.git/`, `target/` and `node_modules/` are skipped.
- **Reports** can be saved as HTML, text, a counts-only summary, or CSV. **Apply a patch** shows what fits before anything is changed, and backs up each file first.
- **Merge** shows the original, mine and theirs above a result you can edit. Changes made on only one side are merged already, and each conflict waits for Take mine, Take theirs or Take both.
- **Git:** files with conflicts get **Resolve in merge…**. **Use Limoniter for Git diff and merge** makes `git difftool` and `git mergetool` open Limoniter. It asks first if you already use another tool, and **Stop using Limoniter for Git** puts back your earlier settings. The command line takes `limoniter --diff LEFT RIGHT` and `limoniter --merge BASE MINE THEIRS RESULT`.

**Amiga**
- **Code hints** from the Amiga NDK 3.2, downloaded from Aminet when you ask: what each function does and its arguments, `_LVO` offsets in the 68000 assembler, and structure fields on hover.
- **A guide to the hardware registers** for OCS, ECS and AGA, shown on hover, plus copper list tools.
- **A bitplane converter** that turns a PNG or JPEG into planes, a palette, ILBM, source, sprites and bobs.
- **Executables** built by the 68000 assembler, which run in FS-UAE or WinUAE, and source-level debugging through their GDB servers.
- **Demo tools:** a blitter minterm calculator, a table generator, and demo and copper starters. Shrinkler, Cranker, LSPConvert and P61 can be run on your files, and ProTracker modules show their details.

**Atari ST**
- **A guide to the hardware registers**, shown on hover, TOS calls, and calculators for the STE blitter and MFP timers, plus ST and STE demo starters.
- **A picture, sprite and font converter**, and Spectrum 512 pictures read and written.
- **Running in Hatari**: choose the machine, memory, monitor and CPU. Limoniter finds your TOS, or downloads EmuTOS. It makes bootable floppies and uses Hatari's profiler by label and line.
- **Music and packers:** SNDH and YM details, Pack-Ice and Atomik files unpacked wherever they're opened, and YM2149 tables.

**Commodore 64**
- **KickAssembler and 64tass**, with KickAssembler downloaded when you ask (its download states no licence, and Limoniter says so first). A guide to the VIC-II, SID, CIA and KERNAL, shown on hover.
- **Raster timing:** 6502 cycle counts, a raster line calculator, stable raster interrupts, and demo and intro starters.
- **A picture, sprite and charset converter**, with C64 modes in the character and sprite editor.
- **SID tunes, packers, CRT cartridges and TAP tapes.**

**Also**
- **OpticOS 1.0.2**, and Limoniter checks OpticOS's releases for newer ROMs and installs them (this can be turned off).
- Hatari keeps the program's output in the log while you're debugging.

## 0.8.0

Released 14 November 2026.

Limoniter now runs on Linux, writes to many more machines' disks, and converts Amiga and Atari ST word processor documents.

**Linux**
- **AppImage and .deb** downloads, for 64-bit Intel and AMD computers. It runs under Wayland and X11, and remote-file passwords are kept in GNOME Keyring or KWallet.

**Changing more disk images**
- **Add, delete and rename files** on Apple II DOS 3.3 and ProDOS disks, BBC Micro DFS and ADFS discs, Spectrum TR-DOS disks and SCL archives, Mac HFS disks (resource forks, types and creators included), and Atari MyDOS and SpartaDOS disks; Applesoft and TR-DOS BASIC listings are tokenised as they go on.
- **New blank images** for each of them: Apple II 140K and 800K, BBC 100K to 640K, TR-DOS 640K, Mac 400K, 800K and 1.4MB, MyDOS and SpartaDOS.
- Each is checked against its machine's own tools: AppleCommander, a2kit, beebtools, MMB_Utils, Apple's fsck_hfs, hfsutils, lsatr, and a Pentagon's TR-DOS running in ZEsarUX.

**Amiga and Atari ST documents**
- **Converted to Markdown, plain text or RTF**: IFF formatted text (FTXT), ProWrite, AmigaGuide (each node a section, with its links), Protext, 1st Word and 1st Word Plus, WordPerfect 4 and 5 (Amiga, ST and DOS), Signum! 1 and 2, Final Writer and Wordworth. Bold, italics, underlining, headings, footnotes, headers and footers are kept.

**More media read**
- **Oric** Sedoric disks, **ZX81** programs on TZX tapes, **Color Computer and Dragon** `.cas` tapes, **Atari 8-bit** Koala and HIP pictures, **Compact Pro** archives and **Disk Copy 6** images.
- **Flux images** of Commodore 1571 disks, Apple II 13-sector disks, and Mac 400K and 800K disks.

**Easier to use**
- **File › Open a file from the web**: opens a file from an http or https address.
- **Open recent** has **Clean up** (removes files that are gone) and **Clear list**.
- **Screen readers** can follow the editor, tabs, the command box and the image browser; **F10** opens the menus, and **Ctrl+F6** moves out of the editor.
- **OpticOS**: the Spectrum keyboard shows OpticOS's own keys.
- **A user guide** on the website: [limoniter.com/guide](https://limoniter.com/guide/).

## 0.7.0

Released 14 November 2026.

Limoniter now writes to the disks and tapes it reads, builds sjasmplus projects as sjasmplus does, runs OpticOS's BASIC, and reads many more machines' media, floppy flux images among them.

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

## 0.6.0

Released 13 November 2026.

Limoniter becomes an IDE, for the old machines and the new: completion, hover, go to definition, renaming and problems as you type in Z80, 6502, 6809 and 68000 source, language servers for everything else, a fuller debugger with watches, a memory window and a profiler, a disassembler, a character and sprite editor, a Spectrum screen designer, a terminal and tasks.

**Knowing your code** (Z80, 6502, 6809 and 68000 source)
- **Completion** as you type: instructions, directives, your labels and macros; BASIC keywords in Spectrum and Commodore listings.
- **Hover** over a label for its value, or an instruction for its address, bytes and timing.
- **Go to definition** (F12), **Find all uses** (Shift+F12) and **Rename** (Ctrl+F12), across INCLUDEd files.
- **Problems as you type**, and an **outline** of labels, macros and ORGs in the Functions panel.

**Language servers** for everything else (Settings, Editing; off until switched on)
- Completion, hover, definitions, uses, renaming and problems from rust-analyzer, clangd, pylsp, gopls and others.

**Debugging**
- **Call stack, watches and a memory window** that picks out what changed.
- **Conditional breakpoints** (`A=0 && [count]>3`) and **stopping when an address is written**.
- **Spectrum BASIC**: breakpoints on lines, a statement at a time, and its variables, on the built-in Spectrum.
- **6809 programs in XRoar** and **68000 programs in Hatari**.
- **A real Next**, through DeZog's dezogif on a serial cable (not yet tried on a real Next: tell us how it goes).
- **A profiler** on the built-in Spectrum: T-states by routine and line.

**Tools for the old machines**
- **Disassembler** for the Z80 (and Next), 6502 and 6809: traced, labelled, and always assembling back to the same bytes.
- **Character and sprite editor**: UDGs, fonts and the Next's 256-colour sprites, to and from DB lines and files.
- **Spectrum screen designer**: draw with INK, PAPER, BRIGHT and FLASH; bring in a PNG; save a .scr.

**For every project**
- **Terminal** (Ctrl+`): your shell in a panel, on a real pseudo-terminal.
- **Tasks**: build, run and test commands found from Cargo, Make, npm, Go, pytest and CMake, or your project's own; **Ctrl+Shift+B** builds, **Ctrl+Shift+T** tests.
- **Problems read back** from compilers and marked by their lines; **test results** listed test by test, with Run only this.

## 0.5.1

Released 13 November 2026.

New files of any kind from File › New as, assembling to a plain binary, and one box to choose the assembler for each processor.

- **File › New as** starts a new file of a kind: Spectrum, Next, C64 or BBC BASIC (ready for F9, and GRAPH mode for the Spectrum's), Z80, 6502, 6809 or 68000 assembler with a few lines to begin with, Markdown, or text in a retro encoding. The list is yours to change: each kind is a file ending, an encoding, line endings, colouring and starting text (File › New as › Change this list, or Settings).
- **Assemble and save as binary** (File › Run on) saves just the assembled bytes, with no loader or header, and says the address to load them at.
- **One Assemblers box** (File › Run on › Assemblers, or Settings) chooses Limoniter's own assembler or an installed one for each processor, all in one place. A program you point to is matched to its processor.
- **Fixed:** assembler source ending `.z80` opened as a Spectrum snapshot. It now opens as source; real snapshots still open as snapshots.

## 0.5.0

Released 13 November 2026.

Limoniter now writes machine code, runs it and debugs it: assemblers for the Z80, 6502, 6809 and 68000 built in, F9 to assemble and run on the built-in Spectrum or in an emulator, and a debugger with breakpoints and stepping. Spectrum BASIC gets GRAPH mode, UDGs drawn as their pictures, and a Spectrum keyboard on the screen.

**Write machine code, run it and debug it**
- **Assemblers built in** for the Z80 (with the Next's extra instructions), the 6502, the 6809 and the 68000. Each gives exactly the same bytes as the tool people already use: sjasmplus, ACME, lwasm and vasm. They understand those tools' ways of writing too, ca65's as well for the 6502.
- **Or use your own**: File › Run on › Assembler… picks sjasmplus or Pasmo for the Z80, ACME for the 6502, lwasm for the 6809 or vasm for the 68000. Problems go in the Output panel; click one to go to its line.
- **F9 assembles and runs**: a Spectrum tape with a BASIC loader (48K or 128K), a Next `.nex`, a C64 program, or an Atari ST program in Hatari. A 6809 program is saved as a CoCo `.bin`. **Assemble and save** saves it without running.
- **A debugger**: click beside a line number for a breakpoint. When the program stops there, the line is marked and the registers are shown, with Step, Step over and Carry on. It works on the built-in Spectrum, in ZEsarUX (so Next programs too) and for 6502 programs in VICE.
- The status bar shows each line's address, bytes and how long it takes to run.

**Spectrum BASIC**
- **GRAPH mode** (Ctrl+Shift+9) types block graphics and UDGs as the Spectrum's GRAPHICS key does.
- **UDGs are drawn as their pictures**, taken from the tape or snapshot the program came from, its DATA lines, or a file you choose.
- **zmakebas and bas2tap listings** convert to programs exactly as those tools do. Settings chooses which way listings write graphics.
- **A Spectrum keyboard** under the built-in Spectrum, with every key's words and symbols, for the key combinations.

**Also**
- Packaging for the Microsoft Store and winget is ready, for when Limoniter is published there.
