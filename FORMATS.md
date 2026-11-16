# File and data formats

This page lists every file and data format Limoniter understands. Read and Write say whether Limoniter can open the format and whether it can save or change it: ✓ fully, ◐ in part (the notes say how far), ✗ not at all. Opens as says what you see: editable text, the hex editor, a formatted document (markup), a picture (image), or a browsable list of files (directory).

**Key:** ✓ yes · ◐ in part · ✗ no

## Text and character sets

Text in any of these encodings opens and saves byte for byte; bytes with no Unicode equivalent are carried through unchanged.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| UTF-8 | — | ✓ | ✓ | text | With or without a byte order mark; any size of file. |
| UTF-16 | — | ✓ | ✓ | text | Both byte orders. |
| UTF-32 | — | ✓ | ✓ | text | Both byte orders. |
| UTF-7 | — | ✓ | ✓ | text | Written as RFC 2152 describes. |
| East Asian encodings (7) | — | ✓ | ✓ | text | Shift-JIS, EUC-JP, ISO-2022-JP, GBK, GB 18030, Big5 and EUC-KR. |
| US-ASCII (7-bit) | — | ✓ | ✓ | text | Also the Generic 7-bit ASCII preset. |
| Windows code pages (10) | — | ✓ | ✓ | text | Windows-1250 to 1258 and 874. |
| ISO 8859 sets (15) | — | ✓ | ✓ | text | ISO-8859-1 to 16. |
| KOI8 sets (4) | — | ✓ | ✓ | text | KOI8-R, KOI8-U, KOI8-T and KOI8-RU. |
| DOS code pages (20) | — | ✓ | ✓ | text | CP437, CP850, CP866 and others. |
| Classic Macintosh encodings (18) | — | ✓ | ✓ | text | Mac Roman and its regional, Symbol and Dingbats variants. |
| Other regional sets | — | ✓ | ✓ | text | TIS-620, VISCII, ARMSCII-8, Georgian, KZ-1048, PT154, Kamenický, KOI7 and others. |
| HP Roman-8, DEC Multinational, NeXTSTEP, Palm OS | — | ✓ | ✓ | text | Workstation and handheld sets. |
| EBCDIC code pages (22) | `.ebc`, `.ebcdic` | ✓ | ✓ | text | IBM mainframe sets, with and without the euro; NL line endings or fixed 80-byte records. |
| Commodore PETSCII and screen codes | `.seq` | ✓ | ✓ | text | C64 and C128, VIC-20 and PET. |
| Atari ATASCII and screen codes | — | ✓ | ✓ | text | Atari 8-bit, with its 0x9B line ending. |
| Atari ST character set | — | ✓ | ✓ | text | Also the Atari ST preset. |
| Amiga character set | — | ✓ | ✓ | text | Amiga plain text and Amiga ANSI presets. |
| Sinclair character sets | — | ✓ | ✓ | text | ZX Spectrum, ZX80, ZX81 and QL. |
| Acorn character sets | — | ✓ | ✓ | text | BBC Micro, BBC Mode 7, Acorn Atom and RISC OS. |
| Amstrad character sets | — | ✓ | ✓ | text | CPC, PCW and CP/M Plus (text and screen), and NC100, NC150 and NC200. |
| Cambridge Z88 character set | — | ✓ | ✓ | text | ASCII with the pound sign, and the international OZ versions' letters at their ISO 8859-1 places. |
| MSX character sets (5) | — | ✓ | ✓ | text | International, Japanese, Brazilian, Russian and Korean. |
| Other home computer sets | — | ✓ | ✓ | text | Apple II, TRS-80, Sharp MZ and X1, Oric, Kaypro, Osborne, Enterprise, TI-99/4A and more. |
| Teletext national character sets (26) | — | ✓ | ✓ | text | Every national option of the teletext standard. |
| ISO 646 national sets (18) | — | ✓ | ✓ | text | The 7-bit national versions of ASCII: UK, German, French, Italian, Spanish, Portuguese, Swedish, Norwegian, Danish, Swiss, Hungarian, Irish and Icelandic. |
| DEC Special Graphics | — | ✓ | ✓ | text | The VT100's line drawing. |
| ASCII with the top bit set (Prime) | — | ✓ | ✓ | text | Every byte has its top bit set; lines end 0x8A or 0x8D 0x8A. |
| Line endings | — | ✓ | ✓ | text | CRLF, LF, CR, ATASCII (0x9B), EBCDIC NL (0x15) or fixed-width rows; each line keeps its own. |

