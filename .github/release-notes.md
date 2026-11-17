## What's new in 0.12.0

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

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
