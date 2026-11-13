## What's new in 0.5.1

- **File › New as** starts a new file of a kind: Spectrum, Next, C64 or BBC BASIC (ready for F9, and GRAPH mode for the Spectrum's), Z80, 6502, 6809 or 68000 assembler with a few lines to begin with, Markdown, or text in a retro encoding. The list is yours to change: each kind is a file ending, an encoding, line endings, colouring and starting text (File › New as › Change this list, or Settings).
- **Assemble and save as binary** (File › Run on) saves just the assembled bytes, with no loader or header, and says the address to load them at.
- **One Assemblers box** (File › Run on › Assemblers, or Settings) chooses Limoniter's own assembler or an installed one for each processor, all in one place. A program you point to is matched to its processor.
- **Fixed:** assembler source ending `.z80` opened as a Spectrum snapshot. It now opens as source; real snapshots still open as snapshots.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
