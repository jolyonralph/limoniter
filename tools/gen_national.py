#!/usr/bin/env python3
"""Builds data/charsets-national.txt: the 7-bit national versions of ASCII (ISO 646), DEC's line-drawing set, and
ASCII with the top bit always set, in the same format as charsets.txt.

Usage: python3 -I tools/gen_national.py <folder of kreativekorp/charsets tables> <output>

Sources, none invented here:
- ISO 646: Rebecca Bettencourt's kreativekorp/charsets, tables/x-iso-646-*.txt, at commit db9120f7b6e4, the same
  source and version as charsets-retro.txt, read with gen_charsets.py's own reader. Each differs from ASCII at a
  few of the twelve places ISO 646 leaves to each country (# $ @ [ \\ ] ^ _ ` { | } ~). They match the national
  variants table in Wikipedia's ISO/IEC 646 article.
- DEC Special Graphics: the VT100's line-drawing set (VT100 User Guide, EK-VT100-UG, table 3-9), as Wikipedia's
  DEC Special Graphics article shows it: ASCII, with 0x5F to 0x7E as the characters below, 0x5F being a blank.
- ASCII with the top bit set: ASCII with 128 added to every byte, as Prime computers and some others kept text;
  a line feed is 0x8A and a carriage return 0x8D. A byte below 128 has no character and is kept as itself.
"""
import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("gen_charsets", os.path.join(HERE, "gen_charsets.py"))
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

# Table, id, name, scope. Each set's own table says which country.
ISO_646 = [
    ("x-iso-646-gb", "iso-646-gb", "ISO 646 UK (BS 4730)", "The pound sign at 0x23 and an overline at 0x7E."),
    ("x-iso-646-de", "iso-646-de", "ISO 646 German (DIN 66003)", "§ at 0x40, Ä Ö Ü and ä ö ü ß in place of [ \\ ] { | } ~."),
    ("x-iso-646-fr", "iso-646-fr", "ISO 646 French (NF Z 62-010, 1982)", "£ à ° ç § µ é ù è ¨ in place of # @ [ \\ ] ` { | } ~."),
    ("x-iso-646-fr1", "iso-646-fr1", "ISO 646 French (NF Z 62-010, 1973)", "As the 1982 version, but with ` kept at 0x60."),
    ("x-iso-646-it", "iso-646-it", "ISO 646 Italian (UNI 0204-70)", "£ § ° ç é ù à ò è ì in place of # @ [ \\ ] ` { | } ~."),
    ("x-iso-646-es", "iso-646-es", "ISO 646 Spanish", "£ § ¡ Ñ ¿ ° ñ ç in place of # @ [ \\ ] { | }."),
    ("x-iso-646-es2", "iso-646-es2", "ISO 646 Spanish (second version)", "· ¡ Ñ Ç ¿ ´ ñ ç ¨ in place of @ [ \\ ] ^ { | } ~."),
    ("x-iso-646-pt", "iso-646-pt", "ISO 646 Portuguese", "§ Ã Ç Õ ã ç õ ° in place of @ [ \\ ] { | } ~."),
    ("x-iso-646-pt2", "iso-646-pt2", "ISO 646 Portuguese (second version)", "´ Ã Ç Õ ã ç õ in place of @ [ \\ ] { | }."),
    ("x-iso-646-se", "iso-646-se", "ISO 646 Swedish and Finnish (SEN 850200 B)", "¤ Ä Ö Å ä ö å and an overline in place of $ [ \\ ] { | } ~."),
    ("x-iso-646-se2", "iso-646-se2", "ISO 646 Swedish, for names (SEN 850200 C)", "As the Swedish set, with É Ü é ü at 0x40, 0x5E, 0x60 and 0x7E."),
    ("x-iso-646-no", "iso-646-no", "ISO 646 Norwegian (NS 4551-1)", "Æ Ø Å æ ø å and an overline in place of [ \\ ] { | } ~."),
    ("x-iso-646-no2", "iso-646-no2", "ISO 646 Norwegian (NS 4551-2)", "§ Æ Ø Å æ ø å | in place of # [ \\ ] { | } ~."),
    ("x-iso-646-dk", "iso-646-dk", "ISO 646 Danish (DS 2089)", "¤ Æ Ø Å Ü æ ø å ü in place of $ [ \\ ] ^ { | } ~."),
    ("x-iso-646-ch", "iso-646-ch", "ISO 646 Swiss", "French and German letters in place of # @ [ \\ ] ^ _ ` { | } ~."),
    ("x-iso-646-hu", "iso-646-hu", "ISO 646 Hungarian", "¤ Á É Ö Ü á é ö ü ˝ in place of $ @ [ \\ ] ` { | } ~."),
    ("x-iso-646-ie", "iso-646-ie", "ISO 646 Irish", "£ and the accented vowels in place of # @ [ \\ ] ^ ` { | } ~."),
    ("x-iso-646-is", "iso-646-is", "ISO 646 Icelandic", "¤ Ð Þ ´ Æ Ö ð þ ´ æ ö in place of $ @ [ \\ ] ^ ` { | } ~."),
]

