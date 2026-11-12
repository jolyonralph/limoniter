use super::*;

pub fn format_of(id: &str) -> TextFormat {
    TextFormat { charset: charset::known(id), ..TextFormat::default() }
}

pub fn saved(decoded: &Decoded, format: &TextFormat) -> Vec<u8> {
    let eol_of = |i: usize| decoded.exceptions.get(&i).copied().unwrap_or(decoded.base);
    encode(&decoded.lines, &eol_of, format).unwrap()
}

fn round_trip_as(data: &[u8], mut format: TextFormat) -> Vec<u8> {
    let decoded = decode(data, &mut format);
    saved(&decoded, &format)
}

fn round_trip(data: &[u8]) -> Vec<u8> {
    let found = detect(data);
    round_trip_as(data, TextFormat { charset: found.charset, ..TextFormat::default() })
}

#[test]
fn plain_files_come_back_byte_for_byte() {
    for data in [
        &b""[..],
        b"one line",
        b"a\r\nb\r\n",
        b"a\nb\n",
        b"a\rb\r",
        b"\xEF\xBB\xBFwith bom\r\nx",
        b"\xFF\xFEa\x00\r\x00\n\x00b\x00",
        b"\xFE\xFF\x00a\x00\n\x00b",
        b"\xFF\xFE\x00\x00a\x00\x00\x00\n\x00\x00\x00",
        b"\x00\x00\xFE\xFF\x00\x00\x00a\x00\x00\x00\n",
        b"caf\xE9 not utf8\n",
        "caf\u{e9} utf8\n".as_bytes(),
    ] {
        assert_eq!(round_trip(data), data, "for {data:?}");
    }
}

/// Every byte in every set, alone and among others, in every line policy.
#[test]
fn every_single_byte_value_survives_untouched_in_every_set() {
    let all: Vec<u8> = (0..=255).collect();
    let reversed: Vec<u8> = (0..=255).rev().collect();
    for set in charset::all() {
        for policy in NewlinePolicy::CHOICES {
            for data in [&all, &reversed] {
                let format = TextFormat { charset: set, newline: policy, ..TextFormat::default() };
                assert_eq!(&round_trip_as(data, format), data, "{} with {policy:?}", set.id);
            }
            for value in 0..=255u8 {
                let data = [b'x', value, b'y'];
                let format = TextFormat { charset: set, newline: policy, ..TextFormat::default() };
                assert_eq!(round_trip_as(&data, format), data, "{} byte {value:02X} with {policy:?}", set.id);
            }
        }
    }
}

#[test]
fn invalid_unicode_survives() {
    for data in [&b"ok\xFF\xFEbad\xC3\n\xE2\x82"[..], b"\xED\xA0\x80 surrogate", b"\xF4\x8F\xBC\x80 is the private range", b"\xF4\x90\x80\x80 too high", b"\xC0\xAF overlong"] {
        assert_eq!(round_trip_as(data, format_of("utf-8")), data, "{data:?}");
    }
    for data in [&b"\xFF\xFEabc"[..], b"\xFF\xFE\x00\xD8", b"\xFF\xFE\x00\xDCa\x00", b"\xFF\xFE\xFF\xDB\x00\xDF"] {
        assert_eq!(round_trip_as(data, format_of("utf-16le")), data, "{data:?}");
    }
    assert_eq!(round_trip_as(b"\xFE\xFF\xD8\x00", format_of("utf-16be")), b"\xFE\xFF\xD8\x00");
    for data in [&b"\xFF\xFE\x00\x00\x00\x00\x11\x00"[..], b"\xFF\xFE\x00\x00\x00\xD8\x00\x00", b"\xFF\xFE\x00\x00abc"] {
        assert_eq!(round_trip_as(data, format_of("utf-32le")), data, "{data:?}");
    }
}

