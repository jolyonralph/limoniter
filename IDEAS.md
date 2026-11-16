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
| **Real files from the Amstrad PCW, Cambridge Z88 and Amstrad NC100** to test against: LocoScript documents, Mallard BASIC, PipeDream files, BBC BASIC programs, EPROM and memory card images, PCW disk images; and an NC ROM dump, to confirm its character set (taken from a fan site's picture of the ROM font) | Files or images from anyone who has them; formats read from documents alone tend to break on real files | ✓ | S | Amstrad PCW, Z88 and NC100 |
| **Real SFL source** to check the ICL 2900 disassembler's notation: how SFL writes the store-to-store functions' lengths, masks and literals, and the conditional jumps' masks, which the order code manual doesn't show | A listing or a program from someone who wrote SFL | | S | ICL order code |
| **Web search** for the selected text | Deciding whether sending text to an outside service is wanted | | S | MISSING_FEATURES 49 |

## B. Claude can do alone

| Idea | Why | 0.10 | Size | From |
|---|---|---|---|---|
| **Tile and cascade documents**, and **scroll two views together** outside Compare | Several files side by side, beyond the split view; the window shows one document at a time today, so this needs room made for several | ✓ | M | MISSING_FEATURES 42 |
| **BBC Micro development tools**: BeebAsm as an installed assembler, a guide to the MOS calls (OSWRCH, OSBYTE, OSWORD), vectors and the 6845, System VIA and video ULA on hover, Mode 7 and mode 0–6 starters, and a debugger (through b2's or another emulator's remote protocol, as with VICE) | Matching what the Amiga, ST and C64 got in 0.9 | | L | asked 15 November 2026 |
| **More 6502 machines for F9**: Acorn Electron (the BBC's MOS calls, a disc or UEF tape; Elkulator or MAME), Atari 8-bit (an `.xex`, run in Altirra or atari800), Apple II (a DOS 3.3 disk that BRUNs it; AppleWin), Oric (a `.tap`; Oricutron), and VIC-20, Plus/4 and PET as choices for `; machine:` beside the C64, since VICE runs them already | 6502 source runs only on a C64 or a BBC Micro today | | M each | 6502 machine choice |
| **VMS record formats**: variable-length records (a 2-byte length before each, padded to even) and VFC print control, read as lines and saved back the same, beside EBCDIC NL and fixed-width | Files copied off VMS in binary mode show the lengths as junk; the most common legacy-file problem |  | S–M | Legacy formats discussion |
| **SIMH `.tap` tape images** in the browser: each record's 4-byte length before and after it, tape marks between files, ANSI and IBM labels | The standard container for archived 9-track tapes from DEC, IBM and ICL, on bitsavers and elsewhere |  | M | Legacy formats discussion |
| **VMS BACKUP savesets** (`.bck`) in the browser, on their own or inside a SIMH tape | How most VMS software and data was archived |  | M–L | Legacy formats discussion |
| **DEC disks**: Files-11 ODS-2 (VMS), ODS-1 (RSX-11), RT-11 and RSTS/E images in the browser, read-only to begin with | SIMH disk images of PDP-11s and VAXes |  | L each | Legacy formats discussion |
| **A VAX disassembler**, beside the Z80, 6502 and 6809 ones | VAX MACRO is highlighted already |  | L | Legacy formats discussion |
| **Packed 6-bit and 5-bit text**: characters across word boundaries rather than one a byte, with shift characters; then ICL 1900 (24-bit words, four characters each), DEC SIXBIT (PDP-10, 36-bit words), CDC display code (60-bit words), UNIVAC Fieldata and Baudot/ITA2 teleprinter tape | Every character set today is one byte a character; this opens up the mainframes and minis before 8-bit bytes. ICL 1900 tape images from the George 3 emulator community to test with. Exact 64-code tables for ICL 1900 (`ICL_Character_Codes.htm`: digits at 00, space at 20, £ at 24, letters from 41) and Univac Fieldata (`UnisysFieldata.htm`: @ at 00, letters from 06, digits from 60) are at nssdc.gsfc.nasa.gov/nssdc/formats/ |  | L | Legacy formats discussion |
| **ICL 2900 / VME EBCDIC** as one more EBCDIC page: the table, 00 to FF with graphic characters, paper tape and card codes, is in the North Staffs Polytechnic SFL handout (c. 1982, a scan, so read from the page images); then VME's tape and file formats if they can be found documented | ICL 2900 and Series 39 data; Buckle's *The ICL 2900 Series* (1978) has no table |  | S (table), unknown (formats) | Legacy formats discussion |
| **DEC Technical** character set (the VT220's maths and Greek; DEC Special Graphics is done) | DEC documents with formulae. Its table is drawn on Wikipedia but not given as Unicode, and several of its pieces (the large sigma's parts) need choosing with care | | S | Legacy formats discussion |
| **Old machines' numbers in the value inspector**, beyond IBM's and the VAX's (whose F, D, G and H floats, also the PDP-11's, are done): Data General Eclipse and Xerox Sigma 9 (base 16, like IBM's; Sigma 5 two's complement); IBM 1130/1800 (16-bit words); Modcomp (two's complement); SEL (as IBM); and, with 36-, 48-, 60- and 18-bit words read from the bytes they were packed into, the PDP-10, IBM 7090/7094, GE-635, Univac 36-bit and 418, CDC 1604/3000 (48-bit) and 6000/7000 (60-bit, ones' complement), and the PDP-1 | Scientific data from these machines survives in archives such as NASA's NSSDC, whose page per machine (nssdc.gsfc.nasa.gov/nssdc/formats/) gives the layouts; some have mistakes (the CDC 6000 page's bias of 975), so each needs checking against the manufacturer's manual | | M–L | NSSDC formats page |
| **More mainframe numbers in the value inspector**: 128-bit IBM hexadecimal floating point, and ICL 2900 descriptors (two words: type, size, bound or length, byte address) | The 32- and 64-bit IBM floats and packed decimal are done; the descriptors' layout is in Buckle's *The ICL 2900 Series*, Appendix B | | S | Legacy formats discussion (Buckle) |
| **SFL highlighting**, and ICL 2900 code followed from where it starts (as the Z80, 6502 and 6809 are) rather than listed halfword by halfword | The disassembler lists it; code mixed with data shows the data as instructions | | M | ICL order code |
| **More from x86 programs**: imported functions named where they're called (PE import table, ELF PLT and dynamic relocations); data that's referred to labelled too; Win16 (NE), OS/2 and VxD (LE/LX), Mac (Mach-O) and OMF `.obj` files recognised; a DOS `.EXE` shown with its relocations applied at a load segment | Calls into Windows and the C library are what make a program readable | | M | x86 disassembler |
| **BBC BASIC (Z80) programs**, as the Cambridge Z88 and Amstrad NC100/NC150/NC200 save them: listed, typed back and tokenised (the BBC's tokens, Z80 BBC BASIC's line layout) | One piece of work for two machines | ✓ | M | Amstrad PCW, Z88 and NC100 (priority) |
| **Amstrad PCW**: LocoScript 1–4 documents to text or RTF with their styles; 720K disks and the disk format read from the boot sector, as the PCW reads it; Mallard BASIC listed and tokenised; programs run in JOYCE or MAME | LocoScript is what most PCW owners' files are | ✓ | L | Amstrad PCW, Z88 and NC100 (priority) |
| **Cambridge Z88**: PipeDream documents and spreadsheets to text or CSV; EPROM and flash card images (`.epr`, file cards and application cards) in the browser; OZ calls (RST 20h and a call code) named in the Z80 disassembler; programs run in OZvm or MAME |  | ✓ | L | Amstrad PCW, Z88 and NC100 (priority) |
| **Amstrad NC100, NC150 and NC200**: memory card images in the browser; the word processor's documents (Protext-based, so mostly read already); the NC's machine-code program format; programs run in NC100em or MAME |  | ✓ | M–L | Amstrad PCW, Z88 and NC100 (priority) |
| **A 68000 disassembler**, for Atari ST programs, Amiga hunk executables and raw code, followed as the Z80, 6502 and 6809 are | The 68000 can be assembled, run and debugged but not disassembled | | M–L | File formats page |
| **Color BASIC and Dragon BASIC tokenised** from a listing, so programs can be added to Color Computer disks and tapes as BASIC | They're listed but not written | | M | File formats page |
| **Huge files**: fewer commands set aside above 64 MB (Fold all, the function list, previews, BASIC listings, part history) | Every command available, as UltraEdit and EmEditor manage | | L | FEATURES, COMPARISON |
| **Huge files**: UTF-16 files and hex mode above 1 GB; very long lines without slowing the window | Today these are read whole | | L | MISSING_FEATURES (Huge files) |
| **Open files over FTP**, not only send them | SFTP opens and saves; FTP only sends a copy | | M | COMPARISON |
| **Remote search and a remote terminal** over SSH | Working on a server's files as on your own | | L | COMPARISON |
| **A catalogue of add-ons**, and add-ons that add file formats or kinds of window | Sharing Rhai add-ons | | L | COMPARISON |
| **More table tools** for CSV | EmEditor's table tools go further | | M | COMPARISON |
| **Encryption** of files | Needs real care | | M | MISSING_FEATURES 38 |
| **Email** the file from the File menu | Moving it to the Recycle Bin is done. A mail program can't be handed an attachment the same way everywhere (MAPI on Windows, which the new Outlook doesn't take; Mail on a Mac; xdg-email on Linux) | | S | MISSING_FEATURES 41 |
| **Several clipboards** to choose between, beyond the clipboard history | | | S | MISSING_FEATURES (Partly there) |

