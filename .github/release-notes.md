## What's new in 0.10.0

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