#[test]
fn nothing_is_ever_replaced_or_normalised() {
    let mut format = format_of("utf-8");
    let decoded = decode(b"a\xFFb", &mut format);
    assert!(!decoded.lines[0].contains('\u{FFFD}'));
    assert_eq!(decoded.lines[0], format!("a{}b", raw_char(0xFF)));
    let composed = "e\u{301} \u{e9}";
    let mut format = format_of("utf-8");
    let decoded = decode(composed.as_bytes(), &mut format);
    assert_eq!(decoded.lines[0], composed, "both spellings of the same letter stay as they are");
    assert_eq!(saved(&decoded, &format), composed.as_bytes());
}

#[test]
fn line_endings_of_all_kinds_and_final_newlines_survive() {
    for data in [&b"a\rb\rc"[..], b"a\nb\nc\n", b"a\r\nb\r\nc", b"a\r\nb\nc\rd\n\re", b"\n\r\n\r", b"a\n\r\nb", b"no newline", b"x\n"] {
        assert_eq!(round_trip_as(data, format_of("utf-8")), data);
        assert_eq!(round_trip_as(data, format_of("windows-1252")), data);
        assert_eq!(round_trip_as(data, format_of("cp437")), data);
    }
}

#[test]
fn trailing_spaces_controls_and_end_of_file_marks_survive() {
    let data = b"text   \t \r\n\x1B[31mred\x1B[0m\x00\x07\r\nlast\x1A\x1A\x1A\x00\x00";
    for id in ["cp437", "cp850", "windows-1252", "iso-8859-1", "us-ascii", "mac-roman", "amiga-text", "atari-st", "msx"] {
        assert_eq!(round_trip_as(data, format_of(id)), data, "{id}");
    }
    assert_eq!(round_trip_as(data, format_of("utf-8")), data);
}

#[test]
fn mixed_line_endings_are_reported_and_new_lines_use_the_commonest() {
    let mut format = format_of("utf-8");
    let decoded = decode(b"a\r\nb\nc\r\n", &mut format);
    assert_eq!(decoded.lines, ["a", "b", "c", ""]);
    assert!(decoded.counts.is_mixed());
    assert_eq!(decoded.counts, EolCounts { crlf: 2, lf: 1, cr: 0, atascii: 0, nl: 0 });
    assert_eq!(decoded.base, Eol::CrLf);
    assert_eq!(decoded.exceptions.get(&1), Some(&Eol::Lf));
    assert_eq!(decoded.counts.predominant(Eol::Cr), Eol::CrLf);
    let tied = EolCounts { crlf: 1, lf: 1, cr: 0, atascii: 0, nl: 0 };
    assert_eq!(tied.predominant(Eol::Cr), Eol::Cr, "a tie falls back to the preset");
    assert_eq!(EolCounts::default().predominant(Eol::Lf), Eol::Lf, "and so does having none");
}

#[test]
fn utf16_and_utf32_find_line_endings_on_character_boundaries() {
    // U+010A and U+0A0D hold the bytes 0A and 0D, but they aren't line endings.
    let mut format = format_of("utf-16le");
    let data = [0x0Au8, 0x01, 0x0D, 0x0A, 0x41, 0x00];
    let decoded = decode(&data, &mut format);
    assert_eq!(decoded.lines, ["\u{10A}\u{A0D}A"]);
    assert_eq!(saved(&decoded, &format), data);

    let mut format = format_of("utf-16be");
    let data = [0x01u8, 0x0A, 0x00, 0x0D, 0x00, 0x0A, 0x00, 0x41];
    let decoded = decode(&data, &mut format);
    assert_eq!(decoded.lines, ["\u{10A}", "A"]);
    assert_eq!(decoded.counts.crlf, 1);
    assert_eq!(saved(&decoded, &format), data);

    let mut format = format_of("utf-32le");
    let data = [0x0Au8, 0x01, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x01, 0x00];
    let decoded = decode(&data, &mut format);
    assert_eq!(decoded.lines, ["\u{10A}", "\u{10A00}"]);
    assert_eq!(decoded.counts.lf, 1);
    assert_eq!(saved(&decoded, &format), data);
}

