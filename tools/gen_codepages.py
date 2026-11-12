#!/usr/bin/env python3
"""Builds data/charsets-more.txt: more single-byte code pages, in the same format as charsets.txt.

Usage: python3 -I tools/gen_codepages.py <folder of Unicode Consortium vendor MAPPINGS files> <output>

Sources, none invented here, each named in the output:
- Python's codec tables, as for the code pages in charsets.txt, where Python has the page.
- The Unicode Consortium's MAPPINGS/VENDORS files for Apple's and NeXT's sets Python hasn't got
  (www.unicode.org/Public/MAPPINGS/VENDORS/APPLE and NEXT).
- The GNU C Library's iconv tables (through the iconv program) for the rest.
Where a second of these has the same page, every byte is compared, and the differences are listed in the output
with the first source's reading used. A byte a table doesn't define stays undefined.
"""
import codecs
import os
import re
import subprocess
import sys

# (id, name, group, primary source, cross-check) where a source is ("python", codec), ("unicode", file) or ("glibc", name).
PAGES = [
    ("cp720", "CP720 (DOS Arabic)", "DOS code pages", ("python", "cp720"), None),
    ("cp856", "CP856 (DOS Hebrew, IBM)", "DOS code pages", ("python", "cp856"), ("glibc", "IBM856")),
    ("cp1125", "CP1125 (DOS Ukrainian)", "DOS code pages", ("python", "cp1125"), ("glibc", "CP1125")),
    ("cp1006", "CP1006 (IBM Urdu)", "ISO-8859 and others", ("python", "cp1006"), None),
    ("tis-620", "TIS-620 (Thai)", "ISO-8859 and others", ("python", "tis_620"), ("glibc", "TIS-620")),
    ("viscii", "VISCII (Vietnamese)", "ISO-8859 and others", ("glibc", "VISCII"), None),
    ("koi8-t", "KOI8-T (Tajik)", "ISO-8859 and others", ("python", "koi8_t"), ("glibc", "KOI8-T")),
    ("koi8-ru", "KOI8-RU (Russian, Ukrainian, Belarusian)", "ISO-8859 and others", ("glibc", "KOI8-RU"), None),
    ("kz-1048", "KZ-1048 (Kazakh)", "ISO-8859 and others", ("python", "kz1048"), ("glibc", "RK1048")),
    ("pt154", "PT154 (Cyrillic Asian)", "ISO-8859 and others", ("python", "ptcp154"), ("glibc", "PT154")),
    ("armscii-8", "ArmSCII-8 (Armenian)", "ISO-8859 and others", ("glibc", "ARMSCII-8"), None),
    ("georgian-ps", "Georgian-PS", "ISO-8859 and others", ("glibc", "GEORGIAN-PS"), None),
    ("georgian-academy", "Georgian-Academy", "ISO-8859 and others", ("glibc", "GEORGIAN-ACADEMY"), None),
    ("mac-arabic", "Macintosh Arabic", "Macintosh", ("python", "mac_arabic"), None),
    ("mac-farsi", "Macintosh Farsi", "Macintosh", ("python", "mac_farsi"), None),
    ("mac-hebrew", "Macintosh Hebrew", "Macintosh", ("unicode", "HEBREW"), None),
    ("mac-thai", "Macintosh Thai", "Macintosh", ("unicode", "THAI"), None),
    ("mac-croatian", "Macintosh Croatian", "Macintosh", ("python", "mac_croatian"), None),
    ("mac-romanian", "Macintosh Romanian", "Macintosh", ("python", "mac_romanian"), None),
    # Apple's UKRAINE.TXT says Mac OS 9 merged this set into Mac OS Cyrillic, which charsets.txt has; this is the earlier one.
    ("mac-ukrainian", "Macintosh Ukrainian (before Mac OS 9)", "Macintosh", ("glibc", "MAC-UK"), None),
    ("mac-celtic", "Macintosh Celtic", "Macintosh", ("unicode", "CELTIC"), None),
    ("mac-gaelic", "Macintosh Gaelic", "Macintosh", ("unicode", "GAELIC"), None),
    ("mac-inuit", "Macintosh Inuit", "Macintosh", ("unicode", "INUIT"), None),
    ("mac-symbol", "Macintosh Symbol", "Macintosh", ("unicode", "SYMBOL"), None),
    ("mac-dingbats", "Macintosh Dingbats", "Macintosh", ("unicode", "DINGBATS"), None),
    ("hp-roman8", "HP Roman-8", "Workstations and handhelds", ("python", "hp_roman8"), ("glibc", "HP-ROMAN8")),
    ("dec-mcs", "DEC Multinational", "Workstations and handhelds", ("glibc", "DEC-MCS"), None),
    ("nextstep", "NeXTSTEP", "Workstations and handhelds", ("unicode", "NEXTSTEP"), None),
    ("palm-os", "Palm OS", "Workstations and handhelds", ("python", "palmos"), None),
]

