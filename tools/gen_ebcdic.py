#!/usr/bin/env python3
"""Builds data/charsets-ebcdic.txt, the IBM mainframe (EBCDIC) code pages, in the same format as charsets.txt.

Usage: python3 -I tools/gen_ebcdic.py <folder of ICU .ucm files> <output>

Source: IBM's own code page tables, as ICU publishes them (github.com/unicode-org/icu-data, charset/data/ucm,
under the Unicode License V3). Only their round-trip mappings (|0) are used, so a byte reads back as the
character it was written as. A byte IBM leaves unassigned stays undefined.

Each page is also compared byte for byte with the GNU C Library's iconv table and, where it has one, Python's
codec. They don't always agree with IBM (glibc's 875 has gaps and two different bytes; Python's 273 reads
0xBC as U+203E; Python fills unassigned bytes with SUB). Every difference is listed in the output file, and
IBM's table is what's used. Nothing is filled in by hand except the short names of the controls, which are
IBM's names for them.
"""
import os
import re
import subprocess
import sys

ICU_COMMIT = "8d9eb3e27e79f59dd76e278e58d68b4668835027"

PAGES = [
    ("ebcdic-037", "ibm-37_P100-1999", "IBM037", "cp037", "EBCDIC 037 (US, Canada)"),
    ("ebcdic-1140", "ibm-1140_P100-1997", "IBM1140", "cp1140", "EBCDIC 1140 (US, Canada, with euro)"),
    ("ebcdic-1047", "ibm-1047_P100-1995", "IBM1047", None, "EBCDIC 1047 (Latin-1, z/OS Unix)"),
    ("ebcdic-500", "ibm-500_P100-1999", "IBM500", "cp500", "EBCDIC 500 (International)"),
    ("ebcdic-1148", "ibm-1148_P100-1997", "IBM1148", None, "EBCDIC 1148 (International, with euro)"),
    ("ebcdic-273", "ibm-273_P100-1999", "IBM273", "cp273", "EBCDIC 273 (Germany, Austria)"),
    ("ebcdic-1141", "ibm-1141_P100-1997", "IBM1141", None, "EBCDIC 1141 (Germany, Austria, with euro)"),
    ("ebcdic-277", "ibm-277_P100-1999", "IBM277", None, "EBCDIC 277 (Denmark, Norway)"),
    ("ebcdic-1142", "ibm-1142_P100-1997", "IBM1142", None, "EBCDIC 1142 (Denmark, Norway, with euro)"),
    ("ebcdic-278", "ibm-278_P100-1999", "IBM278", None, "EBCDIC 278 (Finland, Sweden)"),
    ("ebcdic-1143", "ibm-1143_P100-1997", "IBM1143", None, "EBCDIC 1143 (Finland, Sweden, with euro)"),
    ("ebcdic-280", "ibm-280_P100-1999", "IBM280", None, "EBCDIC 280 (Italy)"),
    ("ebcdic-1144", "ibm-1144_P100-1997", "IBM1144", None, "EBCDIC 1144 (Italy, with euro)"),
    ("ebcdic-284", "ibm-284_P100-1999", "IBM284", None, "EBCDIC 284 (Spain, Latin America)"),
    ("ebcdic-1145", "ibm-1145_P100-1997", "IBM1145", None, "EBCDIC 1145 (Spain, Latin America, with euro)"),
    ("ebcdic-285", "ibm-285_P100-1999", "IBM285", None, "EBCDIC 285 (United Kingdom)"),
    ("ebcdic-1146", "ibm-1146_P100-1997", "IBM1146", None, "EBCDIC 1146 (United Kingdom, with euro)"),
    ("ebcdic-297", "ibm-297_P100-1999", "IBM297", None, "EBCDIC 297 (France)"),
    ("ebcdic-1147", "ibm-1147_P100-1997", "IBM1147", None, "EBCDIC 1147 (France, with euro)"),
    ("ebcdic-871", "ibm-871_P100-1999", "IBM871", None, "EBCDIC 871 (Iceland)"),
    ("ebcdic-1149", "ibm-1149_P100-1997", "IBM1149", None, "EBCDIC 1149 (Iceland, with euro)"),
    ("ebcdic-875", "ibm-875_P100-1995", "IBM875", "cp875", "EBCDIC 875 (Greek)"),
]