#[test]
fn only_real_line_endings_break_lines() {
    let mut format = format_of("utf-8");
    let decoded = decode("a\u{85}b\u{2028}c\u{1A}d\0e".as_bytes(), &mut format);
    assert_eq!(decoded.lines.len(), 1, "NEL, line separator, Ctrl-Z and NUL stay in the line");
    let mut format = format_of("iso-8859-1");
    assert_eq!(decode(b"a\x85b\x1Ac\x00d", &mut format).lines.len(), 1);
}

#[test]
fn duplicate_mappings_keep_their_own_bytes() {
    // PETSCII 0x61 and 0xC1 are both a spade, and they must not become one byte.
    let mut format = TextFormat { newline: NewlinePolicy::CrOnly, ..format_of("petscii-c64") };
    let data = [0x61u8, 0xC1, 0x41, 0x0E, 0x41, 0xC1, 0x0D, 0x93, 0x12];
    let decoded = decode(&data, &mut format);
    assert_eq!(decoded.lines.len(), 2);
    assert_eq!(saved(&decoded, &format), data);
    let spade: Vec<char> = decoded.lines[0].chars().take(2).collect();
    assert_ne!(spade[0], spade[1]);
}

#[test]
fn a_small_edit_leaves_unrelated_bytes_alone() {
    let data = b"caf\xE9\x81\xFF\x1B[0m\r\nkeep\x90 me\nlast\x1A";
    let mut format = format_of("windows-1252");
    let mut decoded = decode(data, &mut format);
    let edited = decoded.lines[1].replace("keep", "KEEP");
    decoded.lines[1] = edited;
    assert_eq!(saved(&decoded, &format), b"caf\xE9\x81\xFF\x1B[0m\r\nKEEP\x90 me\nlast\x1A");
}

#[test]
fn characters_that_wont_fit_are_listed_with_their_place_before_saving() {
    let format = format_of("iso-8859-1");
    let lines = ["caf\u{e9}", "a\u{20AC}b\u{4E2D}"];
    let eol_of = |_: usize| Eol::Lf;
    let problems = encode(&lines, &eol_of, &format).unwrap_err();
    assert_eq!(problems.total, 2);
    assert_eq!(problems.listed[0], Problem { line: 1, column: 1, ch: '\u{20AC}' });
    assert_eq!(problems.listed[1], Problem { line: 1, column: 3, ch: '\u{4E2D}' });
}

#[test]
fn exports_only_lose_text_by_explicit_choice() {
    let format = format_of("us-ascii");
    let lines = ["caf\u{e9} \u{20AC}\u{4E2D}"];
    let eol_of = |_: usize| Eol::Lf;
    assert!(encode(&lines, &eol_of, &format).is_err());
    assert_eq!(encode_with(&lines, &eol_of, &format, Lossy::Omit).unwrap(), b"caf ");
    assert_eq!(encode_with(&lines, &eol_of, &format, Lossy::Replace).unwrap(), b"caf? ??");
    assert_eq!(encode_with(&lines, &eol_of, &format, Lossy::Transliterate).unwrap(), b"cafe EUR?");
}

#[test]
fn conversion_keeps_the_text_and_finds_what_wont_fit() {
    let mut opened = format_of("cp437");
    let decoded = decode(b"caf\x82 \xE9 \xB0", &mut opened);
    let as_utf8 = convert_lines(&decoded.lines, &opened, &format_of("utf-8")).unwrap();
    assert_eq!(as_utf8, ["caf\u{e9} \u{398} \u{2591}"]);
    let problems = convert_lines(&as_utf8, &format_of("utf-8"), &format_of("iso-8859-1")).unwrap_err();
    assert_eq!(problems.total, 2);
    assert_eq!(problems.listed[0].ch, '\u{398}');
    let raw = convert_lines(&[raw_char(0xFF).to_string()], &format_of("utf-8"), &format_of("cp437"));
    assert_eq!(raw.unwrap_err().total, 1, "a stray byte has no meaning to carry over");
    let petscii = convert_lines(&["HI"], &format_of("utf-8"), &format_of("petscii-c64")).unwrap();
    assert_eq!(petscii[0], format!("{}{}", raw_char(0x48), raw_char(0x49)));
}

