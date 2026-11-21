use super::*;
use crate::media::Volume;
use std::sync::Arc;

/// Each stretch of the signal in the Spectrum's T-states, rounded.
fn in_tstates(signal: &Signal) -> Vec<u32> {
    signal.lengths().iter().map(|s| (s * SPECTRUM_CLOCK).round() as u32).collect()
}

/// Reads a Spectrum tape back from its pulses as the ROM's LD-BYTES would: a long pilot, two sync pulses, then
/// pairs of pulses for bits. Gives each block's bytes and how many pilot pulses it had.
fn spectrum_blocks_heard(signal: &Signal) -> Vec<(u32, Vec<u8>)> {
    let t = in_tstates(signal);
    let near = |a: u32, b: u32| a.abs_diff(b) <= 2;
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if !near(t[i], 2168) {
            i += 1;
            continue;
        }
        let mut pilots = 0;
        while i < t.len() && near(t[i], 2168) {
            pilots += 1;
            i += 1;
        }
        assert!(near(t[i], 667) && near(t[i + 1], 735), "two sync pulses after the pilot");
        i += 2;
        let mut bits = Vec::new();
        while i + 1 < t.len() && t[i] == t[i + 1] && (near(t[i], 855) || near(t[i], 1710)) {
            bits.push(near(t[i], 1710));
            i += 2;
        }
        let bytes = bits.chunks(8).map(|c| c.iter().fold(0u8, |a, b| a << 1 | u8::from(*b))).collect();
        blocks.push((pilots, bytes));
    }
    blocks
}

fn tap_of(blocks: &[&[u8]]) -> Vec<u8> {
    let mut tap = Vec::new();
    for block in blocks {
        tap.extend_from_slice(&(block.len() as u16).to_le_bytes());
        tap.extend_from_slice(block);
    }
    tap
}

#[test]
fn a_spectrum_tap_plays_at_the_roms_timings_and_reads_back_the_same() {
    let program = crate::basic::tokenise_spectrum(&["10 PRINT \"HELLO\""], crate::basic::ZxDialect::Spectrum).unwrap();
    let tap = crate::basic::spectrum_tape(&program, "hello", Some(10));
    let made = make(&tap, "hello.tap").unwrap();
    assert_eq!(made.machine, "ZX Spectrum");
    let heard = spectrum_blocks_heard(&made.signal);
    let (blocks, _) = crate::basic::tape_blocks(&tap);
    assert_eq!(heard.len(), 2);
    assert_eq!(heard[0], (8063, blocks[0].to_vec()), "a header has the long pilot");
    assert_eq!(heard[1], (3223, blocks[1].to_vec()), "data has the short one");
    // A second's pause after each block, and the second's quiet at the end.
    let pauses: Vec<f64> = made.signal.lengths().into_iter().filter(|s| *s > 0.5).collect();
    assert_eq!(pauses.len(), 2);
    assert!(pauses.iter().all(|p| (p - 1.0).abs() < 0.01 || (p - 2.0).abs() < 0.01), "{pauses:?}");
}

#[test]
fn a_jupiter_ace_tape_is_not_taken_for_a_spectrums() {
    let mut header = vec![0u8; 25];
    header.push(header.iter().fold(0, |a, b| a ^ b));
    let tap = tap_of(&[&header, &[1, 2, 3, 0]]);
    assert!(make(&tap, "ace.tap").err().unwrap().contains("Jupiter Ace"));
    assert!(make(&[5, 0, 1], "broken.tap").is_err(), "a block that goes past the end");
}

/// A TZX from its blocks, each given with its ID first.
fn tzx_of(blocks: &[Vec<u8>]) -> Vec<u8> {
    let mut tzx = b"ZXTape!\x1A\x01\x14".to_vec();
    for block in blocks {
        tzx.extend_from_slice(block);
    }
    tzx
}