## Documents and markup

Old word processors' documents are converted when they're opened, from a disk image or archive or by themselves (shown laid out beside their raw text, and saved as text, Markdown or RTF with Format, Save the document as); the text formats here are edited directly.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Markdown | `.md`, `.markdown`, `.mdown`, `.mkd`, `.mdtext` | ✓ | ✓ | text + markup | Previewed with headings, lists, tables, emphasis, code and links. |
| HTML | `.html`, `.htm` | ✓ | ✓ | text + markup | Coloured, laid out or compressed onto few lines; previewed as a web page, without its scripts or styles. |
| RTF | `.rtf` | ✓ | ✓ | text | Coloured as RTF; old documents can be converted to it. |
| WordStar document | — | ✓ | ✗ | markup | WordStar 3.x to 7 and HP MemoMaker, converted to plain text, Markdown or RTF. |
| PipeDream document or spreadsheet | — | ✓ | ✗ | markup | Cambridge Z88; documents to text, Markdown or RTF, spreadsheets as tables and CSV. |
| IFF formatted text (FTXT) | — | ✓ | ✗ | markup | Amiga clipboard, NotePad and TextCraft text. |
| ProWrite and IFF WORD document | — | ✓ | ✗ | markup | Amiga; larger type becomes Markdown headings. |
| AmigaGuide hypertext | `.guide` | ✓ | ✓ | text + markup | Edited as coloured text; converted with each node a section and links kept. |
| Protext document | — | ✓ | ✗ | markup | Arnor's Protext from the Amiga, Atari ST, PC and CPC. |
| 1st Word and 1st Word Plus document | — | ✓ | ✗ | markup | Atari ST and Archimedes, each read in its own character set. |
| WordPerfect document | — | ✓ | ✗ | markup | WordPerfect 4.x and 5.x from DOS, the Amiga and the Atari ST; footnotes kept. |
| Signum! document | `.sdo` | ✓ | ✗ | markup | Atari ST Signum! 1 and 2: text, bold, italics and underlining. |
| Final Writer document | — | ✓ | ✗ | markup | Amiga; paragraphs, styles, centred lines, headers and footers. |
| Wordworth document | — | ✓ | ✗ | markup | Amiga; paragraphs, styles, centred lines, headers and footers. |

## Data and tables

Delimited data can be edited as a table, with the text still the document underneath.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Comma-separated values (CSV) | `.csv` | ✓ | ✓ | text | Table view, RFC 4180 quoting, any size of file. |
| Tab-separated values (TSV) | `.tsv`, `.tab` | ✓ | ✓ | text | Table view. |
| Semicolon- and pipe-separated data | `.psv` | ✓ | ✓ | text | Table view, offered when a file looks like such data. |
| JSON | `.json` | ✓ | ✓ | text | Tree panel; laid out or compressed. |
| XML | `.xml` | ✓ | ✓ | text | Tree panel; laid out or compressed. |
| EDI (X12 and EDIFACT) | — | ✓ | ✓ | text | One segment to a line, or all on one line. |

## Programs and BASIC