#[test]
fn amiga_csi_and_atascii_eol_read_differently() {
    let data = [b'a', 0x9B, b'3', b'1', b'm', b'b'];
    let mut amiga = TextFormat { display: Display::AmigaConsole, ..format_of("amiga-text") };
    let decoded = decode(&data, &mut amiga);
    assert_eq!(decoded.lines.len(), 1);
    assert_eq!(decoded.lines[0].chars().nth(1), Some(raw_char(0x9B)));
    assert_eq!(saved(&decoded, &amiga), data);

    let mut atari = preset_by_id("atari-8bit").unwrap().format();
    let decoded = decode(&data, &mut atari);
    assert_eq!(decoded.lines.len(), 2);
    assert_eq!(decoded.counts.atascii, 1);
    assert_eq!(saved(&decoded, &atari), data);
    let mut atari = preset_by_id("atari-8bit").unwrap().format();
    let decoded = decode(b"a\rb\nc", &mut atari);
    assert_eq!(decoded.lines.len(), 1, "CR and LF are ordinary bytes on ATASCII");
}

#[test]
fn teletext_rows_and_controls_survive_with_no_line_endings_added() {
    let mut row: Vec<u8> = vec![0x01, b'R', b'E', b'D', 0x11, 0x7F, 0x1A, 0x21];
    row.resize(40, 0x20);
    let mut data = row.clone();
    data.extend_from_slice(&row);
    data.extend_from_slice(b"\x0D\x0Apartial");
    let mut format = preset_by_id("teletext").unwrap().format();
    let decoded = decode(&data, &mut format);
    assert_eq!(decoded.lines.len(), 3);
    assert_eq!(decoded.lines[0].chars().count(), 40);
    assert_eq!(decoded.base, Eol::None);
    assert_eq!(saved(&decoded, &format), data);
    let exact = vec![0x41u8; 80];
    let mut format = preset_by_id("teletext").unwrap().format();
    let decoded = decode(&exact, &mut format);
    assert_eq!(decoded.lines.len(), 2, "a whole last row isn't followed by an empty one");
    assert_eq!(saved(&decoded, &format), exact);
    let mut mode7 = TextFormat { newline: NewlinePolicy::FixedRows { width: 40, parity: false }, display: Display::Mode7, ..format_of("bbc-mode7") };
    let screen: Vec<u8> = (0..=255).chain(0..=255).collect();
    let decoded = decode(&screen, &mut mode7);
    assert_eq!(saved(&decoded, &mode7), screen);
}

#[test]
fn detection_trusts_marks_and_only_suggests_otherwise() {
    assert_eq!(detect(b"\xEF\xBB\xBFx").certainty, Certainty::Bom);
    assert_eq!(detect(b"\xFF\xFEx\x00").charset.id, "utf-16le");
    assert_eq!(detect(b"\xFE\xFF\x00x").charset.id, "utf-16be");
    assert_eq!(detect(b"\xFF\xFE\x00\x00x\x00\x00\x00").charset.id, "utf-32le");
    assert_eq!(detect(b"\x00\x00\xFE\xFF\x00\x00\x00x").charset.id, "utf-32be");
    assert_eq!(detect(b"plain").certainty, Certainty::AsciiOnly);
    assert_eq!(detect("caf\u{e9}".as_bytes()).certainty, Certainty::Valid);
    let legacy = detect(b"caf\xE9");
    assert_eq!(legacy.charset.id, "windows-1252");
    assert_eq!(legacy.certainty, Certainty::Suggested);
    assert_eq!(detect(b"\xC9\xCD\xCD\xCD\xCD\xCD\xCD\xBB\xBA\xBA").charset.id, "cp437");
    let mostly = detect("caf\u{e9} d\u{e9}j\u{e0} \u{FF}".as_bytes().iter().copied().chain([0xFF]).collect::<Vec<u8>>().as_slice());
    assert_eq!((mostly.charset.id.as_str(), mostly.certainty), ("utf-8", Certainty::Suggested));
    let wide = detect(b"h\x00e\x00l\x00l\x00o\x00");
    assert_eq!((wide.charset.id.as_str(), wide.certainty), ("utf-16le", Certainty::Suggested));
    for data in [&[0x93u8, 0x12, 0x41, 0x0D][..], &[0x41, 0x9B, 0x42, 0x9B], &[0x01, 0x41, 0x11, 0x21]] {
        let found = detect(data);
        assert!(!matches!(found.charset.id.as_str(), "petscii-c64" | "atascii" | "teletext-en"), "never guessed from a few bytes");
        assert_ne!(found.certainty, Certainty::Bom);
    }
}