fn le16(values: &[u16]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn tzx_blocks_play_as_the_format_says() {
    let data = [0xFFu8, 0x81, 0x7E];
    let mut standard = vec![0x10];
    standard.extend(le16(&[250, 3]));
    standard.extend(data);
    // A turbo block: its own pilot, syncs and bits, and only the top 4 bits of its last byte.
    let mut turbo = vec![0x11];
    turbo.extend(le16(&[1000, 300, 400, 500, 900, 10]));
    turbo.push(4);
    turbo.extend(le16(&[0]));
    turbo.extend([2, 0, 0, 0xA5, 0xF0]);
    let tone = [vec![0x12], le16(&[700, 5])].concat();
    let pulses = [vec![0x13, 3], le16(&[100, 200, 300])].concat();
    let mut pure = vec![0x14];
    pure.extend(le16(&[600, 1200]));
    pure.push(8);
    pure.extend(le16(&[100]));
    pure.extend([1, 0, 0, 0x80]);
    let text = [vec![0x30, 5], b"hello".to_vec()].concat();
    let pause = [vec![0x20], le16(&[500])].concat();
    // A loop of two pure tones, three times.
    let looped = vec![[vec![0x24], le16(&[3])].concat(), [vec![0x12], le16(&[111, 2])].concat(), vec![0x25]];
    let csw = [vec![0x18], 4u32.to_le_bytes().to_vec(), vec![0; 4]].concat();
    let mut blocks = vec![standard, turbo, tone, pulses, pure, text, pause, csw];
    blocks.extend(looped);
    let made = make(&tzx_of(&blocks), "test.tzx").unwrap();
    let t = in_tstates(&made.signal);

    let heard = spectrum_blocks_heard(&made.signal);
    assert_eq!(heard[0], (3223, data.to_vec()), "a flag of &FF has the short pilot");
    // The 250 ms pause comes straight after the standard block's last bit.
    let at = 3223 + 2 + data.len() * 16;
    assert_eq!(t[at], (0.25 * SPECTRUM_CLOCK) as u32, "{:?}", &t[at - 2..at + 2]);
    // Then the turbo block.
    let turbo_at = at + 1;
    assert_eq!(&t[turbo_at..turbo_at + 12], &[1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 1000, 300, 400]);
    let bits: Vec<bool> = t[turbo_at + 12..turbo_at + 12 + 24].chunks(2).map(|pair| {
        assert_eq!(pair[0], pair[1]);
        pair[0] == 900
    }).collect();
    let expected: Vec<bool> = (0..8).map(|k| 0xA5 & (0x80 >> k) != 0).chain((0..4).map(|k| 0xF0 & (0x80 >> k) != 0)).collect();
    assert_eq!(bits, expected, "the last byte's 4 used bits, and no more");
    // With no pause, the tone follows at once, then the three pulses.
    let tone_at = turbo_at + 12 + 24;
    assert_eq!(&t[tone_at..tone_at + 8], &[700, 700, 700, 700, 700, 100, 200, 300]);
    // Pure data: one byte, &80, as a 1 and seven 0s. Its last pulse is ended by an edge and a millisecond of the
    // other level, then its pause and the pause block's run on together.
    let pure_at = tone_at + 8;
    assert_eq!(&t[pure_at..pure_at + 4], &[1200, 1200, 600, 600]);
    assert_eq!(&t[pure_at + 15..pure_at + 17], &[600, 3500]);
    let quiet = t[pure_at + 17];
    assert!(quiet.abs_diff((0.599 * SPECTRUM_CLOCK) as u32) < 4, "100 ms and 500 ms of quiet, {quiet}");
    // The loop played three times over, and the end's quiet after it.
    assert_eq!(&t[pure_at + 18..pure_at + 25], &[111, 111, 111, 111, 111, 111, 3500]);
    assert_eq!(made.notes.len(), 1, "{:?}", made.notes);
    assert!(made.notes[0].contains("&18"), "the CSW block is said to be left out");
}

#[test]
fn a_generalised_block_plays_its_symbols() {
    // Pilot: symbol 0 (an edge, then 1000) three times; data: two symbols of one bit each, 0 two pulses of 300,
    // 1 a forced-high pulse of 600 then 200.
    let mut body = Vec::new();
    body.extend(le16(&[0]));
    body.extend(1u32.to_le_bytes());
    body.extend([1, 1]);
    body.extend(3u32.to_le_bytes());
    body.extend([2, 2]);
    body.push(0);
    body.extend(le16(&[1000]));
    body.push(0);
    body.extend(le16(&[3]));
    body.push(0);
    body.extend(le16(&[300, 300]));
    body.push(3);
    body.extend(le16(&[600, 200]));
    body.push(0b1010_0000);
    let block = [vec![0x19], (body.len() as u32).to_le_bytes().to_vec(), body].concat();
    let made = make(&tzx_of(&[block]), "gen.tzx").unwrap();
    let t = in_tstates(&made.signal);
    // The symbols 1, 0, 1: the forced-high pulse after the pilot's last (high) one runs on from it.
    assert_eq!(&t[..2], &[1000, 1000]);
    assert_eq!(&t[2..7], &[1000 + 600, 200, 300, 300, 600]);
    assert!(made.notes.is_empty());
}

/// Reads a ZX81 or ZX80 tape back as its ROM would: the pulses in each bit counted, 9 a 1 and 4 a 0.
fn zx81_heard(signal: &Signal) -> Vec<u8> {
    let t = in_tstates(signal);
    let mut bits = Vec::new();
    let mut count = 0;
    for (k, length) in t.iter().enumerate() {
        if k % 2 == 1 {
            // A high stretch: one pulse (or the edge that ends the tape).
            if *length == 530 {
                count += 1;
            }
        } else if count > 0 && *length > 4000 {
            assert!(count == 4 || count == 9, "{count} pulses");
            bits.push(count == 9);
            count = 0;
        }
    }
    bits.chunks(8).map(|c| c.iter().fold(0u8, |a, b| a << 1 | u8::from(*b))).collect()
}

#[test]
fn a_zx81_program_goes_after_its_name_and_a_zx80_one_has_none() {
    let program = crate::zx81_program::program(&["10 PRINT \"HI\""]).unwrap();
    let made = make(&program, "games/Hi there.p").unwrap();
    assert_eq!(made.machine, "ZX81");
    let lengths = made.signal.lengths();
    assert!((lengths[0] - 5.0).abs() < 1e-6, "five seconds' quiet first, as SAVE waits");
    let heard = zx81_heard(&made.signal);
    // HITHERE in the ZX81's characters (H &2D, I &2E, T &39, E &2A, R &37), the last marked.
    let name = [0x2D, 0x2E, 0x39, 0x2D, 0x2A, 0x37, 0x2A | 0x80];
    assert_eq!(&heard[..7], &name);
    assert_eq!(&heard[7..], &program[..]);

    let zx80 = crate::zx80_program::program(&["10 PRINT \"HI\""]).unwrap();
    let made = make(&zx80, "hi.o").unwrap();
    assert_eq!(made.machine, "ZX80");
    assert_eq!(zx81_heard(&made.signal), zx80);
    assert_eq!(zx81_name("{}"), vec![0x39, 0x26, 0x35, 0x2A | 0x80], "a name with nothing the ZX81 has");
}

/// The lengths of a TAP file's pulses, in a C64's cycles, as a version 1 TAP holds them.
fn c64_tap_of(waves: impl Iterator<Item = f64>) -> Vec<u8> {
    let mut pulses = Vec::new();
    for seconds in waves {
        let cycles = (seconds * C64_PAL).round() as usize;
        if cycles / 8 > 255 {
            pulses.push(0);
            pulses.extend_from_slice(&(cycles as u32).to_le_bytes()[..3]);
        } else {
            pulses.push((cycles / 8) as u8);
        }
    }
    let mut tap = b"C64-TAPE-RAW".to_vec();
    tap.extend([1, 0, 0, 0]);
    tap.extend((pulses.len() as u32).to_le_bytes());
    tap.extend(pulses);
    tap
}

#[test]
fn a_commodore_program_is_saved_as_the_kernal_saves_it_and_reads_back_from_its_pulses() {
    let program = crate::basic::tokenise_commodore(&["10 PRINT \"HELLO\"", "20 GOTO 10"], crate::basic::CbmMachine::C64).unwrap();
    let made = make(&program, "Hello.prg").unwrap();
    assert_eq!(made.machine, "Commodore 64");
    // The waves as the signal has them, each a high and a low stretch, after the second of quiet.
    // (The millisecond high at the very end only finishes the last edge.)
    let waves: Vec<f64> = made.signal.pieces().iter().filter(|(s, high)| *high && *s < 0.0009).map(|(s, _)| s * 2.0).collect();
    assert!((made.signal.lengths()[0] - 1.0).abs() < 1e-9);
    assert!(waves.iter().all(|w| KERNAL_WAVES.iter().any(|k| (k - w).abs() < 1e-9)), "only the KERNAL's three waves");
    assert_eq!(waves.iter().take_while(|w| (*w - KERNAL_WAVES[0]).abs() < 1e-9).count(), 0x6A00, "the header's leader");
    // Read back by Limoniter's own reader of Commodore tapes, from the pulses alone.
    let tap = c64_tap_of(waves.iter().copied());
    let tape = crate::media::c64tap::C64Tape::open(Arc::new(tap)).unwrap();
    assert_eq!(tape.files.len(), 1, "{:?}", tape.files);
    let file = &tape.files[0];
    assert_eq!((file.name.as_str(), file.kind, file.start), ("HELLO", 1, 0x0801));
    assert!(file.ok);
    assert_eq!(file.data, &program[2..]);
    assert_eq!(usize::from(file.end), 0x0801 + program.len() - 2);

    let code = [0x00, 0xC0, 0xA9, 0x01, 0x60];
    let waves = kernal_waves(&code, "code").unwrap();
    let tap = c64_tap_of(waves.iter().map(|w| KERNAL_WAVES[usize::from(*w)]));
    let file = &crate::media::c64tap::C64Tape::open(Arc::new(tap)).unwrap().files[0];
    assert_eq!((file.kind, file.start, file.data.as_slice()), (3, 0xC000, &code[2..]), "machine code loads where it says");
    assert!(make(&[1, 8], "short.prg").is_err());
}

#[test]
fn a_commodore_tap_plays_its_pulses_at_its_machines_clock() {
    let mut tap = b"C64-TAPE-RAW".to_vec();
    tap.extend([1, 0, 0, 0]);
    tap.extend(5u32.to_le_bytes());
    tap.extend([0x30, 0, 0x10, 0x27, 0x00]);
    let made = make(&tap, "game.tap").unwrap();
    assert_eq!(made.machine, "Commodore 64");
    let lengths = made.signal.lengths();
    assert!((lengths[0] + lengths[1] - 0x30 as f64 * 8.0 / C64_PAL).abs() < 1e-9);
    assert!((lengths[2] * 2.0 - 10_000.0 / C64_PAL).abs() < 1e-9, "a long pulse from its three bytes");
    // A VIC-20's, NTSC, at its own clock; and version 2's halves of waves.
    let mut vic = tap.clone();
    (vic[12], vic[13], vic[14]) = (2, 1, 1);
    let made = make(&vic, "vic.tap").unwrap();
    assert_eq!(made.machine, "VIC-20");
    assert!((made.signal.lengths()[0] - 0x30 as f64 * 8.0 / 1_022_727.0).abs() < 1e-9);
}

/// Reads an Acorn or MSX tape's bits back from its waves: a wave of 1200 Hz a 0, two of 2400 Hz a 1, with None
/// for a gap of quiet. Each wave is timed by its high half, as quiet after it lengthens the low. Gives the bits,
/// and the length in waves of every run of more than 100 waves of 2400 Hz (a carrier tone, with any 1 bits that
/// come just before it).
fn waves_heard(signal: &Signal) -> (Vec<Option<bool>>, Vec<usize>) {
    let fast = |w: f64| (w - 1.0 / 2400.0).abs() < 1e-6;
    let slow = |w: f64| (w - 1.0 / 1200.0).abs() < 1e-6;
    let mut waves = Vec::new();
    for (seconds, high) in signal.pieces() {
        if *high {
            waves.push(Some(seconds * 2.0));
        } else if *seconds > 0.01 {
            waves.push(None);
        }
    }
    let (mut bits, mut tones, mut run) = (Vec::new(), Vec::new(), 0);
    let mut i = 0;
    while i < waves.len() {
        let now = waves[i];
        if now.is_some_and(fast) {
            run += 1;
        } else {
            if run > 100 {
                tones.push(run);
            }
            run = 0;
        }
        match now {
            None => {
                bits.push(None);
                i += 1;
            }
            Some(w) if slow(w) => {
                bits.push(Some(false));
                i += 1;
            }
            Some(w) if fast(w) && waves.get(i + 1).copied().flatten().is_some_and(fast) => {
                bits.push(Some(true));
                run += 1;
                i += 2;
            }
            _ => i += 1,
        }
    }
    if run > 100 {
        tones.push(run);
    }
    (bits, tones)
}

/// Bytes from bits, as a UART reads them: a 0 start bit, eight bits lowest first, then `stops` 1 stop bits.
fn bytes_framed(bits: &[Option<bool>], stops: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut k = 0;
    while k < bits.len() {
        if bits[k] != Some(false) {
            k += 1;
            continue;
        }
        let Some(frame) = bits.get(k + 1..k + 9 + stops) else { break };
        if frame.iter().any(Option::is_none) || frame[8..].iter().any(|b| *b != Some(true)) {
            k += 1;
            continue;
        }
        bytes.push(frame[..8].iter().rev().fold(0u8, |a, b| a << 1 | u8::from(b.unwrap_or(false))));
        k += 9 + stops;
    }
    bytes
}

fn kansas_city_heard(signal: &Signal, stops: usize) -> (Vec<u8>, Vec<usize>) {
    let (bits, tones) = waves_heard(signal);
    (bytes_framed(&bits, stops), tones)
}

#[test]
fn a_bbc_basic_program_is_saved_in_the_cassette_filing_systems_blocks() {
    let lines: Vec<String> = (1..=40).map(|n| format!("{} PRINT \"LINE NUMBER {n} OF THE PROGRAM\"", n * 10)).collect();
    let program = crate::basic_more::tokenise_bbc(&lines).unwrap();
    assert!(program.len() > 256, "long enough for two blocks");
    let made = make(&program, "my prog.bbc").unwrap();
    assert_eq!(made.machine, "BBC Micro or Electron");
    let (heard, tones) = kansas_city_heard(&made.signal, 1);
    // 5.1 seconds of carrier first, then 0.9 between blocks and after the last (each run heard with the stop bit
    // and any 1 bits just before it, two waves to a bit).
    let blocks = cfs_blocks("myprog", &program, 0xFFFF_0E00, 0xFFFF_8023);
    assert_eq!(blocks.len(), program.len().div_ceil(256));
    assert_eq!(tones.len(), blocks.len() + 1);
    assert_eq!(tones[0], 12240);
    assert!(tones[1..].iter().all(|t| (2162..=2178).contains(t) && t % 2 == 0), "{tones:?}");
    assert_eq!(heard, blocks.concat());
    // Read back by Limoniter's own UEF reader, from the bytes alone.
    let mut uef = b"UEF File!\0\x0A\x00".to_vec();
    uef.extend(0x0100u16.to_le_bytes());
    uef.extend((heard.len() as u32).to_le_bytes());
    uef.extend(&heard);
    let tape = crate::media::acorn::Uef::open(Arc::new(uef.clone())).unwrap();
    let files = tape.list(None).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "myprog");
    assert_eq!(tape.read(&files[0]).unwrap(), program);
    // And that UEF played back makes the same bytes.
    let (again, _) = kansas_city_heard(&make(&uef, "x.uef").unwrap().signal, 1);
    assert_eq!(again, heard);
    assert!(make(b"10 PRINT", "text.bbc").is_err(), "a listing as text isn't a program");
}

