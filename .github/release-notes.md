## What's new in 0.5.0

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