Tokenised BASIC is listed as the machine's LIST would show it; some dialects can be tokenised back from an edited listing.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Commodore BASIC program | `.prg`, `.p00` | ✓ | ✓ | text | BASIC 2.0 (C64, VIC-20), 3.5 (C16, Plus/4), 4.0 (PET) and 7.0 (C128); control codes as `{clr}`. |
| ZX Spectrum BASIC (48K and 128K) | `.tap`, `.tzx`, `.bas` | ✓ | ✓ | text | Tokenised to a tape; zmakebas and bas2tap escapes understood. |
| ZX Spectrum Next NextBASIC | `.bas` | ✓ | ✓ | text | Tokenised as a +3DOS `.bas` file. |
| OpticBASIC (OpticOS) | `.tap` | ✓ | ✓ | text | Tokenised exactly as the OpticOS ROM does, onto a tape. |
| BBC BASIC II and IV | `.bbc`, `.bas` | ✓ | ✓ | text | Tokenised as SAVE writes a program. |
| Applesoft BASIC | `.bas` | ◐ | ◐ | text | Listed; tokenised only when added to an Apple II DOS 3.3 or ProDOS disk. |
| Apple II Integer BASIC | `.int` | ✓ | ✗ | text | Listed as its LIST shows it; from DOS 3.3 (type I) and ProDOS (INT) disks. |
| BBC BASIC (Z80) | `.bas` | ✓ | ✓ | text | As the Cambridge Z88, the Amstrad NC100, NC150 and NC200 and CP/M keep it; tokenised from a listing. |
| ZX81 BASIC | `.p`, `.81`, `.p81` | ✓ | ✗ | text | Also from TZX tapes, with the screen saved with the program. |
| MSX-BASIC | `.bas` | ✓ | ✗ | text | From disk or tape, saved from 32K or 16K machines. |
| Atari BASIC | `.bas` | ✓ | ✗ | text | Listed only. |
| Microsoft BASIC-80 (MBASIC) | `.bas` | ✓ | ✗ | text | Protected programs are reported, not listed. |
| Color BASIC and Dragon BASIC | `.bas`, `.cas` | ✓ | ✗ | text | Tandy Color Computer and Dragon, from disk or tape, told apart by their tokens. |
| TRS-80 Level II and Disk BASIC | — | ✓ | ✗ | text | Model I and III, from disk or tape. |
| TRS-80 Model 4 BASIC | — | ✓ | ✗ | text | Listed only. |
| SAM BASIC | `.bas` | ✓ | ✗ | text | With MasterDOS's and MasterBASIC's keywords. |
| Oric BASIC | `.tap`, `.bas` | ✓ | ✗ | text | From Oric tapes and Sedoric disks. |
| Commodore program file (PRG) | `.prg` | ✓ | ✓ | hex | Machine code disassembles to 6502 source; the 6502 assembler saves programs. |
| PC64 program file (P00) | `.p00` | ✓ | ✗ | hex | Unwrapped when added to a Commodore disk. |
| Spectrum +3DOS file | — | ✓ | ✓ | text | Recognised by its PLUS3DOS header; written by NextBASIC and the assembler's SAVE3DOS. |
| ZX Spectrum Next program (NEX) | `.nex` | ✓ | ✓ | directory | Versions 1.0 to 1.3; made by the Z80 assembler's SAVENEX. |
| Atari 8-bit binary load file | `.xex`, `.com`, `.exe`, `.obx` | ✓ | ✗ | directory | Segments with load, run and init addresses; disassembled as 6502. |
| Color Computer machine code (LOADM) | `.bin` | ✓ | ✓ | hex | Disassembled as 6809; made by the 6809 assembler. |
| Atari ST program | `.tos`, `.prg`, `.ttp` | ◐ | ✓ | hex | Run and profiled in Hatari but not disassembled; made by the 68000 assembler. |
| AmigaDOS executable (hunk) | — | ◐ | ✓ | hex | Run in an emulator and crunched with Shrinkler or Cranker; not disassembled. |
| Raw machine code | `.bin` | ✓ | ✓ | hex | Disassembled for the Z80, 6502 or 6809; saved by Assemble and save as binary. |
| DOS program (COM) | `.com` | ✓ | ✗ | hex | Disassembled as 16-bit x86 into NASM-style source. |
| DOS program (MZ EXE) | `.exe` | ✓ | ✗ | hex | Disassembled from its entry point, following near and far calls. |
| Windows program or DLL (PE) | `.exe`, `.dll` | ✓ | ✗ | hex | 32- and 64-bit; disassembled from the entry point and every export. |
| Linux program or library (ELF) | — | ✓ | ✗ | hex | Disassembled from the entry point and the functions its symbol table names. |
| ICL 2900 machine code | — | ✓ | ✗ | hex | Disassembled into SFL, every halfword read as an instruction. |