DEC_GRAPHICS = {
    0x5F: 0x0020, 0x60: 0x25C6, 0x61: 0x2592, 0x62: 0x2409, 0x63: 0x240C, 0x64: 0x240D, 0x65: 0x240A, 0x66: 0x00B0,
    0x67: 0x00B1, 0x68: 0x2424, 0x69: 0x240B, 0x6A: 0x2518, 0x6B: 0x2510, 0x6C: 0x250C, 0x6D: 0x2514, 0x6E: 0x253C,
    0x6F: 0x23BA, 0x70: 0x23BB, 0x71: 0x2500, 0x72: 0x23BC, 0x73: 0x23BD, 0x74: 0x251C, 0x75: 0x2524, 0x76: 0x2534,
    0x77: 0x252C, 0x78: 0x2502, 0x79: 0x2264, 0x7A: 0x2265, 0x7B: 0x03C0, 0x7C: 0x2260, 0x7D: 0x00A3, 0x7E: 0x00B7,
}
DEC_SOURCE = "DEC VT100 User Guide (EK-VT100-UG), table 3-9, as Wikipedia's DEC Special Graphics article shows it"
DEC_TECHNICAL = {
    0x21: 0x23B7, 0x22: 0x250C, 0x23: 0x2500, 0x24: 0x2320, 0x25: 0x2321, 0x26: 0x2502, 0x27: 0x23A1, 0x28: 0x23A3,
    0x29: 0x23A4, 0x2A: 0x23A6, 0x2B: 0x239B, 0x2C: 0x239D, 0x2D: 0x239E, 0x2E: 0x23A0, 0x2F: 0x23A8, 0x30: 0x23AC,
    0x31: 0x23B2, 0x32: 0x23B3, 0x33: 0x2572, 0x34: 0x2571, 0x35: 0x231D, 0x36: 0x231F, 0x37: 0x27E9, 0x3C: 0x2264,
    0x3D: 0x2260, 0x3E: 0x2265, 0x3F: 0x222B, 0x40: 0x2234, 0x41: 0x221D, 0x42: 0x221E, 0x43: 0x00F7, 0x44: 0x0394,
    0x45: 0x2207, 0x46: 0x03A6, 0x47: 0x0393, 0x48: 0x223C, 0x49: 0x2243, 0x4A: 0x0398, 0x4B: 0x00D7, 0x4C: 0x039B,
    0x4D: 0x21D4, 0x4E: 0x21D2, 0x4F: 0x2261, 0x50: 0x03A0, 0x51: 0x03A8, 0x53: 0x03A3, 0x56: 0x221A, 0x57: 0x03A9,
    0x58: 0x039E, 0x59: 0x03A5, 0x5A: 0x2282, 0x5B: 0x2283, 0x5C: 0x2229, 0x5D: 0x222A, 0x5E: 0x2227, 0x5F: 0x2228,
    0x60: 0x00AC, 0x61: 0x03B1, 0x62: 0x03B2, 0x63: 0x03C7, 0x64: 0x03B4, 0x65: 0x03B5, 0x66: 0x03C6, 0x67: 0x03B3,
    0x68: 0x03B7, 0x69: 0x03B9, 0x6A: 0x03B8, 0x6B: 0x03BA, 0x6C: 0x03BB, 0x6E: 0x03BD, 0x6F: 0x2202, 0x70: 0x03C0,
    0x71: 0x03C8, 0x72: 0x03C1, 0x73: 0x03C3, 0x74: 0x03C4, 0x76: 0x0192, 0x77: 0x03C9, 0x78: 0x03BE, 0x79: 0x03C5,
    0x7A: 0x03B6, 0x7B: 0x2190, 0x7C: 0x2191, 0x7D: 0x2192, 0x7E: 0x2193,
}
# Bytes with nothing in the set: reserved by DEC, shown blank on a VT220.
DEC_TECHNICAL_UNDEFINED = [0x38, 0x39, 0x3A, 0x3B, 0x52, 0x54, 0x55, 0x6D, 0x75]
DEC_TECHNICAL_SOURCE = "DEC VT220 Programmer Reference Manual (EK-VT220-RM), the Technical Character Set, with Unicode as Kermit's dectech.txt and Unicode L2/00-159 map it"
DEC_TECHNICAL_SCOPE = "7-bit. Bracket and integral pieces for building large symbols, maths signs, arrows and the Greek alphabet, as a VT220 shows them after ESC ( >. Bytes DEC left empty have no character."


