use super::*;
use crate::asm::hunk;

/// The test files: made by crunching Limoniter's own files with the real crunchers (see testdata/README.txt).
fn testdata(name: &str) -> Vec<u8> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/amiga_crunch/testdata").join(name)).unwrap()
}

/// An executable without its symbol hunks, which every cruncher drops.
fn without_symbols(file: &[u8]) -> Vec<u8> {
    let long = |at: usize| u32::from_be_bytes(file[at..at + 4].try_into().unwrap());
    let header = 20 + 4 * long(8) as usize;
    let mut out = file[..header].to_vec();
    let mut at = header;
    while at < file.len() {
        match long(at) {
            0x3E9 | 0x3EA => {
                let end = at + 8 + 4 * long(at + 4) as usize;
                out.extend(&file[at..end]);
                at = end;
            }
            0x3EB => {
                out.extend(&file[at..at + 8]);
                at += 8;
            }
            0x3EC => {
                let mut end = at + 4;
                while long(end) != 0 {
                    end += 8 + 4 * long(end) as usize;
                }
                out.extend(&file[at..end + 4]);
                at = end + 4;
            }
            0x3F0 => {
                at += 4;
                while long(at) != 0 {
                    at += 8 + 4 * (long(at) & 0xFF_FFFF) as usize;
                }
                at += 4;
            }
            _ => {
                out.extend(&file[at..at + 4]);
                at += 4;
            }
        }
    }
    out
}

/// What a program puts in memory when it's loaded, its hunks at these addresses: what it does when it runs.
fn loaded(file: &[u8]) -> Vec<Vec<u8>> {
    let hunks = hunk::read_executable(file).unwrap();
    let addresses: Vec<u32> = (0..hunks.len() as u32).map(|i| 0x10_0000 + i * 0x1_0000).collect();
    hunk::load(&hunks, &addresses)
}

#[test]
fn crunched_data_decrunches_to_what_was_crunched() {
    let text = testdata("text.txt");
    let sound = testdata("sound.raw");
    for (file, name, original) in [
        ("text.pp", "PowerPacker", &text),
        ("text.imp", "Imploder", &text),
        ("text.crm", "CrunchMania", &text),
        ("text.crm2", "CrunchMania (LZH mode)", &text),
        ("sound.crms", "CrunchMania (sampled)", &sound),
        ("sound.crms2", "CrunchMania (LZH mode, sampled)", &sound),
        ("text.stc", "StoneCracker 4.10", &text),
    ] {
        let crunched = testdata(file);
        let found = recognise(&crunched).unwrap_or_else(|| panic!("{file} isn't recognised"));
        assert_eq!((found.name.as_str(), found.program, found.size), (name, false, Some(original.len())), "{file}");
        let decrunched = decrunch(&crunched).unwrap().unwrap();
        assert!(decrunched.bytes == *original, "{file} doesn't decrunch to what was crunched");
        assert_eq!(contents(&crunched).as_ref(), original.as_slice());
        assert_eq!(decrunched.said(), format!("This file was crunched with {name}"));
    }
}