## Assembler source

Assembler source is plain text; Limoniter's own assemblers turn it into programs, or an installed assembler can be used instead.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Z80 assembler source | `.asm`, `.z80`, `.a80`, `.s` | ✓ | ✓ | text | Assembles as sjasmplus does, to `.tap`, `.nex`, `.sna`, +3DOS or raw binary. |
| 6502 assembler source (ACME and ca65) | `.a`, `.asm`, `.s` | ✓ | ✓ | text | Assembles as ACME does, to a C64 `.prg`, or a bootable BBC Micro disc. |
| 6502 assembler source (KickAssembler) | `.kick`, `.ka`, `.kickass`, `.asm` | ✓ | ✓ | text | Everyday KickAssembler syntax assembles to a C64 `.prg`; scripting is left to KickAssembler. |
| 6502 assembler source (64tass) | `.tas`, `.64tass`, `.asm` | ✓ | ✓ | text | Everyday 64tass syntax assembles to a C64 `.prg`; sections are left to 64tass. |
| 6809 assembler source | `.asm`, `.s` | ✓ | ✓ | text | Assembles as lwasm does, to a Color Computer LOADM binary. |
| 68000 assembler source | `.s`, `.asm`, `.i` | ✓ | ✓ | text | Assembles as vasm does, to an Atari ST `.TOS`, an AmigaDOS executable or raw binary. |
| Other assembler source | `.asm`, `.s`, `.mac`, `.src` | ✓ | ✓ | text | 65816, 8080, 80x86, ARM, AVR, PIC, PDP-11, VAX and IBM HLASM are coloured but not assembled. |

## Pictures and screens