def ascii_rows():
    return {byte: (byte, "c" if gen.is_control(byte) else "p", "") for byte in range(0x80)}


def dec_graphics():
    rows = ascii_rows()
    for byte, cp in DEC_GRAPHICS.items():
        rows[byte] = (cp, "p", "")
    return rows, {}


def dec_technical():
    rows = ascii_rows()
    for byte in range(0x21, 0x7F):
        del rows[byte]
    for byte, cp in DEC_TECHNICAL.items():
        rows[byte] = (cp, "p", "")
    return rows, {}


def high_bit():
    return {byte | 0x80: row for byte, row in ascii_rows().items()}, {}


def main():
    folder, output = sys.argv[1:3]
    out = open(output, "w", encoding="utf-8", newline="\n")
    out.write("# Generated by tools/gen_national.py, in the same format as charsets.txt.\n")
    out.write("# Row: byte, Unicode (or -), kind (p printable, c control, s depends on display state), name. A byte with no row is undefined.\n\n")
    for table, cid, name, scope in ISO_646:
        rows, back = gen.read_kr(folder, table)
        assert len(rows) == 128, table
        gen.write_charset(out, cid, name, "ISO 646 (7-bit national)", gen.KR_SOURCE, gen.KR_VERSION, "7-bit. " + scope, [("Text", (rows, back))])
    gen.write_charset(out, "dec-special-graphics", "DEC Special Graphics (VT100 line drawing)", "ISO 646 (7-bit national)", DEC_SOURCE, "VT100 (1978)",
                      "7-bit. ASCII, with line drawing, scan lines, control pictures and a few symbols at 0x5F to 0x7E, as a VT100 shows them after ESC ( 0.",
                      [("Text", dec_graphics())])
    gen.write_charset(out, "dec-technical", "DEC Technical (VT220 maths and Greek)", "ISO 646 (7-bit national)", DEC_TECHNICAL_SOURCE, "VT220 (1983)",
                      DEC_TECHNICAL_SCOPE, [("Text", dec_technical())])
    # Written out here, for the line ends' own bytes, which write_charset has no place for.
    out.write("@charset ascii-high-bit\nname: ASCII with the top bit set (Prime)\ngroup: ISO 646 (7-bit national)\n"
              "source: ASCII (ANSI X3.4) with 128 added to each byte\nversion: ANSI X3.4-1986\n"
              "scope: ASCII in bytes 128 to 255, as Prime computers kept text, with 0x8D and 0x8A as carriage return and line feed. A byte below 128 is shown as itself.\n"
              "model: none\nkeep_bytes: false\ncr_lf: 8D 8A\n")
    gen.write_table(out, "Text", high_bit())
    out.write("\n")
    out.close()
    print("%d sets" % (len(ISO_646) + 3))


if __name__ == "__main__":
    main()