#[test]
fn a_mark_is_only_kept_if_it_was_there() {
    let mut format = TextFormat { has_bom: true, ..format_of("utf-8") };
    decode(b"no mark", &mut format);
    assert!(!format.has_bom);
    let mut format = format_of("utf-8");
    decode(b"\xEF\xBB\xBFmark", &mut format);
    assert!(format.has_bom);
    // Reading a marked file as a byte set shows the mark as the characters it then is.
    let mut format = format_of("cp437");
    let decoded = decode(b"\xEF\xBB\xBFmark", &mut format);
    assert_eq!(decoded.lines[0].chars().count(), 7);
    assert_eq!(saved(&decoded, &format), b"\xEF\xBB\xBFmark");
}

#[test]
fn every_preset_makes_a_format_that_round_trips() {
    let data: Vec<u8> = (0..=255).collect();
    for preset in &PRESETS {
        let mut format = preset.format();
        assert_eq!(format.charset.id, preset.charset, "{} points at a set that exists", preset.id);
        let decoded = decode(&data, &mut format);
        assert_eq!(saved(&decoded, &format), data, "{}", preset.id);
    }
    assert_eq!(preset_by_id("dos-western").unwrap().charset, "cp850");
    assert_eq!(preset_by_id("modern-mac").unwrap().eol, Eol::Lf);
    assert_eq!(preset_by_id("classic-mac").unwrap().eol, Eol::Cr);
}

#[test]
fn binary_detection() {
    assert!(looks_binary(b"MZ\x00\x01"));
    assert!(!looks_binary(b"plain text"));
    assert!(!looks_binary(b"\xFF\xFEa\x00"));
}

#[test]
fn a_position_and_its_byte_offset_convert_both_ways() {
    for (data, id) in [
        (&b"\xEF\xBB\xBFab\r\ncd\xC3\xA9f\r\nend"[..], "utf-8"),
        (b"ab\ncd\nend", "utf-8"),
        (b"\xFF\xFEa\x00\r\x00\n\x00b\x00", "utf-16le"),
        (b"caf\xE9\r\nsecond", "windows-1252"),
        (b"a\r\nb\nc\rd", "cp437"),
        (b"\xFF\xFE\x00\x00a\x00\x00\x00\n\x00\x00\x00b\x00\x00\x00", "utf-32le"),
    ] {
        let mut format = format_of(id);
        let decoded = decode(data, &mut format);
        let eol_of = |i: usize| decoded.exceptions.get(&i).copied().unwrap_or(decoded.base);
        for (line, text) in decoded.lines.iter().enumerate() {
            for index in 0..=text.chars().count() {
                let offset = offset_of_position(&decoded.lines, &eol_of, line, index, &format);
                assert_eq!(position_of_offset(data, offset, &format), (line, index), "in {data:?}");
            }
        }
    }
    // An offset in the middle of a character lands on that character's start.
    let data = "a\u{e9}b".as_bytes();
    let mut format = format_of("utf-8");
    decode(data, &mut format);
    assert_eq!(position_of_offset(data, 2, &format), (0, 1));
}