Pictures are shown at the machine's own pixel shape beside their bytes, and any of them can be saved as PNG; Write means Limoniter can also make the format.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| PNG | `.png` | ✓ | ✓ | image + hex | Shown as a picture; every picture saves as PNG; PNGs are taken into the converters and screen designer. |
| JPEG | `.jpg`, `.jpeg` | ✓ | ✗ | image + hex | Shown as a picture, and taken into the Amiga, Atari ST and C64 picture converters. |
| Amiga IFF ILBM picture | `.iff`, `.ilbm`, `.lbm`, `.bbm`, `.brush` | ✓ | ✓ | image + hex | HAM, HAM8, Extra Half-Brite and deep pictures; colour cycling plays; made by the converters. |
| Amiga IFF PBM picture | `.iff`, `.lbm` | ✓ | ✗ | image + hex | Chunky Deluxe Paint pictures. |
| Degas and Degas Elite picture | `.pi1`, `.pi2`, `.pi3`, `.pc1`, `.pc2`, `.pc3` | ✓ | ✓ | image + hex | Atari ST, packed or not, with colour cycling; made by the ST converter. |
| NEOchrome picture | `.neo` | ✓ | ✓ | image + hex | Atari ST, with colour cycling; made by the ST converter. |
| Spectrum 512 picture | `.spu`, `.spc` | ✓ | ✓ | image + hex | Atari ST, 48 colours a line, STE Spectrum 4096 too; made by the ST converter. |
| Spectrum 512 smooshed picture | `.sps` | ✓ | ✗ | image + hex | Atari ST. |
| Koala Painter picture | `.koa`, `.kla` | ✓ | ✓ | image + hex | Commodore 64 multicolour; made by the C64 converter. |
| OCP Art Studio picture | `.art`, `.aas` | ✓ | ✓ | image + hex | Commodore 64 hires; made by the C64 converter. |
| Advanced Art Studio picture | `.ocp`, `.mpic` | ✓ | ✓ | image + hex | Commodore 64 multicolour; made by the C64 converter. |
| Doodle! picture | `.dd` | ✓ | ✗ | image + hex | Commodore 64 hires. |
| FLI Designer picture | `.fli` | ✓ | ✓ | image + hex | Commodore 64, 296 pixels across as RECOIL shows it; made by the C64 converter. |
| AFLI picture | `.afl` | ✓ | ✓ | image + hex | Commodore 64; made by the C64 converter. |
| ZX Spectrum screen | `.scr` | ✓ | ✓ | image + hex | 6912 bytes, with or without a +3DOS header; made by the screen designer. |
| ZX Spectrum Next Layer 2 and LoRes screen | `.sl2`, `.nxi`, `.slr` | ✓ | ✗ | image + hex | 256×192, 320×256 and 640×256, with their own palette or the Next's. |
| ZX Spectrum Next palette | `.nxp` | ✓ | ✗ | image + hex | Shown as its 256 colours. |
| MacPaint picture | `.pntg`, `.pnt`, `.mac`, `.macp` | ✓ | ✗ | image + hex | With or without a MacBinary header. |
| RISC OS sprite file | `.spr`, `,ff9` | ✓ | ✗ | image + hex | One sprite or a sheet of many. |
| Kodak Photo CD image | `.pcd` | ✓ | ✗ | image + hex | At its highest resolution, up to 16Base (3072 x 2048). |
| ZSoft PCX picture | `.pcx`, `.pcc` | ✓ | ✗ | image + hex | Monochrome, EGA, 256-colour and 24-bit. |
| GEM bit image | `.img`, `.ximg` | ✓ | ✗ | image + hex | With XIMG palettes. |
| Apple II hi-res screen | `.hgr`, `.hgr1`, `.hgr2`, `.a2hgr` | ✓ | ✗ | image + hex | In the colours an NTSC monitor gave. |
| Atari 8-bit screen | `.gr7`, `.gr8`, `.gr9`, `.gr15`, `.mic`, `.pic`, `.kmp` | ✓ | ✗ | image + hex | Graphics 7, 8, 9 and 15, and MicroPainter with its saved colours. |
| Koala MicroIllustrator picture | `.pic`, `.kpi` | ✓ | ✗ | image + hex | Atari 8-bit, packed, with its saved colours. |
| HIP picture | `.hip` | ✓ | ✗ | image + hex | Atari 8-bit Graphics 9 and 10 screens mixed. |
| Amiga raw bitplanes, sprites and bobs | `.raw`, `.bpl`, `.bin` | ✓ | ✓ | image | Read and made by the Amiga bitplane converter, given width, planes, layout and palette. |
| Atari ST raw screen and planes | `.raw`, `.bin` | ✓ | ✓ | image | Read and made by the ST converter, for the ST, STE, TT and Falcon. |
| Spectrum UDGs and fonts | `.udg`, `.chr` | ✓ | ✓ | image | Opened and saved in the character and sprite editor. |
| ZX Spectrum Next sprites | `.spr` | ✓ | ✓ | image | 16×16 in 256 colours, in the character and sprite editor. |
| Commodore 64 character set | `.64c`, `.bin`, `.prg` | ✓ | ✓ | image | Hires or multicolour, in the character and sprite editor. |
| Commodore 64 sprites (SpritePad) | `.spd`, `.bin`, `.prg` | ✓ | ✓ | image | Hires or multicolour, in the character and sprite editor and the C64 converter. |
| PC ANSI art | `.ans`, `.ice`, `.diz`, `.nfo`, `.pcb`, `.avt`, `.xb` | ✓ | ✓ | text + image | ANSI.SYS and iCE colours, SAUCE, PCBoard and Avatar codes; also BinaryText and XBin. |
| Amiga console text | — | ✓ | ✓ | text + image | Amiga ANSI colour codes drawn as the Amiga draws them, with pens you can set. |
| Commodore PETSCII stream | `.seq` | ✓ | ✓ | text + image | Drawn with colour codes, reverse and cursor movement. |
| Commodore screen memory | — | ✓ | ✓ | text + image | Screen codes drawn as the screen. |
| Teletext and BBC Mode 7 page | `.ep1`, `.tti`, `.ttix` | ✓ | ✓ | text + image | Mosaics, double height, concealing and all national sets. |
| ZX Spectrum text with controls | — | ✓ | ✓ | text + image | Drawn with the Spectrum's colour and position controls. |

