#!/usr/bin/env python3
"""Builds data/charsets-retro.txt: more home and personal computers' character sets, in the same format as
charsets.txt, from the same source and version as its retro sets.

Usage: python3 -I tools/gen_retro.py <folder of kreativekorp/charsets tables> <output>

Source: Rebecca Bettencourt's kreativekorp/charsets, tables/, at commit db9120f7b6e4, read with gen_charsets.py's
own reader. Two things are added here, each said in the set's scope:
- The Apple II tables hold the screen's inverse and MouseText codes in bytes 0 to 127 and the high-bit ASCII its
  text files use in 128 to 255. A low byte repeating a high one is marked inverse, and typing writes the high byte.
- The ZX80's and ZX81's tables stop at 127; bytes 128 to 191 are the inverse of 0 to 63, as the machines draw them.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen_charsets", os.path.join(HERE, "gen_charsets.py"))
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)


def write_table(out, title, rows, back, inverse=()):
    """Like gen_charsets.write_table, with the inverse bytes given rather than worked out."""
    out.write("@table %s\n" % title)
    for byte in range(256):
        row = rows.get(byte)
        if row is None:
            continue
        cp, kind, name = row
        flag = "i" if byte in inverse and kind == "p" else ""
        out.write("%02X\t%s\t%s%s\t%s\n" % (byte, "-" if cp is None else "%04X" % cp, kind, flag, name))
    # Only where the character is written as some byte other than the first that shows it, as charsets.txt does.
    first = {}
    for byte in range(256):
        row = rows.get(byte)
        if row and row[1] == "p" and row[0] is not None:
            first.setdefault(row[0], byte)
    for cp, byte in sorted(back.items()):
        if byte in rows and first.get(cp) != byte:
            out.write("<\t%04X\t%02X\n" % (cp, byte))


def write_set(out, cid, name, group, scope, rows, back, inverse=()):
    out.write("@charset %s\nname: %s\ngroup: %s\nsource: %s\nversion: %s\nscope: %s\nmodel: none\nkeep_bytes: false\n" % (cid, name, group, gen.KR_SOURCE, gen.KR_VERSION, scope))
    write_table(out, "Text", rows, back, inverse)
    out.write("\n")


def apple(rows, back):
    """High-bit ASCII is the text; a low byte with the same character is its inverse (or flashing) screen form."""
    high = {}
    for byte in range(0x80, 0x100):
        row = rows.get(byte)
        if row and row[1] == "p" and row[0] is not None:
            high.setdefault(row[0], byte)
    inverse = set()
    back = dict(back)
    for byte in range(0x80):
        row = rows.get(byte)
        if row and row[1] == "p" and row[0] in high:
            inverse.add(byte)
            back[row[0]] = high[row[0]]
    return rows, back, inverse


def zx_inverse(rows, back):
    rows = dict(rows)
    for byte in range(0x40):
        if byte in rows and (byte | 0x80) not in rows:
            cp, kind, name = rows[byte]
            rows[byte | 0x80] = (cp, kind, ("inverse " + name).strip())
    return rows, back, set(range(0x80, 0xC0))


SETS = [
    ("apple2", "x-apple2", "Apple II and II Plus", "Apple II", "Text as Apple II files keep it, in high-bit ASCII (bytes 128 and up); the low bytes are the screen's inverse and flashing forms, marked inverse.", apple),
    ("apple2e", "x-apple2e", "Apple IIe (with MouseText)", "Apple II", "High-bit ASCII text, with the enhanced IIe's MouseText among the low bytes; the low bytes that repeat a character are its inverse form.", apple),
    ("apple2gs", "x-apple2gs", "Apple IIc and IIgs", "Apple II", "High-bit ASCII text, with MouseText; the low bytes that repeat a character are its inverse form.", apple),
    ("trs80-m1", "x-trs80m1", "TRS-80 Model I", "Tandy", "Text and the 2-by-3 block graphics at 128 to 191.", None),
    ("trs80-m3", "x-trs80m3", "TRS-80 Model III and 4", "Tandy", "Text, the block graphics and the Model III's own characters.", None),
    ("trs80-coco", "x-trs80-coco", "TRS-80 Color Computer and Dragon", "Tandy", "The MC6847's characters as BASIC prints them.", None),
    ("zx80", "x-zx80", "Sinclair ZX80", "Sinclair", "The ZX80's own set, not ASCII. Bytes 128 to 191 are the inverse of 0 to 63.", zx_inverse),
    ("zx81", "x-zx81", "Sinclair ZX81", "Sinclair", "The ZX81's own set, not ASCII. Bytes 128 to 191 are the inverse of 0 to 63; BASIC's keywords aren't in the table.", zx_inverse),
    ("sinclair-ql", "x-sinclairql", "Sinclair QL", "Sinclair", "The QL's set, ASCII with its own upper half.", None),
    ("oric", "x-oricscii", "Oric-1 and Atmos", "Other 8-bit computers", "ORICSCII text.", None),
    ("sharp-mz80k", "x-sharpscii-mz80k-jp", "Sharp MZ-80K (Japanese)", "Sharp", "SHARPSCII as the MZ-80K has it, with katakana.", None),
    ("sharp-mz-eu", "x-sharpscii-mz700-eu", "Sharp MZ-700 and MZ-800 (European)", "Sharp", "SHARPSCII, European version.", None),
    ("sharp-mz-jp", "x-sharpscii-mz700-jp", "Sharp MZ-700 (Japanese)", "Sharp", "SHARPSCII, Japanese version, with katakana.", None),
    ("sharp-x1", "x-sharpx1", "Sharp X1", "Sharp", "The X1's set, with katakana and graphics.", None),
    ("acorn-atom", "x-acorn-atom", "Acorn Atom", "Acorn", "The Atom's text set.", None),
    ("bbc-master", "x-bbc-master", "BBC Master (system font)", "Acorn", "The Master's upper half, as RISC OS's system font has it.", None),
    ("riscos", "x-riscos", "RISC OS (Acorn Latin-1)", "Acorn", "Acorn's Latin-1, with its own characters from 128 to 159.", None),
    ("amstrad-cpc", "x-amstrad-cpc", "Amstrad CPC", "Amstrad", "The CPC's firmware set, with its graphics. CP/M Plus is a separate set.", None),
    ("enterprise", "x-enterprise", "Enterprise 64 and 128", "Other 8-bit computers", "The Enterprise's text set.", None),
    ("aquarius", "x-aquarius", "Mattel Aquarius", "Other 8-bit computers", "The Aquarius's set, with its graphics.", None),
    ("spectravideo", "x-spectravideo", "Spectravideo SV-318 and SV-328", "Other 8-bit computers", "The Spectravideo's text set.", None),
    ("ti994a", "x-ti994a", "Texas Instruments TI-99/4A", "Other 8-bit computers", "The TI-99/4A's text set.", None),
    ("tomy-tutor", "x-tomy-tutor", "Tomy Tutor", "Other 8-bit computers", "The Tomy Tutor's text set.", None),
    ("coleco-adam", "x-coleco-adam-sw", "Coleco Adam", "Other 8-bit computers", "SmartWriter's text set.", None),
    ("osi", "x-osi", "Ohio Scientific", "Other 8-bit computers", "The OSI Challenger's set, with its graphics.", None),
    ("robotron-kc", "x-robotron", "Robotron KC 85", "Other 8-bit computers", "The East German KC 85's set.", None),
    ("kaypro", "x-kaypro", "Kaypro", "CP/M", "Kaypro's CP/M set, with its graphics.", None),
    ("osborne1", "x-osborne1", "Osborne 1", "CP/M", "The Osborne 1's set.", None),
    ("kamenicky", "x-kamenicky", "Kamenický (Czech and Slovak DOS)", "DOS code pages", "The Kamenický brothers' DOS code page, before CP852.", None),
    ("koi7", "x-koi7", "KOI-7 N2 (Russian 7-bit)", "ISO-8859 and others", "Seven bits, with Cyrillic capitals in place of ASCII's small letters.", None),
]


def main():
    folder, output = sys.argv[1:3]
    out = open(output, "w", encoding="utf-8", newline="\n")
    out.write("# Generated by tools/gen_retro.py from kreativekorp/charsets, in the same format as charsets.txt.\n")
    out.write("# Row: byte, Unicode (or -), kind (p printable, c control, s depends on display state; i = drawn inverse), name.\n\n")
    for cid, table, name, group, scope, adjust in SETS:
        rows, back = gen.read_kr(folder, table)
        inverse = set()
        if adjust:
            rows, back, inverse = adjust(rows, back)
        write_set(out, cid, name, group, scope, rows, back, inverse)
    out.close()
    print("%d sets" % len(SETS))


if __name__ == "__main__":
    main()
