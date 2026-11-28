#!/usr/bin/env python3
"""Packs the CMU Pronouncing Dictionary and WordNet 3.1 into data/words/*.bin for Limoniter's Words tools.

Usage: gen_words.py CMUDICT.dict WORDNET_DICT_FOLDER OUT_FOLDER

Both inputs are downloaded by hand (see data/words/README-sources.txt); nothing here fetches anything.
Formats (each file is raw-deflated by miniz_oxide's decompress_to_vec):

cmudict.bin: for each word in byte order: prefix length shared with the last word (u8), suffix length (u8), the
suffix bytes, then the first pronunciation as one byte a phoneme, ended by 0. Consonants are 1..=24 (in PHONEMES
order); a vowel is 32 + 3 * its place among the vowels + its stress (0, 1 or 2).

wordnet.bin: UTF-8 text. Sections, each started by a line "#name":
  #synsets: one line a synset, in file order (nouns, verbs, adjectives, adverbs): part letter TAB words joined by
            "|" TAB short definition TAB antonyms joined by "|".  A synset's number is its line's place (from 0).
  #order:   for lemmas whose best-first order of senses differs from file order: lemma TAB synset numbers (base 36,
            spaces between).
  #lemmas:  words (lower case, one word) found only in senses left out, one a line, so they still count as known.
  #exc:     irregular forms: part letter TAB inflected TAB base.
"""
import sys, zlib, os

MAX_DEFINITION = 70
PHONEMES_C = "B CH D DH F G HH JH K L M N NG P R S SH T TH V W Y Z ZH".split()
PHONEMES_V = "AA AE AH AO AW AY EH ER EY IH IY OW OY UH UW".split()


def deflate(data):
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def cmu(src, out):
    best = {}
    for line in open(src, encoding="latin-1"):
        parts = line.split("#")[0].split()
        if len(parts) < 2:
            continue
        w = parts[0]
        if "(" in w:
            continue  # variants: the first pronunciation is the one kept
        if not all(c.isalpha() and c.isascii() or c in "'.-" for c in w):
            continue
        best[w] = parts[1:]
    buf = bytearray()
    last = b""
    for w in sorted(best, key=lambda s: s.encode()):
        b = w.encode()
        p = 0
        while p < len(last) and p < len(b) and last[p] == b[p] and p < 255:
            p += 1
        suffix = b[p:]
        buf += bytes([p, len(suffix)]) + suffix
        for ph in best[w]:
            stress = 0
            if ph[-1].isdigit():
                stress = int(ph[-1]); ph = ph[:-1]
            if ph in PHONEMES_C:
                buf.append(PHONEMES_C.index(ph) + 1)
            else:
                buf.append(32 + 3 * PHONEMES_V.index(ph) + stress)
        buf.append(0)
        last = b
    open(os.path.join(out, "cmudict.bin"), "wb").write(deflate(bytes(buf)))
    print("cmudict words", len(best), "raw", len(buf))