## Sound and music

Music files open in hex with everything the file says about itself beside them.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| ProTracker module | `.mod`, `mod.*` | ✓ | ✗ | hex | ProTracker, NoiseTracker, StarTrekker, multichannel and 15-sample Soundtracker modules; not played. |
| C64 SID tune | `.sid` | ✓ | ✗ | hex | PSID and RSID versions 1 to 4; its code opens as a program and disassembles. |
| Atari ST SNDH music | `.sndh`, `.snd` | ✓ | ✗ | hex | Packed or not; tunes, timers and flags described. |
| Atari ST YM music | `.ym` | ✓ | ✗ | hex | YM2!, YM3!, YM3b, YM5! and YM6!, packed with LHA or not. |

## Disk images

Disk images open as a browser of their files. Those marked writable can have files added, deleted and renamed, and new blank ones can be made.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| CD-ROM image (ISO 9660) | `.iso`, `.bin` | ✓ | ✗ | directory | Joliet and Rock Ridge names; raw `.bin` images too (not the `.cue`). |
| PC FAT disk image | `.img`, `.ima`, `.vfd`, `.vhd`, `.dsk` | ✓ | ✓ | directory | FAT12, 16 and 32 with MBR partitions and long names; also MSX disks and NextZXOS SD cards. |
| Atari ST floppy (ST) | `.st` | ✓ | ✓ | directory | FAT floppies. |
| Atari ST packed floppy (MSA) | `.msa` | ✓ | ◐ | directory | Written only as a bootable floppy made for a program. |
| Commodore 1541 disk image (D64) | `.d64` | ✓ | ✓ | directory | PRG, SEQ and USR files; names in PETSCII. |
| Commodore 1571 disk image (D71) | `.d71` | ✓ | ✓ | directory | PRG, SEQ and USR files. |
| Commodore 1581 disk image (D81) | `.d81` | ✓ | ✓ | directory | PRG, SEQ and USR files. |
| Atari 8-bit disk image (ATR) | `.atr` | ✓ | ✓ | directory | Atari DOS 2.0 and 2.5, MyDOS and SpartaDOS 2.x, 3.x and X written; SpartaDOS 1 only read. |
| Atari 8-bit disk image (XFD) | `.xfd` | ✓ | ✗ | directory | Same DOSes as ATR. |
| Apple II disk image | `.dsk`, `.do`, `.po`, `.2mg`, `.hdv` | ✓ | ✓ | directory | DOS 3.3 and ProDOS written; DOS 3.2 and Apple II CP/M only read. |
| Apple II WOZ image | `.woz` | ✓ | ✗ | directory | WOZ 1 and 2, read from their bits into sectors. |
| Cambridge Z88 card | `.epr` | ✓ | ✗ | directory | File EPROM and flash cards, older versions of files shown too; application cards recognised. |
| Amstrad NC memory card | `.card` | ✓ | ✗ | directory | ZCN's CP/M drives, one for each 256K; program cards (NC100PRG) recognised. |
| Amiga floppy (ADF) | `.adf` | ✓ | ✓ | directory | OFS and FFS, DD and HD written; disks with directory caches or long names only read. |
| Gzipped Amiga floppy (ADZ) | `.adz` | ✓ | ✗ | directory | Unzipped as it opens. |
| Amiga DMS floppy | `.dms` | ✓ | ✗ | directory | Unpacked as it opens. |
| Amiga hard disk image (HDF) | `.hdf` | ✓ | ✗ | directory | With partitions. |
| Spectrum +3 and Amstrad CPC disk (DSK) | `.dsk` | ✓ | ✓ | directory | Standard and extended; +3DOS, AMSDOS and CP/M, PCW disks too. |
| ZX Spectrum TR-DOS disk | `.trd` | ✓ | ✓ | directory | BASIC tokenised from a listing; a full disk is compacted as MOVE does. |
| ZX Spectrum TR-DOS archive (SCL) | `.scl` | ✓ | ✓ | directory | Same files as TR-DOS disks. |
| SAM Coupé and +D disk | `.mgt`, `.dsk`, `.sad` | ✓ | ✗ | directory | SAMDOS, MasterDOS and BDOS, and Spectrum +D and DISCiPLE disks. |
| Oric Sedoric disk | `.dsk` | ✓ | ✗ | directory | MFM_DISK images, one or two sides, every sector's CRC checked. |
| Color Computer and Dragon disk (JVC) | `.dsk`, `.jvc` | ✓ | ◐ | directory | Only single-sided RS-DOS disks are written; Dragon DOS and OS-9 are only read. |
| Color Computer and Dragon disk (VDK) | `.vdk` | ✓ | ✗ | directory | RS-DOS, Dragon DOS and OS-9. |
| OS-9 disk image | `.os9` | ✓ | ✗ | directory | With folders, dates and permissions. |
| DMK, JV3 and JV1 sector images | `.dmk`, `.jv3`, `.jv1`, `.dsk`, `.dis` | ✓ | ✗ | directory | TRS-80 (TRSDOS, LDOS, NEWDOS) and other disks; copy-protected disks open as sectors. |
| BBC Micro DFS disk | `.ssd`, `.dsd`, `.img` | ✓ | ✓ | directory | Acorn DFS written; Watford 62-file catalogues only read. |
| Acorn ADFS disk (old map) | `.ads`, `.adm`, `.adl`, `.adf` | ✓ | ✓ | directory | ADFS S, M and L, with free space map and checksums. |
| RISC OS ADFS disc (D, E and F) | `.adf` | ✓ | ✗ | directory | New-map RISC OS discs. |
| Macintosh HFS disk image | `.dsk`, `.img`, `.hfs`, `.toast`, `.cdr` | ✓ | ✓ | directory | Floppies, hard disks and CDs; a disk with one HFS partition can be written. |
| Macintosh MFS disk image | `.dsk`, `.img` | ✓ | ✗ | directory | The first Mac floppies. |
| Macintosh HFS+ disk image | `.dsk`, `.img` | ✓ | ✗ | directory | HFSX, HFS-wrapped and compressed files; Apple partition maps and GUID partition tables. |
| Disk Copy 4.2 image | `.image`, `.dc42` | ✓ | ✓ | directory | HFS disks written with Disk Copy's checksum; Apple II ones only read. |
| Disk Copy 6 NDIF image | `.img`, `.smi` | ✓ | ✗ | directory | In MacBinary, stored or ADC-compressed. |
| Mac disk image (DMG) | `.dmg` | ✓ | ✗ | directory | UDIF with zlib, bzip2, ADC or LZFSE, MacBinary-wrapped too. |
| CP/M disk image | `.img`, `.dsk`, `.cpm`, `.ima` | ✓ | ✗ | directory | 82 machine formats, worked out from the directory. |
| HP LIF disk and tape image | `.lif`, `.hpi` | ✓ | ✗ | directory | Series 80, 9000 Series 200/300, Integral PC, HP-41/71B/75; LIF ASCII files open as text. |
| ImageDisk image | `.imd` | ✓ | ✗ | directory | The file system inside is opened; unread sectors listed. |
| Teledisk image | `.td0` | ✓ | ✗ | directory | The file system inside is opened; unread sectors listed. |
| SuperCard Pro flux image | `.scp` | ✓ | ✗ | directory | Decoded from IBM, Amiga, Commodore, Apple and Mac encodings; the decoded disk can be saved. |
| HxC flux image | `.hfe` | ✓ | ✗ | directory | Decoded as SuperCard Pro images are. |
| KryoFlux stream files | `.raw`, `.zip` | ✓ | ✗ | directory | A folder or ZIP of streams, decoded as SuperCard Pro images are. |

