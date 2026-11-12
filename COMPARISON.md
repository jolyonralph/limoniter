# How Limoniter compares

Limoniter set out to be a capable everyday text and hex editor that also understands the files, character sets and disks of older computers. This page sets it beside the editors people most often use instead, free and commercial, and is honest about what they do that Limoniter doesn't.

It reflects the editors as of November 2026, going by their own documentation and how they're widely known to work. Editors change quickly; corrections are welcome.

**Key:** ✓ built in · ◐ through a plug-in or extension, or only in part · ✗ not available · ? not checked

## The editors

| Editor | Free or paid | Open source | Runs on |
|---|---|---|---|
| **Limoniter** | Free | Yes (Apache 2.0) | Windows; Linux (builds and runs); macOS (builds and passes its tests; no app to download yet) |
| Windows Notepad | Free, with Windows (some AI features need a Microsoft subscription) | No | Windows |
| Notepad++ | Free | Yes (GPL) | Windows |
| Visual Studio Code | Free (some extensions and AI plans are paid) | Core yes (MIT); Microsoft's build no | Windows, macOS, Linux |
| Vim / Neovim | Free | Yes | Windows, macOS, Linux and more |
| GNU Emacs | Free | Yes (GPL) | Windows, macOS, Linux and more |
| Kate | Free | Yes (LGPL) | Linux, Windows, macOS |
| Zed | Free (paid plans for its AI features) | Yes (GPL) | macOS, Linux, Windows |
| UltraEdit | Commercial: subscription | No | Windows, macOS, Linux |
| EmEditor | Commercial: subscription, with a free edition for personal home and school use | No | Windows |
| Sublime Text | Commercial: one-off licence, with an unlimited evaluation | No | Windows, macOS, Linux |
| BBEdit | Commercial: one-off licence; keeps working with fewer features after its trial | No | macOS |
| TextPad | Commercial: one-off licence | No | Windows |

## Everyday editing

| | Limoniter | Notepad | Notepad++ | VS Code | Vim / Neovim | Emacs | Kate | Zed | UltraEdit | EmEditor | Sublime | BBEdit | TextPad |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Tabs and split views | ✓ | ◐ tabs | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Multiple carets | ✓ | ✗ | ✓ | ✓ | ◐ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ? | ? |
| Column (block) editing | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Regular expressions | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Find and replace in files | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Code folding | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ? |
| Record and play macros | ✓ | ✗ | ✓ | ◐ | ✓ | ✓ | ✓ | ✗ | ✓ | ✓ | ✓ | ◐ | ✓ |
| Scripting | ✓ Rhai | ✗ | ◐ | ✓ | ✓ | ✓ | ◐ | ◐ | ✓ JavaScript | ✓ JavaScript | ✓ Python | ✓ AppleScript | ✗ |
| Compare two files | ✓ | ✗ | ◐ | ✓ | ✓ | ✓ | ◐ | ◐ | ✓ | ✓ | ◐ | ✓ | ✓ |
| Spell checking | ✓ | ✓ | ◐ | ◐ | ✓ | ✓ | ✓ | ? | ✓ | ✓ | ✓ | ✓ | ✓ |
| Undo history as a tree | ✓ | ✗ | ✗ | ✗ | ✓ | ◐ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Crash recovery of unsaved edits | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ? |
| Markdown preview | ✓ | ✓ | ◐ | ✓ | ◐ | ◐ | ✓ | ✓ | ✓ | ✓ | ◐ | ✓ | ✗ |
| CSV and TSV shown and edited as a table | ✓ | ✗ | ◐ | ◐ | ◐ | ◐ | ? | ? | ? | ✓ | ◐ | ? | ? |

## Big files and bytes