#[test]
fn crunched_programs_are_built_again_as_they_were() {
    let original = testdata("hello.exe");
    let stripped = without_symbols(&original);
    for (file, name, same_bytes) in [
        ("hello.stc", "StoneCracker 4.10", true),
        ("hello.stclib", "StoneCracker 4.10 (its decruncher in stc.library)", true),
        ("hello.imp", "Imploder", true),
        ("hello.implib", "Imploder (its decruncher in explode.library)", false),
        ("hello.crm", "CrunchMania", false),
        ("hello.crmlib", "CrunchMania (its decruncher in CrM.library)", false),
    ] {
        let crunched = testdata(file);
        let found = recognise(&crunched).unwrap_or_else(|| panic!("{file} isn't recognised"));
        assert_eq!((found.name.as_str(), found.program, found.load), (name, true, None), "{file}");
        let built = decrunch(&crunched).unwrap().unwrap().bytes;
        let ours = hunk::read_executable(&built).unwrap();
        let theirs = hunk::read_executable(&original).unwrap();
        // The same hunks, of the same kinds, in the same memory, with the same relocations.
        assert_eq!(ours.len(), theirs.len(), "{file}");
        for (a, b) in ours.iter().zip(&theirs) {
            assert_eq!((a.kind, a.memory), (b.kind, b.memory), "{file}");
            let mut ra = a.relocations.clone();
            let mut rb = b.relocations.clone();
            ra.sort_unstable();
            rb.sort_unstable();
            assert_eq!(ra, rb, "{file}");
        }
        // Library imploding makes the first hunk room enough for its root, which stays in the size it's given.
        if file != "hello.implib" {
            assert_eq!(loaded(&built), loaded(&original), "{file}");
        }
        if same_bytes {
            assert!(built == stripped, "{file} isn't the original program, byte for byte, but for its symbols");
        }
    }
}

#[test]
fn a_program_is_shown_for_what_crunched_it() {
    let found = recognise(&testdata("hello.stc")).unwrap();
    assert_eq!(said(&found), "This program was crunched with StoneCracker 4.10");
    let sections = sections(&testdata("hello.imp"));
    assert_eq!(sections.len(), 1);
    let rows = &sections[0].rows;
    assert!(rows.iter().any(|(l, v)| l == "Crunched with" && v == "Imploder"));
    assert!(rows.iter().any(|(l, v)| l == "Its hunks" && v == "code 172 bytes, data 760 bytes, data (chip) 440 bytes, BSS 260 bytes"));
    assert!(rows.iter().any(|(l, v)| l == "Relocations" && v == "10"));
}

#[test]
fn files_that_are_not_crunched_are_left_alone() {
    for file in ["hello.exe", "text.txt", "sound.raw", "hello.s"] {
        let bytes = testdata(file);
        assert!(recognise(&bytes).is_none(), "{file}");
        assert!(decrunch(&bytes).is_none(), "{file}");
        assert_eq!(contents(&bytes).as_ref(), bytes.as_slice());
    }
    assert!(recognise(b"PP20").is_none());
    assert!(recognise(&[]).is_none());
    assert!(sections(&testdata("text.txt")).is_empty());
}

#[test]
fn damaged_crunched_files_say_so_without_harm() {
    // Every file cut short or with a byte changed either decrunches (to something) or says it can't; none panics.
    for file in ["text.pp", "text.imp", "text.crm", "text.crm2", "sound.crms2", "text.stc", "hello.stc", "hello.stclib", "hello.imp", "hello.implib", "hello.crm", "hello.crmlib"] {
        let good = testdata(file);
        for at in (0..good.len()).step_by(37) {
            let mut bad = good.clone();
            bad[at] ^= 0x5A;
            let _ = decrunch(&bad);
            let _ = decrunch(&good[..at]);
        }
        // A file cut in half is noticed.
        let half = &good[..good.len() / 2];
        assert!(!matches!(decrunch(half), Some(Ok(d)) if d.bytes == decrunch(&good).unwrap().unwrap().bytes), "{file}");
    }
}

#[test]
fn encrypted_powerpacker_data_is_said_to_be() {
    let mut file = testdata("text.pp");
    file[..4].copy_from_slice(b"PX20");
    file.splice(4..4, [0x12, 0x34]);
    let found = recognise(&file).unwrap();
    assert_eq!(found.name, "PowerPacker, encrypted");
    assert!(decrunch(&file).unwrap().unwrap_err().contains("encrypted with a password"));
}

