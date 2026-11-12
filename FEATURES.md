# What Limoniter can do

Limoniter is a text and hex editor for Windows, which also builds and runs on Linux, and builds and passes its tests on macOS. Alongside everyday editing it knows the character sets, file formats and media of older computers: it opens 8-bit and 16-bit text byte for byte, previews retro screens, pictures and BASIC programs, and browses disk images, tapes and archives from classic machines without changing them.

Every command is in the menus, arranged by what it is for. Press **F1** to find any command by describing what it does. On a Mac, shortcuts use Cmd where this list says Ctrl, Option and an arrow moves by a word, Cmd+[ and Cmd+] go back and forward, and files open from Finder by double-clicking or Open With.

## Editing

- Tabs for open files; **Ctrl+Tab** goes back through files in the order they were used.
- Split a file into two panes (one above the other, or side by side), or open a second tab on it.
- Column mode (**Alt+C**), or **Alt**+drag for a block selection; typing then works on every selected line.
- Columns: insert or fill, cut, delete, justify left/centre/right, number them, and add up the numbers.
- Lines: move up and down, join, sort A to Z or Z to A, indent, outdent, trim leading or trailing spaces.
- Change case: UPPER CASE, lower case, Sentence case, Title Case.
- Text objects: select, replace or delete inside or around quotes, brackets, tags, a word, a line or a paragraph.
- Select every copy of the selection, or to the matching bracket.
- Insert the date, time, file name or path, a GUID, a page break, a colour, a byte by its value, or another file.
- Snippets (**Ctrl+J**) with `$CURSOR`, `$SELECTION`, `$DATE`, `$TIME` and `$FILE` filled in.
- Swap with clipboard (**Ctrl+Shift+V**) puts the selection on the clipboard and the clipboard in its place.
- Undo as a tree: undoing and then typing keeps the undone branch; the Edit history shows every state.
- Edit history can be kept with the file between sessions.
- History of this part (**Ctrl+Alt+H**): the changes to just the selected lines or function, each of which can be taken back on its own.
- Narrow to the selection (**Ctrl+Shift+N**): every command then works on just those lines.
- Folding, hiding chosen lines, and showing only the lines that contain the selection.
- Colour tags (**Ctrl+Shift+1** to **6**) mark sections in six colours, shown in the margin and the document map, and kept with the file.
- Bookmarks, and back and forward (**Alt+Left**, **Alt+Right**) through where the cursor has been, across files.
- Show line numbers, ruler, document map, changed lines, spaces and tabs, line endings, control characters by name, rainbow brackets and the cursor's line and column.
- Spell checking with Windows' own dictionary, in Windows' language; **F7** goes to the next mistake with suggestions.

## Laying out text and code

- Reindent by brackets; tabs to spaces and back; break long lines at a column; join each paragraph into one line.
- Paragraphs: reformat to a width and margins, align left, centre, right or justify.
- Lay out code: HTML, XML, JSON, JavaScript, CSS, SQL and C-like code (brace on the same line or its own line).
- Compress HTML, XML, JSON, JavaScript and CSS onto as few lines as possible.
- EDI files: one segment to a line, or all onto one line.
- Comma-separated text to fixed-width columns and back.
- Comment out and uncomment lines in the file's language.
- The formatters only ever change spacing; if anything else would change, the text is left alone.

## Table view

- **View › Table view** (**F6**) shows comma-, tab-, semicolon- or pipe-separated data as rows and columns. A file that looks like such data is offered as a table when it's opened, and .csv and .tsv files can open as tables straight away.
- The text is still the document: changing a cell changes only that field, keeping every other field's quoting and spacing as it was, so one Undo takes it back and saving writes exactly what you'd expect.
- Type or double-click to change a cell; Enter keeps it and moves down, Tab moves across, Esc leaves it as it was.
- Select a block of cells with Shift and the arrow keys, Shift+click or by dragging. Copy puts it on the clipboard as a spreadsheet would, so it pastes into Excel or LibreOffice; a block copied from a spreadsheet pastes in, adding rows past the end, as one step for Undo. Delete empties the block, and one value pasted over it fills every cell.
- The total, average, smallest and largest of the numbers in the selection, or in the cursor's column, are shown above the table.
- Fields in quotes may hold the delimiter, quotes and line breaks (RFC 4180); a quote inside an unquoted field is just a character.
- Click a heading to show the rows in order of that column (numbers as numbers), and type to show only the rows that contain something; neither changes the file. **Sort the file by this column** does, keeping the headings at the top.
- Insert and delete rows and columns. Rows with the wrong number of fields, or a quote that's never closed, are marked.
- The first row can be headings or data; columns can be dragged wider or narrower, or double-clicked to fit. The row numbers, and the first column if asked, stay in view when the table is scrolled sideways. Find moves the table to the row it finds.
- It works on files under 64 MB; it isn't a spreadsheet, and cells don't hold formulas.