## Done

| Idea | Version |
|---|---|
| A universal Mac app, for Intel Macs as well as Apple Silicon | 0.9.1 |
| 6502 source on the BBC Micro: source that calls the MOS goes on a disc that *RUNs it, and runs in b2 with F9 | 0.10 (built, not yet released) |
| Character sets for the Amstrad PCW (text, renamed so it's found, and its screen set), the Cambridge Z88 and the Amstrad NC100, NC150 and NC200 | 0.10 (built, not yet released) |
| An x86 disassembler (8086 to AVX-512): Tools, Disassemble, for DOS .COM and .EXE, Windows PE and Linux ELF files, or plain code | 0.10 (built, not yet released) |
| An ICL 2900 disassembler: Tools, Disassemble lists 2900 code in SFL, from ICL's *2900 Order Code* | 0.10 (built, not yet released) |
| A file formats page on the website (`FORMATS.md`, made into `web/formats.html`): every format with what's read, what's written and how it opens | 0.10 (built, not yet released) |
| 6502 source whose code doesn't say whether it's for a C64 or a BBC Micro: F9 asks, and `; machine: c64` or `; machine: bbc` in the source says | 0.10 (built, not yet released) |
| Tests run on an Intel Mac in CI as well as Apple Silicon, and on Ubuntu 26.04 beside 24.04 before `ubuntu-latest` moves (in `tests.yml`; not yet run on GitHub) | 0.10 (built, not yet released) |
| Format, Convert: Base64 encode and decode, JSON to XML and XML to JSON | 0.10 (built, not yet released) |
| Your own highlighting grammars: `.sublime-syntax` files in the grammars folder beside the settings | 0.10 (built, not yet released) |
| Help, Check for a newer Limoniter, and once a day by itself, which can be turned off; not for the Store's or Flathub's copies | 0.10 (built, not yet released) |
| Tools, Save settings to a file and Restore settings from a file | 0.10 (built, not yet released) |
| Window, Layouts: named arrangements of the panels; Window, Gather windows | 0.10 (built, not yet released) |
| Column markers at chosen columns, and line spacing | 0.10 (built, not yet released) |
| Web pages (HTML) in the live preview | 0.10 (built, not yet released) |
| The value inspector panel: integers and IEEE floats either way round, IBM hexadecimal floats (32 and 64-bit), VAX F, D, G and H floats, packed decimal and RADIX-50 | 0.10 (built, not yet released) |
| ISO 646's national sets, DEC Special Graphics, and ASCII with the top bit set (Prime), with 0x8D 0x8A line ends | 0.10 (built, not yet released) |
| Old word processors' documents opened by themselves shown converted, and Format, Save the document as | 0.10 (built, not yet released) |
| PNG and JPEG shown as pictures when opened | 0.10 (built, not yet released) |
| Apple II Integer BASIC listed, from DOS 3.3 and ProDOS disks | 0.10 (built, not yet released) |
| Edit, Surround with: brackets, quotes, a tag or a snippet | 0.10 (built, not yet released) |
| Format, Tabs and spaces menu, with tabs to spaces in the indent only | 0.10 (built, not yet released) |
| Search, Bookmarks: go to, copy, cut and delete bookmarked lines, delete the rest, swap and clear | 0.10 (built, not yet released) |
| Checking open files for changes every two seconds, and reloading untouched ones without asking, following a log's end | 0.10 (built, not yet released) |
| File properties: owner, the program it opens with, and lines of code | 0.10 (built, not yet released) |
| File, This file, Move to the Recycle Bin (or the Trash) | 0.10 (built, not yet released) |
