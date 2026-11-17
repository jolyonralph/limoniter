use super::*;

#[test]
fn floats_are_motorola_fast_floating_point() {
    assert_eq!(f64_to_ffp(50.0), 0xC800_0046);
    assert_eq!(ffp_to_f64(0xC800_0046), 50.0);
    for v in [1.0, 0.5, 2.5625, 1234.5678, 1e-3, 6.25e9] {
        assert!((ffp_to_f64(f64_to_ffp(v)) - v).abs() / v < 1e-6, "{v}");
    }
    assert_eq!(float_text(2.75), "2.75");
    assert_eq!(float_text(2.0), "2.0");
}

#[test]
fn a_listing_tokenises_and_lists_back() {
    let text = [
        "Rem A test of AMOS",
        "Screen Open 0,320,256,32,Lowres",
        "Curs Off : Flash Off : Cls 0",
        "For I=1 To 10",
        "   Print \"Hello \";I;\" \";X#;A$",
        "Next I",
        "If A>3 Then Goto AGAIN",
        "AGAIN:",
        "Ink 3 : Bar 10,10 To 100,$40",
        "Proc SKETCH[5,%101]",
        "Procedure SKETCH[N,M]",
        "   Plot N,M",
        "End Proc",
        "Wait Key",
    ];
    let program = tokenise(&text).unwrap();
    assert!(program.starts_with(b"AMOS Basic v134 "));
    let listed = list(&program);
    assert!(listed.notes.iter().all(|n| !n.contains("Extension")), "{:?}", listed.notes);
    let trimmed: Vec<String> = listed.lines.iter().map(|l| l.trim_end().to_string()).collect();
    assert_eq!(trimmed, text.map(String::from), "{:#?}", listed.lines);
    // It comes back the same once more.
    assert_eq!(tokenise(&listed.lines).unwrap(), program);
    // The Procedure line knows how far its End Proc is.
    let code = &program[20..];
    let mut at = 0;
    while word(code, at + 2) != TK_PROC {
        at += usize::from(code[at]) * 2;
    }
    // It keeps how far the line after its End Proc is, less 14.
    let after = at + long(code, at + 4) as usize + 14;
    let mut line = at;
    while word(code, line + 2) != TK_END_PROC {
        line += usize::from(code[line]) * 2;
    }
    assert_eq!(after, line + usize::from(code[line]) * 2);
}

/// Which bytes of a program AMOS fills in itself when it tests it: the places of variables and labels, the
/// jumps of loops and IFs, and a procedure's encryption seed and flags. They're not compared.
fn filled_in(code: &[u8], tested: bool) -> Vec<bool> {
    let mut mask = vec![false; code.len()];
    let mut at = 0;
    while at + 2 <= code.len() {
        let length = usize::from(code[at]) * 2;
        if length == 0 {
            break;
        }
        let end = (at + length).min(code.len());
        // An empty line's indent, which AMOS keeps as 0 or 1.
        if word(code, at + 2) == 0 {
            mask[at + 1] = true;
        }
        let mut p = at + 2;
        while p + 2 <= end {
            let token = word(code, p);
            if token == 0 {
                break;
            }
            p += 2;
            if token <= TK_LGO {
                mask[p] = true;
                mask[p + 1] = true;
                // A name AMOS hasn't yet worked out is a variable; once tested, it may be a procedure.
                if matches!(token, TK_VAR | TK_PRO) {
                    mask[p - 1] = true;
                    mask[p + 3] = code[p + 3] & 0x7F == 0;
                }
                let n = usize::from(code[p + 2]);
                p += if n & 1 == 1 { 5 } else { 4 } + n;
            } else if token < TK_EXT || token == TK_DFL {
                p += match token {
                    TK_CH1 | TK_CH2 => {
                        let n = usize::from(word(code, p));
                        2 + n + (n & 1)
                    }
                    TK_DFL => 8,
                    _ => 4,
                };
            } else {
                if token == TK_EXT {
                    // The extension's unused byte.
                    mask[p + 1] = true;
                    p += 4;
                }
                match token {
                    TK_REM1 | TK_REM2 => {
                        mask[p] = true;
                        let n = usize::from(code[p + 1]);
                        p += 2 + n + (n & 1);
                    }
                    TK_PROC => {
                        // An untested program's distances can be from before it was last changed.
                        for m in &mut mask[if tested { p + 4 } else { p }..p + 8] {
                            *m = true;
                        }
                        p += 8;
                    }
                    t => {
                        for m in &mut mask[p..p + extra(t)] {
                            *m = true;
                        }
                        p += extra(t);
                    }
                }
            }
        }
        // Anything after the end of the line's tokens.
        for m in &mut mask[p.min(end)..end] {
            *m = true;
        }
        at = end;
    }
    mask
}

