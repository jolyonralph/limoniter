"""Puts together THIRD-PARTY-NOTICES.txt, which the installer carries: the licence of everything built into the program.

Most of these licences require their text to go with every copy, so it's made fresh from what's actually being built.
    python -I tools/gen_notices.py <the app folder> <the file to write> [<the platform, as Rust names it>]
The platform is Windows (x86_64-pc-windows-msvc) unless another is named, as the Linux packages name theirs.
It says on the console if a package has no licence text of its own to quote.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

TARGET = sys.argv[3] if len(sys.argv) > 3 else 'x86_64-pc-windows-msvc'
# What a package's licence files are called.
LICENCE_NAME = re.compile(r'^(licen[cs]e|copying|notice|unlicense|copyright|ofl|ufl)', re.IGNORECASE)
LICENCE_FOLDERS = ('', 'fonts', 'licenses', 'licences', 'license', 'LICENSES')
RULE = '=' * 78


def run(folder, *command):
    return subprocess.run(command, cwd=folder, check=True, capture_output=True, encoding='utf-8').stdout


def packages_built_in(folder):
    """Name and version of everything linked into the program, leaving out build-time tools."""
    tree = run(folder, 'cargo', 'tree', '--locked', '-e', 'normal', '--target', TARGET, '--prefix', 'none', '-f', '{p}')
    found = set()
    for line in tree.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0] != 'limoniter':
            found.add((parts[0], parts[1].lstrip('v')))
    return found


def licence_texts(manifest):
    root = os.path.dirname(manifest)
    texts = []
    for sub in LICENCE_FOLDERS:
        place = os.path.join(root, sub)
        if not os.path.isdir(place):
            continue
        for name in sorted(os.listdir(place)):
            path = os.path.join(place, name)
            if os.path.isfile(path) and LICENCE_NAME.match(name):
                with open(path, encoding='utf-8', errors='replace') as file:
                    texts.append(file.read().strip())
    return texts


def key_of(text):
    """Texts differing only in how they're spaced count as the same."""
    return hashlib.sha256(' '.join(text.split()).encode()).hexdigest()


def main():
    folder, target = sys.argv[1], sys.argv[2]
    wanted = packages_built_in(folder)
    metadata = json.loads(run(folder, 'cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', TARGET))
    details = {(p['name'], p['version']): p for p in metadata['packages']}

    by_text = {}
    first_seen = {}
    listing = []
    without = []
    for key in sorted(wanted):
        package = details[key]
        name = f'{key[0]} {key[1]}'
        stated = package.get('license') or 'see its own files'
        listing.append(f'{name}: {stated}  (source: https://crates.io/crates/{key[0]}/{key[1]})')
        texts = licence_texts(package['manifest_path'])
        if not texts:
            without.append(f'{name} ({stated})')
        for text in texts:
            digest = key_of(text)
            first_seen.setdefault(digest, text)
            by_text.setdefault(digest, []).append(name)

    grammars = run(folder, 'cargo', 'run', '--locked', '--quiet', '--release', '--example', 'grammar_credits')
    with open(os.path.join(folder, 'data', 'unifont-license.txt'), encoding='utf-8') as file:
        unifont = file.read().strip()
    with open(os.path.join(folder, 'data', 'icu-data-license.txt'), encoding='utf-8') as file:
        icu = file.read().strip()
    with open(os.path.join(folder, 'data', 'nextstep-mapping-notice.txt'), encoding='utf-8') as file:
        nextstep = file.read().strip()

    out = [
        'Limoniter is built with the work of many other people. Their licences follow.',
        'Limoniter\'s own licence is in the file LICENSE.',
        '',
        RULE,
        'Part 1: the retro character fonts',
        RULE,
        '',
        unifont,
        '',
        RULE,
        'Part 2: the character tables',
        RULE,
        '',
        'The tables of old computer character sets draw on mapping data from the kreativekorp/charsets',
        'project (https://github.com/kreativekorp/charsets), with thanks.',
        '',
        'The IBM mainframe (EBCDIC) code pages are IBM\'s own tables as published in ICU\'s charset data',
        '(https://github.com/unicode-org/icu-data), under this licence:',
        '',
        icu,
        '',
        'The Macintosh Hebrew, Thai, Celtic, Gaelic, Inuit, Symbol and Dingbats tables are Apple\'s mapping files as',
        'the Unicode Consortium publishes them (https://www.unicode.org/Public/MAPPINGS/VENDORS/APPLE/). Each says',
        '"Copyright (c) 1995-2002, 2005 by Apple Computer, Inc., all rights reserved" and states no licence, so the',
        'terms they may be used on are uncertain. They are used here in good faith, as mapping tables of this kind',
        'commonly are (Python\'s own Macintosh codecs come from the same files), with that uncertainty acknowledged.',
        'Apple Computer, Inc. makes no warranty or representation about them, as their headers say.',
        '',
        'The NeXTSTEP table is from the Unicode Consortium\'s NEXTSTEP.TXT, which carries this notice:',
        '',
        nextstep,
        '',
        RULE,
        'Part 3: the grammars used for colouring code',
        RULE,
        '',
        grammars.strip(),
        '',
        RULE,
        'Part 4: every software library built into Limoniter, with where its source can be found',
        RULE,
        '',
        *listing,
        '',
        RULE,
        'Part 5: the text of those libraries\' licences',
        RULE,
    ]
    if without:
        out += ['', 'These libraries don\'t carry their own copy of the licence they state, as listed in Part 4. The standard', 'text of those licences is among the texts below: ' + ', '.join(without) + '.']
    for digest in sorted(by_text, key=lambda each: by_text[each][0]):
        out += ['', RULE, 'This text applies to: ' + ', '.join(by_text[digest]), RULE, '', first_seen[digest]]
    os.makedirs(os.path.dirname(os.path.abspath(target)), exist_ok=True)
    with open(target, 'w', encoding='utf-8', newline='\r\n' if 'windows' in TARGET else '\n') as file:
        file.write('\n'.join(out) + '\n')

    print(f'{len(wanted)} libraries, {len(by_text)} different licence texts')
    if without:
        print('No licence text of their own to quote, so only what they state is listed:')
        for each in without:
            print('  ' + each)


main()
