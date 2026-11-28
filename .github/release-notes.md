## What's new in 0.18.0

Limoniter 0.18.0 makes Limoniter the place where Claude Code and Codex work. Both run in a panel beside your files, and both work with `claude` and `codex` in the terminal. Each edit is a diff to accept or change. Both can work on one project at once, with Limoniter as referee. 0.18.0 also adds tools for writers: rhymes, a thesaurus, metre, and song lyrics for Suno. Colour tags can now be named, and a colour picker writes colours the way an Amiga, a Spectrum or a web page expects them.

**AI agents**
- **The AI agents panel** (View, Panels; Tools, New AI agent session): Claude Code or Codex working on the project from a side panel, as in VS Code. Choose the permission mode, the model and the effort, and change them as it works. The transcript shows its answers as Markdown, its thinking and each tool it uses, folded. `@` mentions a file, a message typed while it works waits its turn, and Stop interrupts. Sessions run side by side as tabs, and past ones can be carried on.
- **Each edit opens as a diff** in the editor: Accept, Reject, or change the text first and then Accept. Commands ask **Allow once**, **Allow for this session** or **Refuse**.
- **Codex** with its own permission levels (Read only, Ask before changes, Workspace write, Full access), its own models and reasoning effort, and a tool server from Limoniter that gives it the problems, the open file, the selection and the open tabs.
- **Claude Code in Limoniter's terminal** connects by itself, as it does with VS Code: its proposed edits open as a review tab (Accept with Alt+A, Reject with Alt+R), it sees what you select and the problems the language servers have found, and **Edit, Mention in Claude Code** puts the file and the selected lines in its prompt.
- **In WSL**: on Windows the panel finds Claude Code and Codex in your WSL distributions too, and runs them there on your project, with every path translated both ways.
- **A copy of its own**: a session can work in its own Git worktree, so it can't disturb your work or another agent's. **Review and merge** shows what it changed and merges it back with Limoniter's three-way merge. **Ask the other to review** hands the changes to the other tool to look over.
- **Claude Code and Codex together** on one copy: files and folders can be **claimed** for a session (by you, from the Explorer, or by the agent), and another session's edits there are refused with a message saying who has them. **Builds and tests wait their turn**, one at a time across sessions. A **task board** hands out tasks for the sessions to take.
- **Usage** (Tools, Usage of AI tools): what Claude Code and Codex have used today or this week, by tool and model, counted from the tools' own logs so that sessions in a terminal count too. It also shows the plan's five-hour and weekly limits with their reset times, and the sessions running now with their tokens and how full their context is.
- **Files changing** (View, Panels): the project's files as they change, newest first, each marked **You**, an agent's name, or **Elsewhere**.

**Opening files from a terminal**
- `limoniter notes.txt:42` opens the file in the Limoniter window already open, at that line; `--new-window` gives it a window of its own, and `--wait` waits until it's closed, so Limoniter can be Git's editor (`git config core.editor "limoniter --wait"`).
- In Limoniter's own terminal the command reaches that terminal's window. The Windows installer can add it to PATH for every terminal.
- In WSL, a small `limoniter` script (offered when a WSL terminal opens) runs the Windows Limoniter with the paths turned into Windows' form.

**Writing**
- **Words** (Tools, Words): rhymes for the word at the cursor (Ctrl+F7) and a thesaurus (Shift+F7), both kept in the program, so nothing is downloaded. Syllable counts beside each line, the stress and metre of a passage ("iambic pentameter"), haiku and limerick checks, and repeated words, filler words and adverbs marked.
- **Song lyrics** (File, New as, Song lyrics): section tags such as `[Verse]` and `[Chorus]`, coloured, folded and offered as you type, a style box kept in the file, Suno's length limits shown as you write, and the syllables and rhyme scheme of each section in the margin.

**Colours**
- **Colour tags of your own**: tags with a name, a colour and a key of your own ("To check", "Slow"), kept in your settings or with the project. The six can be renamed, and the tag menu is now in View.
- **A colour picker** (Tools, Ctrl+Alt+K, or More… on the swatch beside a colour) that writes the colour in the file's own form: hex, rgb(), hsl() or oklch() for the web, `$0RGB` and AGA's halves on the Amiga, INK and BRIGHT on the Spectrum, and the palettes of the C64, CPC, Next, Atari ST and others, each machine's nearest colour shown. It has a contrast checker and an eyedropper for anywhere on the screen.

**Retro**
- **Set up the Amiga** (File, Run on): downloads the emulator, finds your Kickstarts (Amiga Forever's too) or uses the free AROS, and sets F9. Amiga programs then run in Limoniter's window, booted straight into them.
- **Watchpoints**: stop when an address is read or written, or when its value changes, rises or falls. **Conditional breakpoints**, with hit counts. Both work on the built-in machines, the Amiga debugger, ZEsarUX and VICE.
- **The trainer** (Run on, Trainer…) finds where a game keeps its lives or score, and freezes them.
- A **debug console port** on each built-in machine shows what a program writes to it in the Output panel.

**And**
- With no project open, the Project panel lists the **recent projects** to open.

## Downloads

- **Windows:** `Limoniter-Setup-….exe`. This release isn't signed yet, so Windows may say it protected your PC: choose **More info**, then **Run anyway**.
- **Windows, portable:** `Limoniter-…-Windows-portable.zip`, with no installer and no administrator needed: unzip it anywhere (a USB stick, say) and run `limoniter.exe`. It keeps its settings and everything else in the `Limoniter data` folder beside it, leaving nothing on the PC, so several versions can sit side by side. It's unsigned too, so Windows warns about it as it does about the installer.
- **macOS:** `Limoniter-…-macOS.zip`. For Apple Silicon and Intel Macs. Unzip it and move Limoniter to Applications. It isn't signed by Apple, so the first time, open it, then choose **System Settings → Privacy & Security → Open Anyway** (on macOS 14 and earlier, Control-click it and choose **Open**).
- **Linux:** `Limoniter-…-x86_64.AppImage` runs on most distributions from 2022 on: make it executable (`chmod +x`) and run it. Or `limoniter_…_amd64.deb` for Debian 12, Ubuntu 22.04 and later: `sudo apt install ./limoniter_…_amd64.deb`. Both are for 64-bit Intel and AMD computers.

`SHA256SUMS.txt` lists each download's SHA-256, and `SHA256SUMS.txt.minisig` is its signature, which Limoniter checks before it updates itself.

See [what Limoniter can do](https://limoniter.com/features.html), [the file formats it knows](https://limoniter.com/formats.html) and [how it compares](https://limoniter.com/compare.html).
