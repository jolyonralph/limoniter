## What's new in 0.14.3

Limoniter 0.14.3 fixes boxes and menus that didn't fit the window, takes Limoniter's own assembler beyond the 68000, and gives the About box a demo screen.

- **Boxes and menus fit any screen.** A box taller or wider than the window used to be cut off with no way to reach the rest; now it stays on the screen and scrolls, with its OK and Cancel buttons kept in sight below. Menus longer than the window, such as File › New as and its sub-menus, scroll too, and a menu from the menu bar opens below the bar. This applies to every box and menu in Limoniter, and new ones get it automatically.
- **The 68008, 68010, 68020 and 68030 in Limoniter's own assembler and disassembler.** `MACHINE 68020` (or `MC68020`, `OPT P=68020`) in the source, or a setting, chooses the processor. The 68020/030's addressing modes (scaled indexes, memory indirect), bit fields, CAS, CHK2, 64-bit multiply and divide, PACK and the rest assemble to exactly vasm's bytes, and the disassembler reads them back for an A1200's or A4000's programs. An instruction a processor hasn't got is refused, naming the one that has it. The maths chips, the MMU and the 68040/060's own instructions come next.
- **Cycle counts beside 68000 code**, as for the 6502, Z80 and 6809, and the Sinclair QL's 68008's counts.
- **Four 68000 mistakes put right:** `bra.l`, `tst.w a0` and `cmp #n,x(pc)` are 68020 instructions and are now refused for a 68000 (as vasm refuses them) instead of being quietly assembled as something else; `move ccr,d0` now gives the right bytes; and a PC-relative offset out of reach is refused.
- **The About box** opens with an 80s demo screen: the Limoniter logo over bouncing copper bars, and a sine scroller saying what Limoniter is.
- A test that couldn't run on Windows (it needs a Unix shell) no longer stops Windows' checks.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
