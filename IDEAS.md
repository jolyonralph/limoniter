# Ideas

Features and improvements that have been thought of but not built, in two lists:

- **A. Needs you:** these can't be finished without something only you can do: real hardware, an account, a
  purchase, a submission, or a decision.
- **B. Claude can do alone:** these can be built and checked without your help, by the tests, by independent tools
  and in CI.

When an idea comes up, it goes in one of the lists, even if it may never be done. When one is built, it moves to
**Done** at the bottom with the version it shipped in.

Started on 15 November 2026 from what was still open in `MISSING_FEATURES.md` and `COMPARISON.md` (checked against
`FEATURES.md`), and from the 0.9 releases.

**0.10** marks a suggestion for the next version, which is yours to change. **Size** is a rough guess: S small,
M medium, L large. **From** says where the idea came from.

## A. Needs you

| Idea | What's needed from you | 0.10 | Size | From |
|---|---|---|---|---|
| **Test the universal Mac app on an Intel Mac** | Run 0.9.1 on the 2018 MacBook Pro and say what goes wrong | ✓ | S | 0.9.1 |
| **Greaseweazle testing**: flux images read from real disks | A Greaseweazle and disks of each kind, read and sent back as `.scp` or `.hfe` files | ✓ | M | FEATURES (Floppy archivists) |
| **Sign and notarise the Mac app** | An Apple Developer account (99 USD a year), its certificate and an app-specific password as repository secrets | ✓ | M | README, release notes |
| **Sign the Windows installer** | A code-signing certificate, or an Azure Trusted Signing account, as repository secrets | ✓ | M | README, release notes |
| **Microsoft Store**: each release's package uploaded | Uploading in Partner Center | each release | S | release workflow |
| **Flathub**: the Flatpak sent for each release | Submitting it to Flathub | each release | S | release workflow |
| **winget**: release pull requests sent without a hand step | A `WINGET_TOKEN` secret, if it isn't already set | | S | release workflow |
| **Real machines**: sending programs to an Ultimate 64, a Spectrum Next (NextSync) and a MiSTer | The machines, to try each one | | S each | FEATURES (Real machines) |
| **The menus in other languages** | Which languages, and someone who speaks each to check them. Claude can do the work of translating | | L | COMPARISON |
| **Web search** for the selected text | Deciding whether sending text to an outside service is wanted | | S | MISSING_FEATURES 49 |

## B. Claude can do alone

| Idea | Why | 0.10 | Size | From |
|---|---|---|---|---|
| **Run the tests on an Intel Mac** in CI as well as Apple Silicon | Only the Apple Silicon half of the Mac app is ever tested | ✓ | S | 0.9.1 |
| **Conversions:** Base64 encode and decode, JSON to XML and back | Everyday conversions, on the list since the start | ✓ | M | MISSING_FEATURES 38 |
| **Your own highlighting grammars**: a folder of `.sublime-syntax` files that Limoniter picks up | Adding languages without a new release | ✓ | M | MISSING_FEATURES 43; COMPARISON |
| **Check for updates** to Limoniter itself, which can be turned off | People on 0.8 won't know 0.9 is out unless they look | ✓ | S | MISSING_FEATURES 50 |
| **Save and restore settings** to a file | Moving to a new computer, or keeping a copy | ✓ | S | MISSING_FEATURES 50 |
| **Tile and cascade documents**, and **scroll two views together** outside Compare | Several files side by side, beyond the split view | ✓ | M | MISSING_FEATURES 42 |
| **Saved layouts**: named arrangements of the panels | Switching between writing, debugging and comparing | ✓ | M | MISSING_FEATURES 28 |
| **Column markers** at chosen columns (such as 72 or 80) | Fixed-width formats, COBOL, assembler, commit messages | ✓ | S | MISSING_FEATURES 40 |
| **HTML in the live preview** | Live preview does Markdown only | ✓ | M | MISSING_FEATURES 37 |
| **BBC Micro development tools**: BeebAsm as an installed assembler, a guide to the MOS calls (OSWRCH, OSBYTE, OSWORD), vectors and the 6845, System VIA and video ULA on hover, Mode 7 and mode 0–6 starters, and a debugger (through b2's or another emulator's remote protocol, as with VICE) | Matching what the Amiga, ST and C64 got in 0.9 | | L | asked 15 November 2026 |
| **CI on Ubuntu 26**: check the workflows before GitHub moves `ubuntu-latest` | GitHub warns on every run that the move began on 19 October 2026 | ✓ | S | release runs |
| **Huge files**: fewer commands set aside above 64 MB (Fold all, the function list, previews, BASIC listings, part history) | Every command available, as UltraEdit and EmEditor manage | | L | FEATURES, COMPARISON |
| **Huge files**: UTF-16 files and hex mode above 1 GB; very long lines without slowing the window | Today these are read whole | | L | MISSING_FEATURES (Huge files) |
| **Open files over FTP**, not only send them | SFTP opens and saves; FTP only sends a copy | | M | COMPARISON |
| **Remote search and a remote terminal** over SSH | Working on a server's files as on your own | | L | COMPARISON |
| **A catalogue of add-ons**, and add-ons that add file formats or kinds of window | Sharing Rhai add-ons | | L | COMPARISON |
| **More table tools** for CSV | EmEditor's table tools go further | | M | COMPARISON |
| **Encryption** of files | Needs real care | | M | MISSING_FEATURES 38 |
| **Code Surround**: wrap the selection in a pair or a snippet | A small convenience | | S | MISSING_FEATURES 44 |
| **Gather windows**: bring every open window onto the screen | A small convenience | | S | MISSING_FEATURES 44 |
| **Line spacing** in the editor | Readability | | S | MISSING_FEATURES 40 |
| **Delete the file** from the File menu, and **Email** it | Rename and Show in folder are done | | S | MISSING_FEATURES 41 |
| **Several clipboards** to choose between, beyond the clipboard history | | | S | MISSING_FEATURES (Partly there) |
| **A Tabs and spaces menu** | There are two commands and a tab size | | S | MISSING_FEATURES (Partly there) |
| **Check open files for changes** every so often, not only when the window comes back into focus | Log files changing in the background | | S | MISSING_FEATURES (Partly there) |
| **More in File properties**: the program it opens with, owner, lines of code | | | S | MISSING_FEATURES (Partly there) |
| **A Bookmarks menu** | There are set, next, previous and a panel | | S | MISSING_FEATURES (Partly there) |

## Done

| Idea | Version |
|---|---|
| A universal Mac app, for Intel Macs as well as Apple Silicon | 0.9.1 |
| 6502 source on the BBC Micro: source that calls the MOS goes on a disc that *RUNs it, and runs in b2 with F9 | 0.10 (built, not yet released) |