/// ByteKiller data from literal runs alone, as its decoder reads them: bits taken from the stream's last long
/// backwards, each long XORed into the check, and the bytes written from the end back.
pub(super) fn bytekiller_literals(data: &[u8]) -> Vec<u8> {
    let mut bits: Vec<u32> = Vec::new();
    let push = |value: u32, n: u32, bits: &mut Vec<u32>| (0..n).rev().for_each(|i| bits.push(value >> i & 1));
    for run in data.rchunks(8) {
        push(0, 2, &mut bits);
        push(run.len() as u32 - 1, 3, &mut bits);
        for &b in run.iter().rev() {
            push(u32::from(b), 8, &mut bits);
        }
    }
    let first = bits.len() % 32;
    let mut longs = vec![(1u32 << first) | bits[..first].iter().enumerate().fold(0, |v, (i, b)| v | b << i)];
    for chunk in bits[first..].chunks(32) {
        longs.push(chunk.iter().enumerate().fold(0, |v, (i, b)| v | b << i));
    }
    longs.reverse();
    let check = longs.iter().fold(0, |c, l| c ^ l);
    let mut file = Vec::new();
    file.extend((longs.len() as u32 * 4).to_be_bytes());
    file.extend((data.len() as u32).to_be_bytes());
    file.extend(check.to_be_bytes());
    longs.iter().for_each(|l| file.extend(l.to_be_bytes()));
    file
}

#[test]
fn bytekiller_data_is_known_by_its_check() {
    let text = &testdata("text.txt")[..1000];
    let file = bytekiller_literals(text);
    let found = recognise(&file).unwrap();
    assert_eq!((found.name.as_str(), found.size), ("ByteKiller", Some(1000)));
    assert_eq!(decrunch(&file).unwrap().unwrap().bytes, text);
    // ByteKiller Pro marks its data at the end.
    let mut pro = file.clone();
    pro.extend(b"data");
    assert_eq!(recognise(&pro).unwrap().name, "ByteKiller Pro");
    // A wrong check, or a byte too many, and it isn't taken for ByteKiller's.
    let mut wrong = file.clone();
    wrong[11] ^= 1;
    assert!(recognise(&wrong).is_none());
    let mut longer = file;
    longer.push(0);
    assert!(recognise(&longer).is_none());
}

#[test]
fn worth_reading_picks_out_what_might_be_crunched() {
    assert!(worth_reading(b"PP20\0\0\0\0\0\0\0\0", 100));
    assert!(worth_reading(&[0, 0, 3, 0xF3, 0, 0, 0, 0], 100));
    assert!(worth_reading(&[0, 0, 0, 88, 0, 0, 1, 0], 100));
    assert!(!worth_reading(b"Hello, world", 100));
    assert!(!worth_reading(b"PP", 2));
}

/// For studying decrunchers: DISASM_IN names a file, DISASM_AT where in it to start, DISASM_LEN how much.
#[test]
#[ignore]
fn dump_disassembly() {
    let path = std::env::var("DISASM_IN").unwrap();
    let bytes = std::fs::read(path).unwrap();
    let at: usize = std::env::var("DISASM_AT").ok().map_or(0, |s| usize::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());
    let len: usize = std::env::var("DISASM_LEN").ok().filter(|s| !s.is_empty()).map_or(bytes.len() - at, |s| usize::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());
    let program = crate::m68k_disasm::raw(&bytes[at..at + len], 0, &[0], crate::m68k_disasm::Machine::Amiga);
    println!("{}", crate::m68k_disasm::disassemble(&program));
}

/// For checking against other decrunchers: DECRUNCH_IN names a file, DECRUNCH_OUT where to write what it
/// decrunches to.
#[test]
#[ignore]
fn decrunch_a_file() {
    let bytes = std::fs::read(std::env::var("DECRUNCH_IN").unwrap()).unwrap();
    match decrunch(&bytes) {
        Some(Ok(d)) => {
            println!("FOUND {:?}", d.found);
            std::fs::write(std::env::var("DECRUNCH_OUT").unwrap(), &d.bytes).unwrap();
        }
        Some(Err(e)) => println!("ERROR {e}"),
        None => println!("NOT CRUNCHED"),
    }
}