def wordnet(folder, out):
    parts = [("n", "noun"), ("v", "verb"), ("a", "adj"), ("r", "adv")]
    syn = []  # (pos, words, def, ants)
    num = {}  # (pos letter, offset) -> index
    raw = []
    for letter, name in parts:
        for line in open(os.path.join(folder, "data." + name), encoding="utf-8"):
            if line.startswith(" "):
                continue
            head, _, gloss = line.partition(" | ")
            f = head.split()
            offset, ss_type = f[0], f[2]
            nw = int(f[3], 16)
            words = []
            i = 4
            for _ in range(nw):
                words.append(f[i].replace("_", " ")); i += 2
                if words[-1].endswith(")") and "(" in words[-1]:
                    words[-1] = words[-1][: words[-1].rindex("(")]
            npt = int(f[i]); i += 1
            ptrs = []
            for _ in range(npt):
                ptrs.append(f[i:i + 4]); i += 4
            d = gloss.strip()
            for cut in ('; "', ";\""):
                if cut in d:
                    d = d[: d.index(cut)]
            d = d.strip()
            raw.append((letter, ss_type, offset, words, d, ptrs))
            num[(letter if letter != "a" else "a", offset)] = len(raw) - 1
            if ss_type == "s":
                num[("s", offset)] = len(raw) - 1
    # antonyms: lexical pointers "!" from word s to word t of target synset
    def target(sym_pos, off):
        key = (sym_pos, off)
        return num.get(key) if key in num else num.get(("a" if sym_pos == "s" else sym_pos, off))
    ants = []
    for idx, (letter, ss_type, offset, words, d, ptrs) in enumerate(raw):
        a = []
        for p in ptrs:
            if p[0] == "!":
                t = target(p[2], p[1])
                if t is not None:
                    which = int(p[3][2:], 16)
                    if which >= 1 and which <= len(raw[t][3]):
                        a.append(raw[t][3][which - 1])
                    elif which == 0:
                        a.append(raw[t][3][0])
        ants.append(a)
    # satellites borrow the antonyms of the head of their cluster (via "similar to")
    for idx, (letter, ss_type, offset, words, d, ptrs) in enumerate(raw):
        if ss_type == "s" and not ants[idx]:
            for p in ptrs:
                if p[0] == "&":
                    t = target(p[2], p[1])
                    if t is not None and ants[t]:
                        ants[idx] = list(ants[t]); break
    # A sense with nothing to offer (one word, no opposite) is left out, and so are the long tails of definitions.
    keep = {}
    for idx, (letter, ss_type, offset, words, d, ptrs) in enumerate(raw):
        if len(words) > 1 or ants[idx]:
            keep[idx] = len(keep)
    lines = ["#synsets"]
    for idx, (letter, ss_type, offset, words, d, ptrs) in enumerate(raw):
        if idx not in keep:
            continue
        if len(d) > MAX_DEFINITION:
            d = d[:MAX_DEFINITION].rsplit(" ", 1)[0] + "…"
        lines.append("\t".join([ss_type if ss_type == "s" else letter, "|".join(words), d, "|".join(dict.fromkeys(ants[idx]))]))
    # sense order per lemma, from the index files
    natural = {}
    for idx, r in enumerate(raw):
        if idx in keep:
            for w in r[3]:
                natural.setdefault(w.lower(), []).append(keep[idx])
    best = {}
    for letter, name in parts:
        for line in open(os.path.join(folder, "index." + name), encoding="utf-8"):
            if line.startswith(" "):
                continue
            f = line.split()
            lemma = f[0].replace("_", " ")
            n_ptr = int(f[3])
            offs = f[4 + n_ptr + 2:]
            best.setdefault(lemma.lower(), []).extend(keep[num[(letter, o)]] for o in offs if num[(letter, o)] in keep)
    def b36(n):
        s = ""
        while True:
            s = "0123456789abcdefghijklmnopqrstuvwxyz"[n % 36] + s; n //= 36
            if n == 0: return s
    lines.append("#order")
    for lemma, ids in sorted(best.items()):
        nat = natural.get(lemma, [])
        if len(ids) > 1 and sorted(set(ids)) == sorted(set(nat)) and ids != list(dict.fromkeys(nat)):
            lines.append(lemma + "\t" + " ".join(b36(i) for i in dict.fromkeys(ids)))
    # Words that are only in the senses left out still count as words WordNet knows (for ranking rhymes).
    in_kept = {w.lower() for idx in keep for w in raw[idx][3]}
    extra = sorted({w for idx, r in enumerate(raw) if idx not in keep for w in r[3] if w.isalpha() and w.isascii() and w == w.lower() and w not in in_kept})
    lines.append("#lemmas")
    lines.extend(extra)
    lines.append("#exc")
    for letter, name in parts[:3]:
        for line in open(os.path.join(folder, name + ".exc"), encoding="utf-8"):
            f = line.split()
            if len(f) >= 2:
                lines.append(f"{letter}\t{f[0].replace('_', ' ')}\t{f[1].replace('_', ' ')}")
    text = ("\n".join(lines) + "\n").encode()
    open(os.path.join(out, "wordnet.bin"), "wb").write(deflate(text))
    print("wordnet synsets", len(keep), "of", len(raw), "raw", len(text))


if __name__ == "__main__":
    os.makedirs(sys.argv[3], exist_ok=True)
    cmu(sys.argv[1], sys.argv[3])
    wordnet(sys.argv[2], sys.argv[3])