#[test]
fn uef_chunks_play_their_tones_gaps_and_bits() {
    let chunk = |id: u16, body: &[u8]| [id.to_le_bytes().to_vec(), (body.len() as u32).to_le_bytes().to_vec(), body.to_vec()].concat();
    let mut uef = b"UEF File!\0\x0A\x00".to_vec();
    uef.extend(chunk(0x0000, b"made by hand\0"));
    uef.extend(chunk(0x0110, &le16(&[300])));
    uef.extend(chunk(0x0100, &[0x5A]));
    uef.extend(chunk(0x0116, &0.25f32.to_le_bytes()));
    // Seven bits, 1011001, lowest first.
    uef.extend(chunk(0x0102, &[1, 0b0100_1101]));
    uef.extend(chunk(0x0104, &[7, b'E', 2, 0x41]));
    uef.extend(chunk(0x0199, &[]));
    let made = make(&uef, "t.uef").unwrap();
    let lengths = made.signal.lengths();
    assert_eq!(lengths.iter().take(600).filter(|l| (*l - 1.0 / 4800.0).abs() < 1e-9).count(), 600, "300 waves of carrier");
    assert!(made.signal.pieces().windows(2).any(|w| w[0] == (0.001, true) && (w[1].0 - 0.249).abs() < 1e-9 && !w[1].1), "the quarter-second gap after the byte's stop bit");
    let (bits, tones) = waves_heard(&made.signal);
    assert_eq!(tones, vec![300]);
    assert_eq!(bytes_framed(&bits, 1)[0], 0x5A);
    // After the gap: the seven bits, then &41 in seven bits with even parity and two stop bits.
    let tail: Vec<bool> = bits.iter().rev().skip_while(|b| b.is_none()).take_while(|b| b.is_some()).flatten().copied().collect::<Vec<_>>().into_iter().rev().collect();
    let expected = [[true, false, true, true, false, false, true].as_slice(), &[false, true, false, false, false, false, false, true, false, true, true]].concat();
    assert_eq!(tail, expected);
    assert_eq!(made.notes.len(), 1, "the unknown chunk is said");
    // Gzipped, as UEF files often are.
    let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0xFF];
    gz.extend(miniz_oxide::deflate::compress_to_vec(&uef, 6));
    gz.extend(crc32(&uef).to_le_bytes());
    gz.extend((uef.len() as u32).to_le_bytes());
    assert_eq!(make(&gz, "t.uef").unwrap().signal.lengths(), lengths);
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, b| (0..8).fold(crc ^ u32::from(*b), |c, _| if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 }))
}

