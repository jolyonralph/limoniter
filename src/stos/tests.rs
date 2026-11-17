use super::*;

#[test]
fn a_listing_tokenises_and_lists_back() {
    let text = [
        "10 rem A test of STOS",
        "20 mode 0 : key off : curs off : hide on",
        "30 for I=1 to 10 : print \"Hello \";I;A$;X# : next I",
        "40 if A>3 then goto 100 else gosub 200",
        "50 dim PTS(10) : PTS(1)=$FF+%101-2.5",
        "100 ink 3 : bar 10,10 to 100,50 : wait vbl",
        "200 return",
    ];
    let program = tokenise(&text).unwrap();
    assert!(program.starts_with(b"Lionpoulos"));
    let listed = list(&program);
    assert_eq!(listed.lines, text.map(String::from), "{:#?}", listed.lines);
    assert_eq!(tokenise(&listed.lines).unwrap(), program);
    assert_eq!(to_ffp(2.5), 0xA000_0042);
    assert_eq!(ffp(0xC800_0046), 50.0);
}

/// Which bytes STOS fills in as it runs: a variable's place, and where a branch goes. They're not compared.
fn filled_in(program: &[u8]) -> Vec<bool> {
    let mut mask = vec![false; program.len()];
    let mut at = 0;
    while at + 4 <= program.len() {
        let length = word(program, at);
        if length == 0 {
            break;
        }
        let end = (at + length).min(program.len());
        let mut p = at + 4;
        let even = |p: usize| if (p - at) % 2 == 1 { p + 1 } else { p };
        let mut rem = false;
        while p < end {
            let c = program[p];
            p += 1;
            if c == 0 || rem {
                if c == 0 {
                    // The rest of the line after its end.
                    for m in &mut mask[p..end] {
                        *m = true;
                    }
                    break;
                }
                continue;
            }
            if c < 0x80 {
                continue;
            }
            if c >= VARIABLE {
                let q = even(p);
                for m in &mut mask[p..q] {
                    *m = true;
                }
                p = q;
                match c {
                    VARIABLE => {
                        for m in &mut mask[p + 1..p + 4] {
                            *m = true;
                        }
                        p += 4;
                    }
                    STRING => p += 4 + long(program, p) as usize,
                    FLOAT => p += 8,
                    _ => p += 4,
                }
                continue;
            }
            if (0x98..0xA0).contains(&c) {
                let q = even(p) + 4;
                for m in &mut mask[p..q] {
                    *m = true;
                }
                p = q;
            }
            if c == REM {
                rem = true;
            }
            p += match c {
                0xA0 | 0xB8 => 1,
                0xA8 | 0xC0 => 2,
                _ => 0,
            };
        }
        at = end;
    }
    mask
}

/// STOS programs in ../testmedia/stos, if they're there: listed, tokenised again, and the same apart from what
/// STOS fills in as it runs; and listed as STOS saved them as text (.ASC), where that was saved from the same
/// program.
#[test]
fn real_programs_list_and_tokenise_back_to_the_same() {
    let folder = std::path::Path::new("../testmedia/stos");
    if !folder.exists() {
        return;
    }
    let mut files = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(f) = folders.pop() {
        for e in std::fs::read_dir(f).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("bas")) {
                files.push(path);
            }
        }
    }
    files.sort();
    let (mut same, mut listed_same, mut problems) = (0, 0, Vec::new());
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !looks_like(&bytes) {
            continue;
        }
        let listed = list(&bytes);
        let again = match tokenise(&listed.lines) {
            Ok(again) => again,
            Err(e) => {
                problems.push(format!("{name}: {e}"));
                continue;
            }
        };
        let theirs = &bytes[PROGRAM_AT..PROGRAM_AT + (long(&bytes, 14) as usize).min(bytes.len() - PROGRAM_AT)];
        let ours = &again[PROGRAM_AT..];
        let mask = filled_in(theirs);
        let (mut at, mut n, mut bad) = (0, 0, None);
        while at + 4 <= theirs.len().min(ours.len()) {
            let (la, lb) = (word(theirs, at), word(ours, at));
            if la == 0 && lb == 0 {
                break;
            }
            let a = &theirs[at..(at + la).min(theirs.len())];
            let b = &ours[at..(at + lb).min(ours.len())];
            if la != lb || a.iter().zip(b).zip(&mask[at..]).any(|((x, y), m)| !m && x != y) {
                bad = Some(format!("{name} line {}: {:?}\n theirs {:02X?}\n ours   {:02X?}", n + 1, listed.lines.get(n), a, b));
                break;
            }
            at += la;
            n += 1;
        }
        match bad {
            Some(bad) => problems.push(bad),
            None => same += 1,
        }
        // Against STOS's own text listing, saved beside it.
        if let Ok(text) = std::fs::read(path.with_extension("ASC")) {
            let text: Vec<String> = String::from_utf8_lossy(&text).lines().map(|l| l.trim_end_matches('\r').to_string()).filter(|l| !l.is_empty()).collect();
            if text.iter().map(|l| l.trim_end()).eq(listed.lines.iter().map(|l| l.trim_end())) {
                listed_same += 1;
            }
        }
    }
    eprintln!("{same} of {} the same; {listed_same} listed as their .ASC", files.len());
    assert!(problems.is_empty(), "{same} same\n{}", problems.join("\n"));
    assert!(same >= 30, "only {same} compared");
}
