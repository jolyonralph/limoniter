"""Makes src/media/cpm_formats.rs, Limoniter's table of CP/M disk formats, from cpmtools' diskdefs file.

The numbers that describe each format (sector size, sectors per track, tracks, block size, directory entries, skew
and boot tracks) are taken from Michael Haardt's cpmtools, which collected them from many machines' manuals and
disks. Hard disk partitions, which start at an offset into a larger disk, are left out: Limoniter works out a disk's
format from a whole floppy image. Run it again only when the definitions change:
    python -I tools/gen_cpm_formats.py /etc/cpmtools/diskdefs src/media/cpm_formats.rs
"""
import sys

# Names people know the formats by, where the definition's own name doesn't say.
KNOWN = {
    'ibm-3740': '8-inch IBM 3740, single density',
    'pcw': 'Amstrad PCW, 180K',
    'cpcsys': 'Amstrad CPC, system format',
    'cpcdata': 'Amstrad CPC, data format',
    'kpii': 'Kaypro II',
    'kpiv': 'Kaypro 4 and 10',
    'osborne1': 'Osborne 1, double density',
    'osborne4': 'Osborne Nuevo, Vixen and 4',
    'osb1sssd': 'Osborne 1, single density',
    'epsqx10': 'Epson QX-10',
    'ibm-8ss': '8-inch IBM, single-sided double density',
    'ibm-8ds': '8-inch IBM, double-sided double density',
    'apple-do': 'Apple II CP/M (DOS order)',
    'apple-po': 'Apple II CP/M (ProDOS order)',
    'myz80': 'MYZ80 hard disk',
    'p112': 'P112',
    'cpm86-720': 'CP/M-86 on a 720K floppy',
    'cpm86-144feat': 'CP/M-86 on a 1.44MB floppy',
    'pc1.2m': 'CP/M on a PC 1.2MB floppy',
    'bw12': 'Bondwell 12',
    'bw14': 'Bondwell 14',
    'v1050': 'Visual 1050',
    'microbee40': 'Microbee, 40 tracks',
    'rc759': 'RC759 Piccoline',
    'nsfd': 'North Star floppy',
    'z9001': 'Robotron Z9001',
    'trsomsssd': 'TRS-80 Model I Omikron',
    'dec_pro': 'DEC Professional',
    'icl-comet-525ss': 'ICL Comet',
    'interak': 'Interak',
    'lobo2': 'Lobo Max-80, 5.25-inch',
    'lobo3': 'Lobo Max-80, 8-inch',
    'rm-sd': 'Research Machines 380Z, single density',
    'rm-dd': 'Research Machines 380Z, double density',
    'rm-qd': 'Research Machines 380Z, quad density',
    'nigdos': 'NIGDOS',
    'fdd3000': 'Timex FDD3000',
    'fdd3000_2': 'Timex FDD3000, second format',
    'scp624': 'Robotron SCP, 624K',
    'scp640': 'Robotron SCP, 640K',
    'scp780': 'Robotron SCP, 780K',
    'scp800': 'Robotron SCP, 800K',
    'morsddd': 'Morrow, single-sided double density',
    'mordsdd': 'Morrow, double-sided double density',
    'ampdsdd': 'Ampro, double-sided, 48 tpi',
    'ampdsdd80': 'Ampro, double-sided, 96 tpi',
    'altdsdd': 'Altos, double-sided double density',
    'ibmpc-514ss': 'IBM CP/M-86, 5.25-inch single-sided',
    'ibmpc-514ds': 'IBM CP/M-86, 5.25-inch double-sided',
    'alpha': 'Royal alphatronic',
    'cf2dd': 'Amstrad 3-inch CF2DD',
    'td143ssdd8': 'TurboDOS 1.43, 8-inch',
    'heassdd8': 'Heath H89 with Magnolia CP/M, 8-inch',
    'pmc101': 'PMC-101 MicroMate',
    'electroglas': 'Electroglas',
    'dreamdisk40': 'Dreamdisk, 40 tracks',
    'dreamdisk80': 'Dreamdisk, 80 tracks',
    'attwp': 'AT&T/Olivetti word processor',
    'ampro800': 'Ampro, 800K',
    'p112-old': 'P112, older format',
    'nshd4': 'North Star hard disk, 4MB',
    'z80pack-hd': 'z80pack hard disk',
}