| | Limoniter | Notepad | Notepad++ | VS Code | Vim / Neovim | Emacs | Kate | Zed | UltraEdit | EmEditor | Sublime | BBEdit | TextPad |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Very large files (gigabytes) | ◐ | ✗ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ✓ | ✓ | ◐ | ◐ | ◐ |
| Hex editing | ✓ | ✗ | ◐ | ◐ | ◐ | ✓ | ✗ | ✗ | ✓ | ? | ◐ view only | ◐ view only | ? |
| Bytes with no Unicode equivalent kept exactly through open and save | ✓ | ✗ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ | ◐ |

Limoniter keeps files of 64 MB and more on disk and opens them quickly, and edits, searches and saves them in text or hex mode at any size (tested at 10 GB). Some whole-file commands (reindenting, compare, filtering, printing, converting the encoding in place) still aren't available for them. UltraEdit and EmEditor are built for multi-gigabyte files and do more with them.

## Programming and the wider world

| | Limoniter | Notepad | Notepad++ | VS Code | Vim / Neovim | Emacs | Kate | Zed | UltraEdit | EmEditor | Sublime | BBEdit | TextPad |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Plug-ins or extensions from others | ◐ add-on scripts | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ✓ | ✓ | ◐ | ✗ |
| Code intelligence (language servers: completion, go to definition) | ✗ | ✗ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ✓ | ◐ | ✓ | ✗ |
| Debugger | ✗ | ✗ | ✗ | ✓ | ◐ | ✓ | ◐ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Git | ✗ | ✗ | ◐ | ✓ | ◐ | ✓ | ✓ | ✓ | ◐ | ✗ | ◐ | ✓ | ✗ |
| Built-in terminal | ✗ | ✗ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ◐ | ✗ | ◐ | ✗ | ✗ |
| Run a command on the file | ✓ | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Remote files (FTP, SFTP, SSH) | ◐ SFTP; FTP only to send | ✗ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | ◐ | ✓ | ✗ |
| Editing together in real time | ✗ | ✗ | ✗ | ◐ | ✗ | ◐ | ✗ | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| AI assistance | ◐ via Claude Code or Codex, if installed | ✓ | ◐ | ✓ | ◐ | ◐ | ◐ | ✓ | ? | ✓ | ◐ | ✓ | ✗ |
| Interface in languages other than English | ✗ | ✓ | ✓ | ✓ | ✓ | ◐ | ✓ | ✗ | ✓ | ✓ | ✗ | ✗ | ✓ |

## Older computers' files

This is what Limoniter was made for, and where no other editor here comes close.

| | Limoniter | Notepad | Notepad++ | VS Code | Vim / Neovim | Emacs | Kate | Zed | UltraEdit | EmEditor | Sublime | BBEdit | TextPad |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Common Windows, DOS and ISO code pages | ✓ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ | ◐ | ✓ | ✓ | ✓ | ✓ | ✓ |
| EBCDIC (IBM mainframes) | ✓ 22 | ✗ | ✗ | ✗ | ◐ | ◐ | ◐ | ✗ | ✓ | ✓ | ✗ | ◐ | ✗ |
| Home computer character sets (PETSCII, ATASCII, Spectrum, BBC, Amiga, MSX and more) | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Retro screens previewed (ANSI art, PETSCII, teletext, Spectrum) | ✓ | ✗ | ✗ | ◐ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Tokenised BASIC listed and saved back | ✓ | ✗ | ✗ | ◐ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Disk, tape and archive images browsed | ✓ | ✗ | ✗ | ✗ | ✗ | ◐ archives | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Legacy pictures shown and saved as PNG | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Old word processor documents converted | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Sent to the real machine (SD card, Spectrum Next over Wi-Fi, Ultimate 64) | ✓ | ✗ | ✗ | ◐ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ |

Some of these are possible elsewhere with separate tools (cpmtools, AppleCommander, netpbm, Notepad++ or VS Code extensions for one machine or another), but not together in one editor.

## What others do that Limoniter doesn't