#[test]
fn an_msx_cassette_has_the_bioss_long_tone_before_a_header_and_short_ones_after() {
    let mut cas = MSX_MARK.to_vec();
    cas.extend([0xD3; 10]);
    cas.extend(b"GAME  ");
    cas.extend(MSX_MARK);
    cas.extend([0x00, 0x80, 0x12, 0x34, 0x00, 0x00, 0x00, 0x00]);
    let made = make(&cas, "game.cas").unwrap();
    assert_eq!(made.machine, "MSX");
    let (bytes, tones) = kansas_city_heard(&made.signal, 2);
    assert_eq!(tones, vec![15360, 3840]);
    assert_eq!(bytes, [&[0xD3; 10][..], b"GAME  ", &[0x00, 0x80, 0x12, 0x34, 0, 0, 0, 0]].concat());
    assert!((made.signal.lengths()[0] - 2.0).abs() < 1e-9, "two seconds' quiet before a file");
    assert!(make(b"\x55\x55\x55\x3C", "coco.cas").is_err(), "other machines' .cas files aren't MSX ones");
}

#[test]
fn the_wav_is_a_square_wave_with_each_edge_where_it_falls_between_samples() {
    let mut signal = Signal::default();
    signal.hold(1.5 / f64::from(RATE), true);
    signal.hold(2.5 / f64::from(RATE), false);
    let wav = signal.wav();
    assert_eq!(&wav[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), RATE);
    assert_eq!(&wav[44..], &[228, 128, 28, 28], "the second sample is half high, half low");
    let info = crate::sound::info(&make(&tap_of(&[&[0xFF, 1, 0xFE]]), "x.tap").unwrap().signal.wav(), "x.wav").unwrap();
    let seconds = info.seconds.unwrap();
    // Two seconds of pilot, the bits, a second's pause and a second's quiet at the end.
    assert!((4.0..4.1).contains(&seconds), "it plays as a sound, for {seconds} seconds");
}