ROW = re.compile(r"^0x([0-9A-Fa-f]{2})\s+((?:<[A-Z]+>\+)?0x[0-9A-Fa-f]{4,6}(?:\+0x[0-9A-Fa-f]{4,6})*)\s*(?:#\s*(.*))?$")


def is_control(cp):
    return cp < 0x20 or 0x7F <= cp <= 0x9F


def from_python(name):
    rows = {}
    for byte in range(256):
        try:
            rows[byte] = [ord(c) for c in bytes([byte]).decode(name)]
        except UnicodeDecodeError:
            pass
    return rows, {}


def from_unicode(folder, name):
    """A vendor table: one or more code points a byte, with Apple's direction hints (<LR>, <RL>) taken off."""
    rows, names = {}, {}
    for line in open(os.path.join(folder, name + ".TXT"), encoding="latin-1"):
        match = ROW.match(line.strip())
        if match:
            byte = int(match.group(1), 16)
            points = re.sub(r"<[A-Z]+>\+", "", match.group(2)).split("+")
            rows[byte] = [int(point, 16) for point in points]
            names[byte] = (match.group(3) or "").replace("\t", " ").strip()
    # What the files say in their headers: Apple's sets "use the standard control characters at 0x00-0x1F and
    # 0x7F", and NeXTSTEP's "128 characters (0x0 - 0x7f) are identical to ASCII".
    implied = range(0x80) if name == "NEXTSTEP" else [*range(0x20), 0x7F]
    for byte in implied:
        rows.setdefault(byte, [byte])
    return rows, names


def from_glibc(name):
    rows = {}
    for byte in range(256):
        result = subprocess.run(["iconv", "-f", name, "-t", "UTF-32BE"], input=bytes([byte]), capture_output=True)
        if result.returncode == 0 and result.stdout and len(result.stdout) % 4 == 0:
            rows[byte] = [int.from_bytes(result.stdout[i:i + 4], "big") for i in range(0, len(result.stdout), 4)]
    return rows, {}


def read(source, folder):
    kind, name = source
    if kind == "python":
        codecs.lookup(name)
        return from_python(name)
    if kind == "unicode":
        return from_unicode(folder, name)
    return from_glibc(name)


def describe(source, glibc):
    kind, name = source
    if kind == "python":
        return "Python %d.%d codec table %s" % (sys.version_info[0], sys.version_info[1], name), "Python %d.%d.%d" % sys.version_info[:3]
    if kind == "unicode":
        version = "unicode.org MAPPINGS/VENDORS %s.TXT" % name
        return "Unicode Consortium vendor mapping table %s.TXT" % name, version
    return "GNU C Library iconv table %s" % name, glibc


def show(points):
    return "none" if points is None else "+".join("U+%04X" % p for p in points)


def main():
    folder, output = sys.argv[1:3]
    glibc = subprocess.run(["iconv", "--version"], capture_output=True, text=True).stdout.splitlines()[0]
    differences = []
    pages = []
    for cid, name, group, primary, check in PAGES:
        rows, names = read(primary, folder)
        if check:
            other, _ = read(check, folder)
            if not other:
                sys.exit("%s: the cross-check %s has no table" % (cid, check[1]))
            for byte in range(256):
                if rows.get(byte) != other.get(byte):
                    differences.append("#   %s byte %02X: %s %s, %s %s" % (cid, byte, primary[1], show(rows.get(byte)), check[1], show(other.get(byte))))
        pages.append((cid, name, group, primary, check, rows, names))
    out = open(output, "w", encoding="utf-8", newline="\n")
    out.write("# Generated by tools/gen_codepages.py, in the same format as charsets.txt.\n")
    out.write("# Row: byte, Unicode (or -), kind (p printable, c control, s depends on display state), name. A byte with no row is undefined.\n")
    out.write("# Where a second source has the page, every byte was compared; these differ, and the first source is used:\n")
    for line in differences or ["#   none"]:
        out.write(line + "\n")
    out.write("\n")
    for cid, name, group, primary, check, rows, names in pages:
        source, version = describe(primary, glibc)
        if check:
            source += ", compared byte for byte with " + describe(check, glibc)[0]
        out.write("@charset %s\nname: %s\ngroup: %s\nsource: %s\nversion: %s\n" % (cid, name, group, source, version))
        out.write("scope: Complete single-byte code page.\nmodel: none\nkeep_bytes: false\n@table Text\n")
        for byte in range(256):
            points = rows.get(byte)
            if points is None:
                continue
            label = names.get(byte, "")
            if len(points) != 1:
                # A byte standing for a sequence of characters: named, but with no single character to show.
                out.write("%02X\t-\ts\t%s\n" % (byte, label or "sequence " + show(points)))
                continue
            cp = points[0]
            out.write("%02X\t%04X\t%s\t\n" % (byte, cp, "c" if is_control(cp) else "p"))
        out.write("\n")
    out.close()
    print("%d pages, %d differences between sources noted" % (len(pages), len(differences)))


if __name__ == "__main__":
    main()