## Hex editing

- **Ctrl+H** switches any file between text and hex.
- The character pane can show the file's own character set, or EBCDIC.
- Overwrite or insert bytes; insert or delete a number of bytes at the cursor.
- Go to an offset in decimal or hex (`0x...`).
- Search for hex byte sequences as well as text.
- Copy the selected rows as they look on screen.
- String explorer: lists readable strings in the bytes and how each is stored (length prefix, zero terminator, fixed-width padding, top bit set on the last character, `$` terminator, or line ending); can also find UTF-16 strings.

## Large files

- Files of any size open quickly: those of 64 MB and more are kept on disk and read a part at a time (tested with a 10 GB file).
- Edits are kept in memory beside the untouched parts of the file, and saving copies the untouched parts straight across in the background.
- Hex mode works on files of any size, binary or text: a large binary file opens straight into hex. Typing, inserting, deleting, undo, search and Replace All all work, and the offset column widens past 4 GB.
- Search and Replace All in a huge file run a stretch at a time, with progress shown, so the window stays usable.
- Recovery copies of a huge file hold only the edits, not the whole file.
- Word Count and Save a copy in another encoding go through a huge file in the background.
- The string explorer looks through a huge file a stretch at a time (up to its first 200,000 strings).
- A few whole-file commands (reindent, compare, filtering, hiding lines, table view, printing, converting the encoding in place) are still not available above 64 MB.

## Character sets and encodings

- 178 single-byte character sets, plus UTF-8, UTF-16 and UTF-32 (both byte orders), UTF-7, and East Asian encodings (Shift-JIS, EUC-JP, ISO-2022-JP, GBK, GB 18030, Big5, EUC-KR).
- Windows and ISO-8859 code pages, KOI8, and 20 DOS code pages (CP437, CP850, CP866 and others).
- 18 classic Macintosh encodings, HP Roman-8, DEC Multinational, NeXTSTEP and Palm OS.
- 22 EBCDIC code pages for IBM mainframes, with and without the euro.
- 8-bit home computers: Commodore PETSCII and screen codes (C64/C128, VIC-20, PET), Atari ATASCII and screen codes, Atari ST, ZX Spectrum, ZX80, ZX81 and QL, BBC Micro and Mode 7, Acorn Atom, RISC OS, Amiga, Amstrad CPC and CP/M Plus, MSX (five regional sets), Apple II, TRS-80, Sharp MZ and X1, Oric, Kaypro, Osborne and more.
- 26 teletext national character sets.
- Opening and saving give back the same file byte for byte; bytes with no Unicode equivalent are carried through exactly.
- Each line keeps its own line ending: CRLF, LF, CR, ATASCII (0x9B), EBCDIC NL (0x15), or fixed-width rows with none.
- Change the encoding or line ending from the status bar; reopen with another encoding, convert, or save a copy in another.
- Character table panel: every byte of the current character set with its glyph, name and Unicode value, and a Unicode browser.
- Encoding preview panel: the selected text or the character at the cursor as code points and as the bytes it is stored as.

## Kinds of computer (presets)

**Format, Encoding, Kind of computer for new files** fills in character set, line endings and display together. The 21 presets:

- Modern Windows, Legacy Western Windows, DOS US, DOS Western Europe
- Modern macOS, Classic Macintosh, Linux / Unix
- Amiga plain text, Amiga ANSI (with Amiga console sequences), PC ANSI art
- Commodore PET / VIC-20 / C64, Atari ST, Atari 8-bit
- ZX Spectrum text, BBC Micro text, Teletext (40-column rows)
- IBM mainframe text (EBCDIC with NL), IBM mainframe records (fixed 80-byte EBCDIC)
- CP/M text, MSX text, Generic 7-bit ASCII

## Syntax highlighting