## Tape images

Tape images open as a browser of the files on the tape.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| ZX Spectrum tape (TAP) | `.tap` | ✓ | ✓ | directory | Programs, code, screens and headerless blocks can be added and reordered. |
| ZX Spectrum tape (TZX) | `.tzx` | ✓ | ✓ | directory | Other blocks kept when files are added; ZX81 programs come out as `.p` files. |
| Commodore tape archive (T64) | `.t64` | ✓ | ✗ | directory | Commodore file names in PETSCII. |
| Commodore tape pulses (TAP) | `.tap` | ✓ | ✗ | directory | Versions 0 to 2; KERNAL tape files decoded, turbo loaders described but not decoded. |
| MSX, TRS-80, Color Computer and Dragon cassette (CAS) | `.cas` | ✓ | ✗ | directory | BASIC programs, machine code and data, with checksums checked. |
| Oric tape | `.tap` | ✓ | ✗ | directory | Oric-1 and Atmos BASIC and machine code, with load addresses. |
| Acorn tape (UEF) | `.uef` | ✓ | ✗ | directory | BBC Micro and Electron tapes, gzipped or not. |
| ZX Microdrive cartridge | `.mdr` | ✓ | ✗ | directory | Spectrum Microdrive cartridge images. |

## Archives and compressed files

