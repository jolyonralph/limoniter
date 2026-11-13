## What's new in 0.6.0

Limoniter becomes an IDE, for the old machines and the new.

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