#[test]
fn saving_replaces_the_file_and_leaves_no_temp() {
    let dir = std::env::temp_dir().join(format!("limoniter-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved.txt");
    write_file(&path, b"first").unwrap();
    write_file(&path, b"second").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert!(!dir.join("saved.txt.limoniter-tmp").exists());
    fs::remove_dir_all(&dir).unwrap();
}

/// "HELLO WORLD" and "LINE 2" in EBCDIC 037.
const EBCDIC_HELLO: &[u8] = &[0xC8, 0xC5, 0xD3, 0xD3, 0xD6, 0x40, 0xE6, 0xD6, 0xD9, 0xD3, 0xC4];
const EBCDIC_LINE2: &[u8] = &[0xD3, 0xC9, 0xD5, 0xC5, 0x40, 0xF2];

#[test]
fn ebcdic_reads_its_own_line_endings_and_comes_back_exactly() {
    for (ending, eol) in [(&[0x15][..], Eol::Nl), (&[0x25][..], Eol::Lf), (&[0x0D][..], Eol::Cr), (&[0x0D, 0x25][..], Eol::CrLf)] {
        let mut data = EBCDIC_HELLO.to_vec();
        data.extend_from_slice(ending);
        data.extend_from_slice(EBCDIC_LINE2);
        let mut format = format_of("ebcdic-037");
        let decoded = decode(&data, &mut format);
        assert_eq!(decoded.lines, vec!["HELLO WORLD".to_string(), "LINE 2".to_string()], "{eol:?}");
        assert_eq!(decoded.base, eol);
        assert_eq!(round_trip_as(&data, format_of("ebcdic-037")), data);
    }
    // ASCII's line feed byte is a control in EBCDIC, not a line ending.
    let mut format = format_of("ebcdic-037");
    assert_eq!(decode(&[0xC1, 0x0A, 0xC2], &mut format).lines.len(), 1);
    // Every byte survives in every EBCDIC page.
    let all: Vec<u8> = (0..=255).collect();
    for set in charset::all().iter().filter(|set| set.ebcdic) {
        assert_eq!(round_trip_as(&all, TextFormat { charset: set, ..TextFormat::default() }), all, "{}", set.id);
    }
}

#[test]
fn new_lines_in_ebcdic_are_written_in_ebcdic() {
    let format = format_of("ebcdic-037");
    let lines = ["AB".to_string(), "C".to_string(), String::new()];
    let saved = encode(&lines, &|i| if i == 0 { Eol::Nl } else { Eol::CrLf }, &format).unwrap();
    assert_eq!(saved, vec![0xC1, 0xC2, 0x15, 0xC3, 0x0D, 0x25]);
    assert_eq!(eol_bytes(Eol::Lf, format.charset), vec![0x25]);
    assert_eq!(eol_bytes(Eol::Nl, charset::utf8()), "\u{85}".as_bytes(), "outside EBCDIC it's Unicode's next line");
    let problems = encode(&["\u{263A}".to_string()], &|_| Eol::Nl, &format).unwrap_err();
    assert_eq!(problems.total, 1, "a character EBCDIC can't hold is reported, not dropped");
}

#[test]
fn ebcdic_text_is_suggested_and_ascii_text_never_is() {
    let mut data = Vec::new();
    for _ in 0..10 {
        data.extend_from_slice(EBCDIC_HELLO);
        data.push(0x15);
    }
    let found = detect(&data);
    assert_eq!(found.charset.id, "ebcdic-037");
    assert_eq!(found.certainty, Certainty::Suggested);
    assert!(!looks_like_ebcdic(&b"HELLO WORLD, this is ordinary ASCII text with spaces.\r\n".repeat(5)));
    assert!(!looks_like_ebcdic(&"caf\u{e9} na\u{ef}ve r\u{e9}sum\u{e9} ".repeat(20).into_bytes()));
    // Card images have no line endings at all, so whole 80-byte records are taken as rows.
    let mut cards = vec![0x40u8; 160];
    cards[..11].copy_from_slice(EBCDIC_HELLO);
    assert_eq!(ebcdic_rows(&cards), Some(NewlinePolicy::FixedRows { width: 80, parity: false }));
    assert_eq!(ebcdic_rows(&data), None);
    let mut format = TextFormat { newline: NewlinePolicy::FixedRows { width: 80, parity: false }, ..format_of("ebcdic-037") };
    let decoded = decode(&cards, &mut format);
    assert_eq!(decoded.lines.len(), 2);
    assert!(decoded.lines[0].starts_with("HELLO WORLD"));
    assert_eq!(encode(&decoded.lines, &|_| Eol::None, &format).unwrap(), cards);
}

#[test]
fn the_mainframe_presets_are_there() {
    let text = preset_by_id("ibm-mainframe").unwrap();
    assert_eq!((text.charset, text.eol), ("ebcdic-037", Eol::Nl));
    let records = preset_by_id("ibm-records").unwrap();
    assert_eq!(records.newline, NewlinePolicy::FixedRows { width: 80, parity: false });
    assert_eq!(NewlinePolicy::from_key(&records.newline.key()), Some(records.newline));
    let ansi = preset_by_id("pc-ansi").unwrap();
    assert_eq!((ansi.charset, ansi.display), ("cp437", Display::AnsiArt));
    assert_eq!(Display::from_key("ansi"), Some(Display::AnsiArt));
    assert_eq!(preset_for_extension(Path::new("ART.ANS")).map(|p| p.id), Some("pc-ansi"));
    assert_eq!(preset_for_extension(Path::new("menu.seq")).map(|p| p.id), Some("commodore"));
    assert!(preset_for_extension(Path::new("notes.txt")).is_none());
}

#[test]
fn east_asian_text_reads_as_characters_and_saves_back_exactly() {
    // "日本語" and "中文" and "한국어", each in its own set, with ASCII and a line ending.
    for (id, data, text) in [
        ("shift-jis", &b"\x93\xfa\x96\x7b\x8c\xea abc\r\nx"[..], "\u{65E5}\u{672C}\u{8A9E} abc"),
        ("euc-jp", b"\xc6\xfc\xcb\xdc\xb8\xec abc\r\nx", "\u{65E5}\u{672C}\u{8A9E} abc"),
        ("iso-2022-jp", b"\x1b$BF|K\\8l\x1b(B abc\r\nx", "\u{65E5}\u{672C}\u{8A9E} abc"),
        ("gbk", b"\xd6\xd0\xce\xc4 abc\r\nx", "\u{4E2D}\u{6587} abc"),
        ("gb18030", b"\xd6\xd0\xce\xc4 abc\r\nx", "\u{4E2D}\u{6587} abc"),
        ("big5", b"\xa4\xa4\xa4\xe5 abc\r\nx", "\u{4E2D}\u{6587} abc"),
        ("euc-kr", b"\xc7\xd1\xb1\xb9\xbe\xee abc\r\nx", "\u{D55C}\u{AD6D}\u{C5B4} abc"),
    ] {
        let mut format = format_of(id);
        let decoded = decode(data, &mut format);
        assert_eq!(decoded.lines, vec![text.to_string(), "x".to_string()], "{id}");
        assert_eq!(decoded.base, Eol::CrLf);
        assert_eq!(round_trip_as(data, format_of(id)), data, "{id}");
        assert!(!format.has_bom);
    }
}

#[test]
fn multi_byte_sets_keep_what_they_cant_read_and_report_what_they_cant_hold() {
    // A lead byte with nothing after it, a broken pair, and every byte value, all come back exactly.
    let all: Vec<u8> = (0..=255).collect();
    for id in ["shift-jis", "euc-jp", "iso-2022-jp", "gbk", "gb18030", "big5", "euc-kr"] {
        for data in [&b"abc\x93"[..], b"\x93\x0a\x96", b"\x1b$B\x01\x1b(B", &all] {
            assert_eq!(round_trip_as(data, format_of(id)), data, "{id} {data:?}");
        }
    }
    let mut format = format_of("shift-jis");
    let decoded = decode(b"a\x93", &mut format);
    assert_eq!(decoded.lines[0], format!("a{}", raw_char(0x93)), "a byte the set can't read is kept as itself");
    let problems = encode(&["\u{4E2D}\u{D55C}".to_string()], &|_| Eol::CrLf, &format_of("shift-jis")).unwrap_err();
    assert_eq!(problems.listed[0].ch, '\u{D55C}', "Korean isn't in Shift-JIS, and it says where");
    assert_eq!(problems.total, 1);
    assert!(fits('\u{65E5}', charset::known("shift-jis")) && !fits('\u{D55C}', charset::known("shift-jis")));
    assert!(fits('\u{D55C}', charset::known("gb18030")), "GB 18030 holds all of Unicode");
    assert_eq!(char_bytes('\u{65E5}', charset::known("iso-2022-jp"), 0), Some(b"\x1b$BF|\x1b(B".to_vec()), "a character in ISO-2022-JP comes with its escapes");
    assert_eq!(encode_with(&["\u{D55C}x".to_string()], &|_| Eol::CrLf, &format_of("shift-jis"), Lossy::Replace).unwrap(), b"?x");
    // Converting to a multi-byte set and back.
    let from = format_of("utf-8");
    let to = format_of("big5");
    let lines = vec!["\u{4E2D}\u{6587}".to_string()];
    assert_eq!(export(&lines, &|_| Eol::CrLf, &from, &to, Lossy::Refuse).unwrap(), b"\xa4\xa4\xa4\xe5");
}

#[test]
fn utf7_reads_and_writes_as_pythons_encoder_does() {
    // Python's own output for each, so files it (and mailers like it) wrote come back the same.
    for (text, bytes) in [("\u{e9} a", &b"+AOk a"[..]), ("\u{e9}-a", b"+AOk--a"), ("\u{e9}a", b"+AOk-a"), ("\u{e9}", b"+AOk-"), ("a+b", b"a+-b"), ("Hi! ~\\", b"Hi! +AH4AXA-"), ("\u{65e5}\u{672c}\u{8a9e}", b"+ZeVnLIqe-"), ("\u{1F600}", b"+2D3eAA-"), ("\u{e9}+", b"+AOkAKw-"), ("\u{e9}/", b"+AOk-/")] {
        let mut out = Vec::new();
        utf7_encode(text, true, false, &mut out);
        assert_eq!(out, bytes, "{text}");
        assert_eq!(utf7_decode(bytes).as_deref(), Some(text));
    }
    // In a file: a run before a line ending isn't closed, and every byte comes back exactly.
    let data = b"Caf+AOk\r\n+ZeVnLIqe\r\nplain";
    let mut format = format_of("utf-7");
    let decoded = decode(data, &mut format);
    assert_eq!(decoded.lines, vec!["Caf\u{e9}".to_string(), "\u{65e5}\u{672c}\u{8a9e}".to_string(), "plain".to_string()]);
    assert_eq!(round_trip_as(data, format_of("utf-7")), data);
    // A line another encoder wrote differently, or that isn't UTF-7 at all, stays as its bytes.
    for odd in [&b"+ZeVnLIqe-\r\nx"[..], b"\xe9\xff", b"+AOkA", b"+"] {
        assert_eq!(round_trip_as(odd, format_of("utf-7")), odd, "{odd:?}");
    }
    assert_eq!(encode(&["\u{e9}t\u{e9}".to_string()], &|_| Eol::CrLf, &format_of("utf-7")).unwrap(), b"+AOk-t+AOk-");
    // glibc's iconv puts the optional characters in base64; its own set reads and writes them so, and the
    // other set keeps such a line as its bytes, since it would write the ! as itself.
    let mut strict = format_of("utf-7-strict");
    assert_eq!(decode(b"Hi+ACE \r\nx", &mut strict).lines[0], "Hi! ");
    assert_eq!(round_trip_as(b"Hi+ACE \r\nx", format_of("utf-7-strict")), b"Hi+ACE \r\nx");
    assert_eq!(round_trip_as(b"Hi+ACE \r\nx", format_of("utf-7")), b"Hi+ACE \r\nx");
    let mut out = Vec::new();
    utf7_encode("Hi! ~", true, true, &mut out);
    assert_eq!(out, b"Hi+ACE +AH4-", "as iconv writes it");
}