Archives are only read; their files can be opened or saved out as they are.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| ZIP archive | `.zip` | ✓ | ✗ | directory | Stored and deflated files. |
| LHA and LZH archive | `.lha`, `.lzh` | ✓ | ✗ | directory | All common methods, CRC checked. |
| StuffIt archive | `.sit` | ✓ | ✗ | directory | Classic and StuffIt 5, with folders and both forks. |
| Compact Pro archive | `.cpt` | ✓ | ✗ | directory | With folders, RLE and LZH, every CRC checked. |
| MacBinary file | `.bin`, `.macbin` | ✓ | ✗ | directory | MacBinary I, II and III; both forks and type and creator shown. |
| BinHex 4.0 file | `.hqx` | ✓ | ✗ | directory | CRCs checked; `.sit.hqx` and `.cpt.hqx` open straight through. |
| Amiga Forever package (RP9) | `.rp9` | ✓ | ✗ | directory | The disks and files inside the package. |
| Pack-Ice packed file | — | ✓ | ✗ | hex | Atari ST, versions 2.1 to 2.4; opens read-only as what it holds. |
| Atomik 3.5 packed file | — | ✓ | ✗ | hex | Atari ST; opens read-only as what it holds. |

## Snapshots and memory images

Snapshots open as their BASIC program, screen and memory.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| ZX Spectrum Z80 snapshot | `.z80` | ✓ | ✗ | directory | BASIC, screen and memory; disassembled from the saved PC. |
| ZX Spectrum SNA snapshot | `.sna` | ✓ | ◐ | directory | 48K and 128K; written only by the Z80 assembler's SAVESNA. |

## Cartridges and ROMs

Cartridge images open as a browser of their ROM chips, each a file by bank and address.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Commodore 64 cartridge (CRT) | `.crt` | ✓ | ◐ | directory | Every hardware type read; only normal 8K and 16K cartridges are made, as cartconv makes them. |

## Other

Formats that don't fit the sections above.

| Format | Endings | Read | Write | Opens as | Notes |
|---|---|---|---|---|---|
| Unified diff (patch) | `.diff`, `.patch` | ✓ | ✓ | text | Saved from comparisons and applied to a file or folder. |