# IBM's short names for the EBCDIC controls, by byte.
NAMES = {
    0x00: "NUL", 0x01: "SOH", 0x02: "STX", 0x03: "ETX", 0x04: "SEL", 0x05: "HT", 0x06: "RNL", 0x07: "DEL",
    0x08: "GE", 0x09: "SPS", 0x0A: "RPT", 0x0B: "VT", 0x0C: "FF", 0x0D: "CR", 0x0E: "SO", 0x0F: "SI",
    0x10: "DLE", 0x11: "DC1", 0x12: "DC2", 0x13: "DC3", 0x14: "RES", 0x15: "NL", 0x16: "BS", 0x17: "POC",
    0x18: "CAN", 0x19: "EM", 0x1A: "UBS", 0x1B: "CU1", 0x1C: "IFS", 0x1D: "IGS", 0x1E: "IRS", 0x1F: "IUS",
    0x20: "DS", 0x21: "SOS", 0x22: "FS", 0x23: "WUS", 0x24: "BYP", 0x25: "LF", 0x26: "ETB", 0x27: "ESC",
    0x28: "SA", 0x29: "SFE", 0x2A: "SM", 0x2B: "CSP", 0x2C: "MFA", 0x2D: "ENQ", 0x2E: "ACK", 0x2F: "BEL",
    0x30: "DS0", 0x31: "DS1", 0x32: "SYN", 0x33: "IR", 0x34: "PP", 0x35: "TRN", 0x36: "NBS", 0x37: "EOT",
    0x38: "SBS", 0x39: "IT", 0x3A: "RFF", 0x3B: "CU3", 0x3C: "DC4", 0x3D: "NAK", 0x3E: "DS3", 0x3F: "SUB",
    0xFF: "EO",
}

ROW = re.compile(r"^<U([0-9A-Fa-f]{4,6})>\s+\\x([0-9A-Fa-f]{2})\s+\|0")


def is_control(cp):
    return cp < 0x20 or 0x7F <= cp <= 0x9F


def read_ucm(path):
    rows = {}
    for line in open(path, encoding="utf-8"):
        match = ROW.match(line.strip())
        if match:
            byte = int(match.group(2), 16)
            if byte in rows:
                sys.exit("%s maps byte %02X twice" % (path, byte))
            rows[byte] = int(match.group(1), 16)
    return rows


def iconv_byte(page, byte):
    result = subprocess.run(["iconv", "-f", page, "-t", "UTF-32BE"], input=bytes([byte]), capture_output=True)
    return int.from_bytes(result.stdout, "big") if result.returncode == 0 and len(result.stdout) == 4 else None


def python_byte(name, byte):
    try:
        return ord(bytes([byte]).decode(name))
    except UnicodeDecodeError:
        return None


def show(cp):
    return "none" if cp is None else "U+%04X" % cp


def main():
    folder, output = sys.argv[1:3]
    glibc = subprocess.run(["iconv", "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    out = open(output, "w", encoding="utf-8", newline="\n")
    out.write("# Generated by tools/gen_ebcdic.py from IBM's tables in ICU (unicode-org/icu-data commit %s, Unicode License V3).\n" % ICU_COMMIT)
    out.write("# Row: byte, Unicode, kind (p printable, c control), name. A byte with no row is unassigned.\n")
    out.write("# Compared with %s and Python %d.%d; where they differ from IBM, IBM is used:\n" % ((glibc,) + sys.version_info[:2]))
    differences = []
    pages = []
    for cid, ucm, glibc_name, python_name, name in PAGES:
        ibm = read_ucm(os.path.join(folder, ucm + ".ucm"))
        for byte in range(256):
            for source, theirs in [("glibc " + glibc_name, iconv_byte(glibc_name, byte)), ("Python " + (python_name or ""), python_byte(python_name, byte) if python_name else ibm.get(byte))]:
                # Python fills IBM's unassigned bytes with SUB; that isn't a disagreement about any character.
                if theirs != ibm.get(byte) and not (theirs == 0x1A and byte not in ibm and source.startswith("Python")):
                    differences.append("#   %s byte %02X: IBM %s, %s %s" % (cid, byte, show(ibm.get(byte)), source, show(theirs)))
        pages.append((cid, ucm, name, ibm))
    for line in differences or ["#   none"]:
        out.write(line + "\n")
    out.write("\n")
    for cid, ucm, name, ibm in pages:
        out.write("@charset %s\nname: %s\ngroup: EBCDIC (IBM mainframe)\n" % (cid, name))
        out.write("source: IBM code page table %s, from ICU's charset data, under the Unicode License V3\n" % ucm)
        out.write("version: unicode-org/icu-data commit %s\n" % ICU_COMMIT[:12])
        out.write("scope: Complete single-byte code page. Lines end with NL (0x15), LF (0x25), CR (0x0D) or CR LF, or the file is fixed-length records.\n")
        out.write("model: none\nkeep_bytes: false\nebcdic: true\n@table Text\n")
        for byte in range(256):
            if byte in ibm:
                cp = ibm[byte]
                control = is_control(cp)
                out.write("%02X\t%04X\t%s\t%s\n" % (byte, cp, "c" if control else "p", NAMES.get(byte, "") if control else ""))
        out.write("\n")
    out.close()
    print("%d pages, %d differences from glibc or Python noted" % (len(pages), len(differences)))


if __name__ == "__main__":
    main()