- The large set of languages bundled with the two-face collection (C, Rust, Python, HTML, Markdown, SQL and many more).
- 39 grammars written for Limoniter, mostly for older systems:
  - Assemblers: 6502, 65816, 6809, 680x0, 8080, 80x86, Z80, ARM, AVR, PIC, PDP-11, VAX, IBM HLASM.
  - BASICs: Commodore, Sinclair, QBasic, AMOS, Blitz Basic, STOS.
  - Mainframe and business: COBOL, JCL, PL/I, RPG, REXX, MUMPS, xBase.
  - Others: APL, Forth, Logo, Modula-2, Prolog, Smalltalk, PostScript, RTF, Inform 6, ZIL, CONFIG.SYS.
  - Amiga: AmigaGuide and the Amiga Installer language.
- Choose the language by hand under **Format, Colour as**.

## Previews

Shown beside the text or filling the window (**View, Preview**). Nothing in a preview runs or fetches anything.

- Markdown, with headings, lists, tables, emphasis, code and links.
- PC ANSI art (`.ans`, `.ice`, `.diz`, `.nfo` and others) with ANSI.SYS colours, iCE colours and the SAUCE record; also BinaryText.
- Commodore PETSCII streams (`.seq`), drawn with colour codes, reverse and cursor movement, and Commodore screen memory.
- Teletext and BBC Mode 7 pages, with mosaics, double height, concealing and all national sets; `.ep1`, `.tti` and `.ttix`.
- ZX Spectrum text with its colour and position controls.
- Tokenised BASIC programs, listed as the machine's LIST would show them.
- Pictures in older machines' formats (below).

## Pictures

Shown at the machine's own pixel shape, with details from the file, and saved as PNG pixel for pixel.

- Amiga IFF ILBM and PBM, including HAM, HAM8, Extra Half-Brite and deep pictures; colour cycling (Deluxe Paint and Graphicraft) can be played.
- Atari ST Degas, Degas Elite (packed or not) and NEOchrome, with playable colour cycling.
- Commodore 64: Koala Painter, OCP Art Studio, Advanced Art Studio, Doodle!.
- ZX Spectrum screens (`.scr`), with or without a +3DOS header.
- MacPaint, with or without a MacBinary header.
- RISC OS sprite files, one sprite or a sheet of many.
- Kodak Photo CD (`.pcd`) at its highest resolution, up to 16Base (3072 x 2048).
- ZSoft PCX (monochrome, EGA, 256-colour and 24-bit) and GEM bit images (`.img`, with XIMG palettes).
- Apple II hi-res screens, in the colours an NTSC monitor gave them.

## Disk, tape and archive browser

Images and archives open as a read-only browser tab. Folders open in place, an image inside an image opens a level further in, and a file opens as a read-only document, so previews, BASIC listings and encodings all work on it. Files can be saved out exactly as they are, the image itself can be viewed in hex, and **Info** shows everything the image says about itself. Large images are read from disk a part at a time.

| System | Formats |
|---|---|
| CD-ROM | ISO 9660 with Joliet and Rock Ridge names; raw `.bin` images (open the `.bin`, not the `.cue`) |
| PC / DOS | FAT12, FAT16 and FAT32 floppy and hard disk images (`.img`, `.ima`, `.vfd`, `.vhd`), with MBR partitions and long names |
| Atari ST | `.st` and packed `.msa` floppies |
| MSX | FAT disk images |
| Commodore | 1541, 1571 and 1581 disks (D64, D71, D81); T64 tapes |
| Atari 8-bit | ATR and XFD disks (single, enhanced and double density) with Atari DOS 2.0/2.5, MyDOS (with folders) or SpartaDOS (with folders and dates); text opens as ATASCII |
| Tandy Color Computer and Dragon 32/64 | JVC (`.dsk`), VDK and OS-9 (`.os9`) images, and DMK; RS-DOS (Disk Extended Color BASIC), Dragon DOS, and OS-9 with its folders, dates and permissions |
| TRS-80 | Model I, III and 4 disks as DMK, JV3 or JV1 images, with TRSDOS 2.3, Model III TRSDOS 1.3, or LDOS, LS-DOS, TRSDOS 6 and NEWDOS; copy-protected disks open as their sectors |
| Apple II | DOS 3.3 and ProDOS disks (`.dsk`, `.do`, `.po`, `.2mg`, `.hdv`) in either sector order, 800K and hard disk ProDOS volumes with folders; Applesoft programs, text and hi-res screens recognised |
| Amiga | ADF floppies (OFS, FFS and long-name variants), gzipped ADZ, DMS, HDF hard disks with partitions, LHA/LZH archives, Amiga Forever RP9 packages |
| ZX Spectrum | +3DOS DSK disks, TR-DOS (`.trd`) and SCL, Microdrive cartridges (`.mdr`), TAP and TZX tapes, Z80 and SNA snapshots (BASIC, screen and memory) |
| Amstrad CPC / PCW | DSK disks (standard and extended), AMSDOS and CP/M |
| Acorn | BBC/Electron DFS (`.ssd`, `.dsd`, Watford 62-file), ADFS S/M/L (`.ads`, `.adm`, `.adl`), RISC OS ADFS D/E/F (`.adf`), UEF tapes |
| Macintosh | MFS and HFS floppies, hard disks and CDs, Apple partition maps, Disk Copy 4.2 images; both forks and type/creator shown |
| CP/M | 82 machine formats (Kaypro, Osborne, Epson QX-10, Apple II CP/M, 8-inch IBM and more), worked out automatically from the directory |
| HP | LIF disks and tapes (Series 80, 9000 Series 200/300, Integral PC, HP-41/71B/75); LIF ASCII files offered as text |
| Floppy archivists | ImageDisk (`.imd`), Teledisk (`.td0`), DMK, JV3 and JV1, unread sectors listed, the file system inside opened (FAT, CP/M, TRS-80 and more) |
| Archives | ZIP (stored and deflated), LHA/LZH (all common methods, CRC checked) |

