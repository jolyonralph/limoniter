## What's new in 0.13.0

Limoniter is now a full IDE for a folder of code: open the folder and work across all of it, with the Explorer, Go to file and symbol, a Search panel, split editors, terminals in tabs, more from language servers, and debugging for Python, C, C++ and Rust. For retro work there are x86 and ARM assemblers, the Hitachi 6309, Vectrex cartridges, Dragon DOS disks and Amiga boot disks.

**Working on a folder of code**
- **Open folder** (File, Open folder): any folder becomes a project, with an **Explorer** like VS Code's. Files are created, renamed, moved by dragging and deleted in place, and Git's status shows beside each one. Every file is indexed in the background, keeping to `.gitignore`.
- **Go to file** (Ctrl+E) finds any file from a few letters of its name. **Go to symbol** finds a function or label in the folder (Ctrl+T), or in this file (Ctrl+Shift+O).
- **Search panel** (Ctrl+Shift+F): results as you type, as a tree of files and lines. With text to put in its place, each match shows the change before it's made. Replace in one file or all, after a question.
- **Split editor:** another file beside the one in front (Ctrl+\\, or Ctrl+Enter in Go to file). Clicking either pane brings it to the front.
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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