- **Other platforms.** Limoniter is made for Windows. It now builds and runs on Linux, where a few things are lighter (no file-type registration; spelling through hunspell or aspell; printing as plain pages through CUPS), and it builds and passes its tests on macOS, but there's no Mac app to download yet. VS Code, Sublime Text, Vim, Emacs, Kate, Zed and UltraEdit are long established on all three; BBEdit is the Mac's own.
- **Plug-ins.** Notepad++, VS Code, Sublime Text, Vim, Emacs, Kate, Zed and EmEditor have thousands of add-ons written by others. Limoniter's add-ons are Rhai scripts that add commands, run when files open or save, and show panels, but there is no catalogue of them yet, and they can't add new kinds of window, language support or file formats.
- **Programming tools.** Code completion and go-to-definition through language servers, debuggers, Git, a terminal inside the editor, and test runners are standard in VS Code, Zed and Kate and available in most others. Limoniter has none of these; it lists a file's functions from its grammar and can run a command on the file.
- **Remote and shared editing.** VS Code, Zed, UltraEdit, BBEdit, Vim and Emacs edit files on other machines over SSH or FTP; Zed and VS Code let several people edit together. Limoniter opens and saves files over SFTP, and can send a copy to an FTP server, but can't open files over FTP, and has no remote terminal, remote search or shared editing.
- **Huge files.** UltraEdit and EmEditor are tuned for files of many gigabytes and keep every command available. Limoniter opens, edits, searches and saves such files in text or hex, but still sets a few commands aside above 64 MB.
- **Tables of data.** EmEditor treats CSV files as tables at any size, with many more tools for them. Limoniter's table view edits, sorts and filters delimited data under 64 MB, and leaves larger files as text.
- **AI built in.** Notepad, VS Code, Zed, EmEditor and BBEdit have AI features of their own. Limoniter's assistant hands the work to Claude Code or Codex, which have to be installed and signed in separately.
- **Other languages.** Notepad, Notepad++, VS Code, UltraEdit, EmEditor and others can show their menus in many languages. Limoniter's are in English only.
- **Maturity.** Notepad++, Vim, Emacs, UltraEdit and the others have been used by many people for many years. Limoniter is new, and will have rough edges the others have long since smoothed.

## What only Limoniter does

- Opens the disk images, tapes and archives of the Amiga, Commodore 64, ZX Spectrum, Amstrad, BBC Micro and Acorn, Apple II, Atari 8-bit and ST, TRS-80, Tandy Color Computer and Dragon, Macintosh, CP/M machines (with their format worked out for itself), HP and PC, and shows what's inside, read only.
- Reads and writes text in the character sets of those machines, byte for byte, alongside 22 EBCDIC code pages and the usual modern ones.
- Previews ANSI art, PETSCII, teletext and Spectrum screens, lists tokenised BASIC from ten dialects and saves several back, shows legacy pictures (Amiga IFF with colour cycling, Atari ST, C64, MacPaint, RISC OS sprites, PCX, GEM, Apple II, Kodak Photo CD) and saves them as PNG, and converts WordStar and MemoMaker documents.
- Keeps an undo history as a tree, and the history of any part of a file.

Each of these readers is checked against independent tools on real disks: cpmtools, AppleCommander, atrcopy, trs80-tool, ToolShed, ddosutils, netpbm and ImageMagick among them.

## Sources

Pricing and platform details were checked in November 2026, including:
[EmEditor licence price update](https://www.emeditor.com/?p=30355),
[UltraEdit pricing](https://www.capterra.com/p/183163/UltraEdit/pricing/),
[Notepad updates for Windows Insiders](https://blogs.windows.com/windows-insider/2026/01/21/notepad-and-paint-updates-begin-rolling-out-to-windows-insiders/),
[Notepad Markdown support](https://winaero.com/microsoft-makes-notepad-markdown-support-generally-available),
[Zed for Windows](https://zed.dev/blog/zed-for-windows-is-here).