- Commodore file names are shown in PETSCII; Spectrum BASIC from tapes, disks and snapshots comes out as +3DOS files.
- A damaged or unexpected image gives an error rather than a wrong file. Nothing is ever written back.

## Documents and conversions

- WordStar 3.x to 7 documents and HP MemoMaker documents, found inside disk images, can be opened as text or converted to Markdown, plain text or RTF.
- Bold, underline, italics, page breaks, headers and footers are kept; what can't be carried over (printer codes, fonts, margins) is listed in notes.
- Pictures can be saved as PNG; tokenised BASIC can be opened as a text listing and saved back as a program.

## BASIC programs

- Listed: Commodore BASIC V2 (C64, VIC-20, PET; `.prg`, `.p00`), ZX Spectrum 48K and 128K, Spectrum Next NextBASIC, BBC BASIC II and IV, MSX-BASIC, Atari BASIC, Microsoft BASIC-80 (MBASIC), Applesoft BASIC, Color BASIC (Tandy Color Computer) and Dragon BASIC.
- Tokenised from a listing: Spectrum (saved as `.tap`), Spectrum Next (`.bas`), Commodore (`.prg`) and BBC BASIC.
- Listing and tokenising undo each other: tokenising a listing gives back the original bytes.
- Commodore control codes appear as `{clr}`, `{red}` and so on, as `petcat` writes them.
- MSX-BASIC and Atari BASIC are listed only; protected MBASIC programs are reported, not listed.

## Searching and comparing

- Find and replace with match case, whole word, regular expressions and hex bytes; search history and favourites.
- Find in Files and Replace in Files over a folder (with subfolders and file types), the open files or a project; optional `.bak` copies.
- Searching runs in the background, so the window stays responsive.
- Compare with another file: two files side by side with differences marked.
- Go to line, matching bracket, next and previous change, next and previous colour tag.

## Panels

Switched on under **View, Panels**:

- Files on this computer, Project, Open files, and File lists (recently opened files).
- Functions in the current file, Bookmarks, Tags (HTML and Markdown tags to wrap a selection, plus your own), Edit history.
- Macros and Scripts.
- File properties: size, counts, SHA-256 of the saved file, and everything a disk image or archive says about itself.
- JSON and XML managers showing the document as a tree.
- Characters (the Character table), Encoding preview, String explorer.
- Clipboard history (the last 50 items, never saved to disk).
- An Output panel for commands, scripts and the assistant.

## Tools

- Count words; SHA-256 fingerprint; convert a number between decimal, hex and binary; copy the file's path.
- Run a command (**F5**) in the file's folder, with `%FILE%`, `%NAME%`, `%STEM%` and `%DIR%`; output goes to the Output panel.
- Projects: a named set of files with notes, backups and restore; paths kept relative so a project can be moved.
- File, This file: rename, show in Explorer, open in its usual program, or split into parts. Save just the selection.
- Coding assistant (**Ctrl+Shift+A**): asks Claude Code or Codex, if installed and signed in, to fix, clean up, explain or write code. Changes are shown first and come off with one Undo. The code is sent to Anthropic or OpenAI by that tool.
- Offer Limoniter to Windows as the editor for chosen file types (Settings, File types).

## Scripts and macros

