use super::*;

fn testmedia(path: &str) -> Option<std::path::PathBuf> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testmedia").join(path);
    p.exists().then_some(p)
}

/// Bytes that look like nothing in particular.
fn junk(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed | 1;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

#[test]
fn every_test_module_is_found_in_junk_whole_and_no_more() {
    let Some(folder) = testmedia("trackers") else { return };
    let mut looked = 0;
    for entry in std::fs::read_dir(folder).unwrap().flatten() {
        let module = std::fs::read(entry.path()).unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        // A 15-sample Soundtracker module carries no mark; it's found by its instruments' sample-disk names.
        if name.to_ascii_lowercase().ends_with(".mod") && !module.get(1080..1084).is_some_and(|t| t.iter().all(|c| c.is_ascii_graphic())) && soundtracker_starts(&module).is_empty() {
            continue;
        }
        let mut bytes = junk(3001, 7);
        bytes.extend(&module);
        bytes.extend(junk(5000, 11));
        let finds = find_music(&bytes, "test");
        let found = finds.iter().find(|f| f.offset == 3001).unwrap_or_else(|| panic!("{name}: {:?}", finds.iter().map(|f| (f.offset, f.kind)).collect::<Vec<_>>()));
        assert!(found.sized, "{name}");
        // Exactly the module: or, where the file has a little more after its last sample, no more than that.
        assert!(found.bytes.len() <= module.len() && module.len() - found.bytes.len() < 64, "{name}: {} of {}", found.bytes.len(), module.len());
        assert_eq!(&found.bytes[..], &module[..found.bytes.len()], "{name}");
        // And it still loads and plays as the module.
        assert!(crate::tracker::load(&found.bytes, &found.file_name()).is_ok(), "{name} loads as {}", found.file_name());
        looked += 1;
    }
    assert!(looked > 10);
}

/// A 15-sample Soundtracker module of two patterns, its first instrument named `first`.
fn soundtracker_module(first: &[u8]) -> Vec<u8> {
    let mut m = b"old tune".to_vec();
    m.resize(20, 0);
    for s in 0..15 {
        let mut name = if s == 0 { first.to_vec() } else { Vec::new() };
        name.resize(22, 0);
        m.extend(name);
        // 100 words long at volume 64, playing once.
        m.extend(if s < 3 { [0, 100, 0, 64, 0, 0, 0, 1] } else { [0, 0, 0, 0, 0, 0, 0, 1] });
    }
    m.extend([3, 120]);
    let mut order = vec![0u8, 1, 0];
    order.resize(128, 0);
    m.extend(order);
    m.extend(vec![0u8; 2 * 1024]);
    m.extend(junk(3 * 200, 3));
    m
}

#[test]
fn a_soundtracker_module_is_found_by_its_sample_disk_names_or_as_a_whole_file() {
    let module = soundtracker_module(b"st-01:strings");
    assert_eq!(module.len(), 600 + 2048 + 600);
    let mut bytes = junk(4000, 5);
    bytes.extend(&module);
    bytes.extend(junk(3000, 9));
    let finds = find_music(&bytes, "disk");
    assert_eq!(finds.len(), 1, "{:?}", finds.iter().map(|f| (f.offset, f.kind)).collect::<Vec<_>>());
    let found = &finds[0];
    assert_eq!((found.offset, found.kind, found.title.as_str(), found.sized), (4000, Music::Soundtracker, "old tune", true));
    assert_eq!(&found.bytes[..], &module[..]);
    assert_eq!(found.file_name(), "old tune.mod");
    assert!(crate::tracker::load(&found.bytes, &found.file_name()).is_ok());
    // Without such a name, only a file that is exactly one module is taken for one.
    let unnamed = soundtracker_module(b"strings");
    let mut inside = junk(4000, 5);
    inside.extend(&unnamed);
    assert!(find_music(&inside, "disk").is_empty());
    let whole = find_music(&unnamed, "old.mod");
    assert_eq!(whole.iter().map(|f| (f.offset, f.kind, f.bytes.len())).collect::<Vec<_>>(), [(0, Music::Soundtracker, unnamed.len())]);
    // Junk with a sample-disk name in it isn't a module.
    let mut fake = junk(5000, 13);
    fake[2000..2006].copy_from_slice(b"ST-02:");
    assert!(find_music(&fake, "junk").is_empty());
}

#[test]
fn sid_sndh_and_ym_are_found_with_their_names() {
    // A PSID header, version 2, and a little code after it.
    let mut sid = b"PSID".to_vec();
    sid.extend([0, 2, 0, 0x7C, 0x10, 0x00, 0x10, 0x00, 0x10, 0x03, 0, 1, 0, 1, 0, 0, 0, 0]);
    let mut name = b"Test Tune".to_vec();
    name.resize(32, 0);
    sid.extend(&name);
    let mut author = b"Someone".to_vec();
    author.resize(32, 0);
    sid.extend(&author);
    sid.resize(0x7C, 0);
    sid.extend([0x60; 40]);
    let mut bytes = junk(500, 3);
    bytes.extend(&sid);
    let finds = find_music(&bytes, "disk");
    assert_eq!(finds.len(), 1);
    assert_eq!((finds[0].offset, finds[0].kind, finds[0].title.as_str()), (500, Music::Sid, "Test Tune - Someone"));
    assert_eq!(finds[0].file_name(), "Test Tune - Someone.sid");
    // An SNDH, packed with Pack-Ice: found by what it unpacks to, and exactly as long as its header says.
    let mut sndh = vec![0x60, 0x00, 0x00, 0x10, 0x60, 0x00, 0x00, 0x20, 0x60, 0x00, 0x00, 0x30];
    sndh.extend(b"SNDHTITLThe Packed One\0COMMNobody\0HDNS");
    sndh.extend(junk(2000, 9).iter().map(|b| b & 0x0F));
    let packed = crate::st_packers::tests::ice_pack(&sndh, 1);
    let mut bytes = junk(100, 5);
    bytes.extend(&packed);
    bytes.extend(junk(300, 6));
    let finds = find_music(&bytes, "st disk");
    let ice = finds.iter().find(|f| f.kind == Music::SndhPacked).unwrap();
    assert_eq!((ice.offset, ice.bytes.len(), ice.title.as_str()), (100, packed.len(), "The Packed One"));
    // A YM6: frames of 16 registers, between its header and End!.
    let mut ym = b"YM6!LeOnArD!".to_vec();
    ym.extend(3u32.to_be_bytes());
    ym.extend([0, 0, 0, 1]);
    ym.extend([0, 0]);
    ym.extend(2_000_000u32.to_be_bytes());
    ym.extend(50u16.to_be_bytes());
    ym.extend([0, 0, 0, 0, 0, 0]);
    ym.extend(b"Song\0Me\0\0");
    ym.extend([7u8; 48]);
    ym.extend(b"End!");
    let mut bytes = junk(64, 8);
    bytes.extend(&ym);
    bytes.extend(junk(64, 4));
    let finds = find_music(&bytes, "x");
    assert_eq!((finds[0].offset, finds[0].bytes.len(), finds[0].title.as_str(), finds[0].kind), (64, ym.len(), "Song", Music::Ym));
}

#[test]
fn junk_alone_has_no_music() {
    for seed in 1..20 {
        let finds = find_music(&junk(200_000, seed), "junk");
        assert!(finds.is_empty(), "{:?}", finds.iter().map(|f| (f.offset, f.kind)).collect::<Vec<_>>());
    }
}

#[test]
fn music_on_real_disks_is_found() {
    // An Amiga game's demo and an Archimedes demo disk, each with ProTracker modules on it.
    for (path, at_least) in [("virtual-karting-demo/VirtualKarting2Demo.adf", 5), ("ace2-tcu1-ge72-crimbo-demos-archimedes-1990/ACE2, TCU1, Ge72, Crimbo Demos.adf", 1)] {
        let Some(file) = testmedia(path) else { continue };
        let bytes = std::fs::read(&file).unwrap();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let finds = find_music_everywhere(&bytes, &name);
        eprintln!("{:?}", finds.iter().map(|f| (&f.place, f.offset, f.kind, &f.title, f.bytes.len())).collect::<Vec<_>>());
        let modules: Vec<&Found> = finds.iter().filter(|f| f.kind == Music::ProTracker).collect();
        assert!(modules.len() >= at_least, "{name}: {} modules", modules.len());
        for f in modules {
            assert!(crate::tracker::load(&f.bytes, "x.mod").is_ok(), "{name}: {} loads", f.title);
        }
    }
}

#[test]
fn bytes_are_seen_as_each_machine_s_pictures() {
    let pixel = |p: &Picture, x: usize, y: usize| -> [u8; 3] {
        let i = (y * p.width + x) * 4;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2]]
    };
    // An 8x8 character: its second row's first bit lit, and the next character's first row full.
    let mut bytes = vec![0u8; 32];
    bytes[1] = 0x80;
    bytes[8] = 0xFF;
    let p = picture(&bytes, &View { layout: Layout::Characters, offset: 0, across: 2, down: 1, planes: 1 });
    assert_eq!((p.width, p.height, p.bytes), (16, 8, 16));
    assert_eq!((pixel(&p, 0, 1), pixel(&p, 1, 1), pixel(&p, 8, 0), pixel(&p, 15, 0)), ([255; 3], [0; 3], [255; 3], [255; 3]));
    // An NES tile: low plane row 0 = $80, high plane row 0 = $C0: colours 3 then 2.
    let mut tile = vec![0u8; 16];
    tile[0] = 0x80;
    tile[8] = 0xC0;
    let p = picture(&tile, &View { layout: Layout::Nes, offset: 0, across: 1, down: 1, planes: 2 });
    assert_eq!((pixel(&p, 0, 0), pixel(&p, 1, 0), pixel(&p, 2, 0)), ([255; 3], [170; 3], [0; 3]));
    // The same as a Game Boy tile: the two planes of row 0 are bytes 0 and 1.
    let gb = picture(&[0x80, 0x80], &View { layout: Layout::GameBoy, offset: 0, across: 1, down: 1, planes: 2 });
    assert_eq!(pixel(&gb, 0, 0), [8, 24, 32]);
    // The ST's planes, a word each: plane 1 sets the leftmost pixel to colour 2.
    let st = picture(&[0, 0, 0x80, 0], &View { layout: Layout::AtariSt, offset: 0, across: 1, down: 1, planes: 2 });
    assert_eq!(pixel(&st, 0, 0), [170; 3]);
    // The Spectrum's screen: line 1 is 256 bytes on from line 0, and line 8 is 32 bytes on.
    let mut scr = vec![0u8; 6912];
    scr[256] = 0x80;
    scr[32] = 0x80;
    scr[6144..].fill(0x07);
    let p = picture(&scr, &View { layout: Layout::Spectrum, offset: 0, across: 1, down: 1, planes: 1 });
    assert_eq!((pixel(&p, 0, 1), pixel(&p, 0, 8), pixel(&p, 0, 2)), ([205; 3], [205; 3], [0; 3]));
    // The view's bytes as source.
    let source = as_source(&bytes, &View { layout: Layout::Characters, offset: 8, across: 1, down: 1, planes: 1 }, "db");
    assert!(source.contains("db $FF,$00,$00,$00,$00,$00,$00,$00"), "{source}");
}