/// A program with each keyword as the first of its forms: AMOS stores the first when a line is typed, and the
/// one its arguments call for once the program has been tested, so either is right.
fn canonical(code: &[u8]) -> Vec<u8> {
    let mut out = code.to_vec();
    let mut at = 0;
    while at + 2 <= code.len() {
        let length = usize::from(code[at]) * 2;
        if length == 0 {
            break;
        }
        let end = (at + length).min(code.len());
        let mut p = at + 2;
        while p + 2 <= end {
            let token = word(code, p);
            if token == 0 {
                break;
            }
            p += 2;
            if token <= TK_LGO {
                let n = usize::from(code[p + 2]);
                p += if n & 1 == 1 { 5 } else { 4 } + n;
            } else if token < TK_EXT || token == TK_DFL {
                p += match token {
                    TK_CH1 | TK_CH2 => {
                        let n = usize::from(word(code, p));
                        2 + n + (n & 1)
                    }
                    TK_DFL => 8,
                    _ => 4,
                };
            } else if token == TK_EXT {
                p += 4;
            } else {
                if let Some((_, name, _)) = table().by_key.get(&u32::from(token))
                    && let Some((_, keys)) = table().by_name.iter().find(|(n, _)| *n == name.trim_start().to_lowercase())
                {
                    out[p - 2..p].copy_from_slice(&(keys[0] as u16).to_be_bytes());
                }
                p += match token {
                    TK_REM1 | TK_REM2 => {
                        let n = usize::from(code[p + 1]);
                        2 + n + (n & 1)
                    }
                    t => extra(t),
                };
            }
        }
        at = end;
    }
    out
}

/// Whether a line holds a token, at any even place after its header.
fn contains(line: &[u8], token: u16) -> bool {
    (2..line.len().saturating_sub(1)).step_by(2).any(|p| word(line, p) == token)
}

/// AMOS programs from Aminet in ../testmedia/amos/files, if they're there: listed, tokenised again, and the same
/// apart from what AMOS fills in itself.
#[test]
fn real_programs_list_and_tokenise_back_to_the_same() {
    let folder = std::path::Path::new("../testmedia/amos/files");
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut checked = 0;
    let mut problems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let bytes = std::fs::read(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let listed = list(&bytes);
        if listed.notes.iter().any(|n| n.contains("Extension_")) {
            continue;
        }
        let again = match tokenise(&listed.lines) {
            Ok(again) => again,
            Err(e) => {
                problems.push(format!("{name}: {e}"));
                continue;
            }
        };
        let length = long(&bytes, 16) as usize;
        let mut original = bytes[20..20 + length].to_vec();
        // Locked procedures are compared unlocked.
        let mut at = 0;
        while at + 4 <= original.len() && original[at] > 0 {
            if word(&original, at + 2) == TK_PROC && original[at + 10] & 0x20 != 0 {
                decrypt_procedure(&mut original, at);
            }
            at += usize::from(original[at]) * 2;
        }
        let ours = &again[20..20 + long(&again, 16) as usize];
        let mask = filled_in(&original, bytes[11] == b'V');
        let original = canonical(&original);
        let ours = &canonical(ours)[..];
        // Line by line, each program's own way through: a line can be a different length (a float in the
        // other precision) and the rest still the same.
        let ours_listed = list(&again).lines;
        let (mut pa, mut pb, mut n) = (0, 0, 0);
        let mut bad = Vec::new();
        while pa < original.len() && pb < ours.len() {
            let (la, lb) = (usize::from(original[pa]) * 2, usize::from(ours[pb]) * 2);
            if la == 0 || lb == 0 {
                break;
            }
            let theirs = &original[pa..(pa + la).min(original.len())];
            let mine = &ours[pb..(pb + lb).min(ours.len())];
            let same = la == lb && theirs.iter().zip(mine).zip(&mask[pa..]).all(|((a, b), m)| *m || a == b);
            let floats = contains(theirs, TK_DFL) || contains(theirs, TK_FL);
            if !same && !(floats && ours_listed.get(n) == listed.lines.get(n)) && bad.len() < 3 {
                bad.push(format!("{name} line {}: {:?}\n theirs {:02X?}\n ours   {:02X?}", n + 1, listed.lines.get(n), theirs, mine));
            }
            pa += la;
            pb += lb;
            n += 1;
        }
        if bad.is_empty() {
            checked += 1;
        }
        problems.extend(bad);
    }
    assert!(problems.is_empty(), "{} same; {}", checked, problems.join("\n"));
    assert!(checked >= 10, "only {checked} programs were compared");
}