- Record what you type and the commands you use (**Ctrl+Shift+R**), and play it back (**Ctrl+Shift+P**), once or several times.
- A macro is a script in the Rhai language that you can open and change; you can also write scripts from scratch.
- Scripts can reach only what Limoniter offers them: no files, other programs or network of their own.
- A script runs on its own thread with a Stop button; one Undo takes back everything it did.

## Remote files

- **File › Open a remote file (SFTP)** connects to a server, browses its folders and opens a file; **Save to a server** puts any document there.
- A remote file is edited in a copy on this computer, so everything works on it, very large files included; each Save sends it back in the background, to a new file beside the old one first so the server's copy is never half-written.
- If someone else changes the file on the server meanwhile, Save asks before replacing it.
- Sign in with a password or a key file. Passwords can be kept in Windows Credential Manager, never in Limoniter's settings.
- The server's key is shown to be trusted the first time; after that a different key is refused, in case someone is in between.

## Send to device

- **File › Send to device** sends a copy of the file to the machine it's for; the file goes on saving where it always does. **Ctrl+Alt+S** sends it again to the same place under the same name.
- Devices are set up once, by name, in **Manage devices**:
  - A card or stick found by its volume label, whatever drive letter or mount point it has today (a Gotek's stick, an SD2IEC, a MiSTer's or an Amiga's card). It can be ejected once the file is on it.
  - Any folder: a shared network folder, an emulator's folder, or a TNFS server's folder for FujiNet.
  - An SFTP server (a MiSTer, a Raspberry Pi), with the server's key checked as for remote files.
  - An FTP server, such as an Ultimate 64's or an Amiga's.
  - A ZX Spectrum Next over Wi-Fi: Limoniter serves the file, and `.sync` on the Next fetches it (Jari Komppa's NextSync, versions 3 and 4).
  - An Ultimate 64 or 1541 Ultimate, which runs a `.prg` or `.crt`, plays a `.sid` or `.mod`, or puts a disk image in drive A straight away (firmware 3.11 or later).
  - A command line of your own, given the copy's path, such as an emulator.
- The copy can be written in a machine's own character set and line endings (any system preset) without changing the file.
- On a card or in a folder the copy is written under a temporary name, flushed, and then renamed, so a card pulled out straight afterwards never holds half a file. If the copy there has been changed since it was last sent, or a file of that name wasn't sent from here, you're asked first.
- An add-on's `on_send` function runs first, and can see the device (`send_device()`) and change the name the file is sent as (`set_send_name()`).
- Passwords for FTP and the Ultimate are kept, if you ask, in the system's credential store.

## Add-ons

- An add-on is a Rhai script that adds its own commands to **Tools › Add-ons** and the command box (F1).
- It can run by itself when a file is opened (`on_open`), just before Save writes it (`on_save`), or just before Send to device sends it (`on_send`), for example to tidy a file as it's saved.
- It can show its results in panels of its own (`show_panel`, `add_to_panel`).
- Add-ons live in your own add-ons folder and in a shared folder, so a team can keep one set together.
- A new add-on is off until you turn it on, and one whose file changes is turned off again until you look at it; like any script, it has no files, programs or network of its own.

## Safety

- Unsaved edits are copied to disk as you type, and offered back after a crash, forced close or power cut.
- **File, Recover unsaved edits** offers them again later; damaged copies are detected by checksum.
- Saving never changes bytes you didn't edit.
- Disk images and archives are only ever read; files inside them open read-only.
- Files can be opened read-only.

## Printing and display

- Print with Windows' print dialog, with an optional header and line numbers; hex view can be printed too (up to 16 MB).
- Font choice, larger and smaller text, full screen and always on top.

## Themes

- Two kinds of theme: a **window theme** for the menus, panels and boxes, and a **document theme** for the text and everything drawn with it, chosen under **View, Theme**.
- Each document gets its own document theme, by rules about its file ending, language, character set, or whether it's in hex or table view; the first rule that fits wins. Out of the box, Commodore, Atari, Spectrum, BBC and teletext, and Amiga text open in their machine's colours. Any document can also be given a theme of its own.
- 18 document themes, from Limoniter's own and Solarized, Nord, Gruvbox, Dracula and Monokai to Commodore 64, Atari 8-bit, ZX Spectrum, Amiga Workbench, teletext, DOS and green and amber screens, each with one of 29 sets of syntax colours. 8 window themes.
- **View, Theme, Themes and rules** makes your own: copy a theme and change any colour, or make a whole theme from three colours; see it on a sample as you go. Themes can be saved to a file and opened, to share them.
- Files opened from Explorer join the open window as tabs, or can each have a window of their own.