# Tried first, so a disk that reads the same in several formats is named for the machine most people had.
FIRST = ['ibm-3740', 'kpii', 'kpiv', 'osb1sssd', 'osborne1', 'osborne4', 'pcw', 'cpcsys', 'cpcdata', 'epsqx10', 'ibm-8ss',
         'ibm-8ds', 'apple-do', 'apple-po', 'cpm86-720', 'cpm86-144feat', 'ibmpc-514ss', 'ibmpc-514ds']

# Larger than any floppy: hard disks, whose format can't be told from a floppy-sized directory guess.
MOST_BYTES = 4 * 1024 * 1024


def read(path):
    defs, current, comment = [], None, ''
    for raw in open(path, encoding='latin-1'):
        line = raw.strip()
        if line.startswith('#'):
            comment = line.lstrip('# ').strip()
            continue
        if not line:
            comment = ''
            continue
        words = line.split()
        if words[0] == 'diskdef':
            current = {'name': words[1], 'comment': comment}
        elif words[0] == 'end' and current is not None:
            defs.append(current)
            current, comment = None, ''
        elif current is not None and len(words) >= 2:
            current[words[0]] = words[1]
    return defs


def main():
    source, target = sys.argv[1], sys.argv[2]
    rows = []
    defs = read(source)
    defs.sort(key=lambda d: FIRST.index(d['name']) if d['name'] in FIRST else len(FIRST))
    for d in defs:
        if 'offset' in d or int(d['seclen']) * int(d['sectrk']) * int(d['tracks']) >= MOST_BYTES:
            continue
        skew = d.get('skew', '0')
        if 'skewtab' in d:
            skew_rust = 'Skew::Table(&[' + ', '.join(d['skewtab'].split(',')) + '])'
        else:
            skew_rust = f'Skew::Every({int(skew)})'
        os_kind = {'3': 'Os::Cpm3', 'p2dos': 'Os::P2dos', 'zsys': 'Os::P2dos'}.get(d.get('os', '2.2'), 'Os::Cpm22')
        # A comment describes the format unless it's working or a note: then the definition's name says more.
        comment = d['comment'] if d['comment'] and '=' not in d['comment'] and len(d['comment']) < 70 else ''
        name = d['name']
        if name.startswith('memotech-type'):
            comment = 'Memotech FDX, type ' + name[len('memotech-type'):]
        elif name.startswith('amp') and name[3:].isdigit():
            comment = 'Ampro, format ' + name[3:]
        described = KNOWN.get(name) or comment or name
        described = described.replace('\\', '\\\\').replace('"', '\\"')
        rows.append(
            f'    DiskDef {{ name: "{d["name"]}", described: "{described}", sector_size: {d["seclen"]}, tracks: {d["tracks"]}, '
            f'sectors_per_track: {d["sectrk"]}, block_size: {d["blocksize"]}, directory_entries: {d["maxdir"]}, '
            f'skew: {skew_rust}, boot_tracks: {d.get("boottrk", "0")}, os: {os_kind} }},'
        )
    with open(target, 'w', encoding='utf-8', newline='\n') as out:
        out.write('//! CP/M disk formats, made by tools/gen_cpm_formats.py from the definitions Michael Haardt\'s cpmtools\n')
        out.write('//! collected (its diskdefs file). Each says how a machine laid CP/M out on its disks.\n\n')
        out.write('use super::cpm::{DiskDef, Os, Skew};\n\n')
        out.write('#[rustfmt::skip]\n')
        out.write(f'pub const FORMATS: [DiskDef; {len(rows)}] = [\n')
        out.write('\n'.join(rows))
        out.write('\n];\n')
    print(len(rows), 'formats')


main()
