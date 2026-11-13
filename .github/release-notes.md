## What's new in 0.4.0

**Run what you're editing**
- **Run on machine (F9)** runs the file in front where its kind of program runs: a BASIC listing (tokenised first), a tape, snapshot, program or disk image, or the file selected in a disk image. It asks where the first time.
- **A ZX Spectrum built in** (48K and 128K), with sound, running in a panel beside your file: nothing to install. Click its screen to type on it.
- **Emulators** are found where they're installed and set up in one go: ZEsarUX, Fuse and CSpect for the Spectrum and Next, VICE for Commodores, b2 for the BBC Micro, openMSX. An Ultimate 64 runs a program as it arrives; a Next gets it over NextSync.
- **While it runs**, the built-in Spectrum, ZEsarUX and VICE show their memory in hex view (edits go to the machine) and their screen as text, give back their BASIC program as a listing, and take an edited one back and RUN it.

**Git, for everyone**
- **Source control** (Ctrl+Shift+G): what's changed, staging, commit, push, pull, branches, stashes and tags; marks beside the line numbers for what's changed since the last commit; history, comparing with any commit, who changed each line, and settling merge conflicts.
- **The Git wizard** takes someone who's never used Git from nothing to their first commit, and onto GitHub if they like, explaining each step.

**The assistant**
- **The Assistant panel** keeps the history of everything asked of Claude Code or Codex, and what came of it.
- It can **write a commit message** from your changes, and **suggest how to settle a conflict**.

**Also**
- Amiga ANSI is drawn in the Amiga's own pens, with a palette to change them: Workbench 1.3 and 2.04–3.x presets, or any of the 4096 colours.
- Building on Linux now needs ALSA's headers, for the built-in Spectrum's sound.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon Macs (M1 and later). Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** build it from the source with `cargo build --release`; see the README.

See [what Limoniter can do](https://limoniter.com/features.html) and [how it compares](https://limoniter.com/compare.html).
