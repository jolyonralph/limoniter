## What's new in 0.14.4

Limoniter 0.14.4 fixes the 68000 disassembler on real Amiga demos, and gives a project's Explorer filters, colours and a sense of what's being worked on.

**The 68000 disassembler, put right on a real demo**
- A demo such as SAE's MULater now comes back from its source exactly: the same program, with every relocation, through Limoniter's own assembler and through plain vasm. Before, `/*` inside one of a demo's tables was taken as the start of a comment in Limoniter's assembler, and the check that followed turned correct instructions into bytes and lost relocations, so the rebuilt program could crash.
- Pointers to bitplanes, buffers and tables are no longer followed as code, so there are no more pages of `ori.b #0,d0`; tables of numbers are no longer shown as text.
- Copper lists come out as `dc.w` pairs with the custom chips' register names, their WAITs and SKIPs explained; interrupt and exception vectors are named.
- A big disassembly is much faster: MULater's takes 0.4 seconds instead of 18.

**Projects**
- **Hide what you don't need:** right-click a folder in the Explorer for Hide this folder, or a file for Hide .o files; hidden things also leave Go to file and searching, and come back from the project's settings or with Show hidden items. When you open a folder with build or dependency folders (`node_modules`, `target`, `build`…), Limoniter offers to hide them.
- **Recently edited files stand out:** in the Explorer's A–Z list, a file edited lately has its name in bold with a dot whose colour says how lately (today, this week, this month), and folders holding them are marked too. It goes by Limoniter's saves and Git's history, so a fresh clone doesn't light up everything; choose the time span from the Explorer's menu.
- **Colour this file:** right-click a tab to give a file a tint (blue, green, amber, red, rose, violet) or a theme of its own, so several files of the same kind are told apart at a glance; rules can colour a folder's files or a pattern (`admin/`, `config/production.php`). Kept with the project.
- **Open recent, with Projects and Files apart:** File › Open recent and the start page show recent projects first, and a Files tab with files grouped by the project they belong to; a project's file opens its project too.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
