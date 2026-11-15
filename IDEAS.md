# Ideas

Features and improvements that have been thought of but not built. When an idea comes up, it goes here, even if it
may never be done. When one is built, it moves to **Done** at the bottom with the version it shipped in.

Started on 15 November 2026 from what was still open in `MISSING_FEATURES.md` and `COMPARISON.md`, checked against
`FEATURES.md`, and from the 0.9 releases.

**Size** is a rough guess: S small, M medium, L large. **From** says where the idea came from.

## Possible for 0.10

These are suggestions, and yours to change.

| Idea | Why | Size | From |
|---|---|---|---|
| **Sign and notarise the Mac app** with an Apple Developer ID, and **sign the Windows installer** | Gatekeeper and SmartScreen stop every new user at the first launch; the release notes and guide spend a paragraph on getting past them | M, and needs an Apple Developer account and a code-signing certificate | README, release notes |
| **Run the tests on an Intel Mac** in CI as well as Apple Silicon | Since 0.9.1 the Mac app runs on Intel, but only the Apple Silicon half is ever tested | S | 0.9.1 |
| **Conversions:** Base64 encode and decode, JSON to XML and back | Everyday conversions that have been on the list since the start. Encryption is kept apart, as it needs real care | M | MISSING_FEATURES 38 |
| **Your own highlighting grammars**: a folder of `.sublime-syntax` files that Limoniter picks up | People can add languages without a new release | M | MISSING_FEATURES 43; COMPARISON (add-ons can't add languages) |
| **Check for updates** to Limoniter itself, which can be turned off, as OpticOS's ROMs already are | People on 0.8 won't know 0.9 is out unless they look | S | MISSING_FEATURES 50 |
| **Save and restore settings** to a file | Moving to a new computer, or keeping a copy | S | MISSING_FEATURES 50 |
| **Tile and cascade documents**, and **scroll two views together** outside Compare | Several files side by side, beyond the split view | M | MISSING_FEATURES 42 |
| **Saved layouts**: named arrangements of the panels | Switching between writing, debugging and comparing | M | MISSING_FEATURES 28 |
| **Column markers** at chosen columns (such as 72 or 80) | Fixed-width formats, COBOL, assembler, commit messages | S | MISSING_FEATURES 40 |
| **HTML in the live preview** | Live preview does Markdown only | M | MISSING_FEATURES 37 |

## Later

| Idea | Why | Size | From |
|---|---|---|---|
| **Huge files**: fewer commands set aside above 64 MB (Fold all, the function list, previews, BASIC listings, part history) | Every command available, as UltraEdit and EmEditor manage | L | FEATURES, COMPARISON |
| **Huge files**: UTF-16 files and hex mode above 1 GB; very long lines without slowing the window | Today these are read whole | L | MISSING_FEATURES (Huge files) |
| **Open files over FTP**, not only send them | SFTP opens and saves; FTP only sends a copy | M | COMPARISON |
| **Remote search and a remote terminal** over SSH | Working on a server's files as on your own | L | COMPARISON |
| **The menus in other languages** | Limoniter is English only | L | COMPARISON |
| **A catalogue of add-ons**, and add-ons that add file formats or kinds of window | Sharing Rhai add-ons | L | COMPARISON |
| **More table tools** for CSV | EmEditor's table tools go further | M | COMPARISON |
| **Code Surround**: wrap the selection in a pair or a snippet | A small convenience | S | MISSING_FEATURES 44 |
| **Gather windows**: bring every open window onto the screen | A small convenience | S | MISSING_FEATURES 44 |
| **Line spacing** in the editor | Readability | S | MISSING_FEATURES 40 |
| **Delete the file** from the File menu, and **Email** it | Rename and Show in folder are done | S | MISSING_FEATURES 41 |
| **Several clipboards** to choose between, beyond the clipboard history | | S | MISSING_FEATURES (Partly there) |
| **A Tabs and spaces menu** | There are two commands and a tab size | S | MISSING_FEATURES (Partly there) |
| **Check open files for changes** every so often, not only when the window comes back into focus | Log files changing in the background | S | MISSING_FEATURES (Partly there) |
| **More in File properties**: the program it opens with, owner, lines of code | | S | MISSING_FEATURES (Partly there) |
| **A Bookmarks menu** | There are set, next, previous and a panel | S | MISSING_FEATURES (Partly there) |
| **Encryption** of files | Needs real care | M | MISSING_FEATURES 38 |
| **Web search** for the selected text | It sends the text to an outside service | S | MISSING_FEATURES 49 |

## Done

| Idea | Version |
|---|---|
| A universal Mac app, for Intel Macs as well as Apple Silicon | 0.9.1 |