#[test]
fn files_it_cant_make_tapes_of_say_why() {
    assert!(may_make("game.TZX") && may_make("x.p") && may_make("a.prg") && !may_make("notes.txt"));
    assert!(make(b"hello", "notes.txt").is_err());
    assert!(make(b"ZXTape!\x1A\x01\x14\x10\x00", "cut.tzx").is_err(), "a block cut short");
    assert_eq!(make(b"ZXTape!\x1A\x01\x14\x30\x02hi", "text.tzx").err().unwrap(), "There's nothing on this tape to make a sound of.");
    assert_eq!(make(b"ZXTape!\x1A\x01\x14\x10\x00\x00\x01\x00\xAA", "cpc.cdt").unwrap().machine, "Amstrad CPC");
}

/// Not a test: writes each file named in LIMONITER_TAPE_SOUND (separated by commas) as WAV beside it, to compare
/// with other programs' tape sound.
#[test]
#[ignore = "run by hand to compare with other tools"]
fn write_wavs_for_comparing() {
    for path in std::env::var("LIMONITER_TAPE_SOUND").unwrap_or_default().split(',').filter(|p| !p.is_empty()) {
        let bytes = std::fs::read(path).unwrap();
        let made = make(&bytes, path).unwrap();
        std::fs::write(format!("{path}.limoniter.wav"), made.signal.wav()).unwrap();
        println!("{path}: {} for {:.1} s {:?}", made.machine, made.signal.seconds(), made.notes);
    }
}
