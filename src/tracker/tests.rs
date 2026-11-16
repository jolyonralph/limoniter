use super::play::Player;
use super::*;
use std::sync::Arc;

/// A 4-channel ProTracker module with one looping square-wave sample of 64 bytes, and the given rows in pattern
/// 0 (and 1): (pattern, row, channel, period, sample, effect, parameter).
fn module(order: &[u8], cells: &[(usize, usize, usize, u16, u8, u8, u8)]) -> Vec<u8> {
    let mut bytes = crate::protracker::tests::synthetic("square", &[("square", 32, 64, 0, 0, 32)], order, b"M.K.", 4);
    for &(pattern, row, channel, period, sample, effect, param) in cells {
        let at = 1084 + pattern * 1024 + row * 16 + channel * 4;
        bytes[at] = (sample & 0xF0) | (period >> 8) as u8;
        bytes[at + 1] = period as u8;
        bytes[at + 2] = ((sample & 0x0F) << 4) | effect;
        bytes[at + 3] = param;
    }
    let data = bytes.len() - 64;
    for (i, b) in bytes[data..].iter_mut().enumerate() {
        *b = if i < 32 { 0x60 } else { 0xA0 };
    }
    bytes
}

fn render(song: Song, seconds: f64) -> Vec<f32> {
    let mut player = Player::new(Arc::new(song), 48_000);
    let mut out = vec![0.0; (48_000.0 * seconds) as usize * 2];
    player.render(&mut out);
    out
}

/// Times the left side goes from below zero to above it.
fn rises(out: &[f32]) -> usize {
    out.as_chunks::<2>().0.iter().map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
}

#[test]
fn a_note_plays_at_its_pitch() {
    // ProTracker's C-2, period 428, plays 8363 samples a second: a 64-sample square is 130.7 Hz.
    let song = load(&module(&[0], &[(0, 0, 0, 428, 1, 0, 0)]), "x.mod").unwrap();
    assert_eq!(song.cell(&song.patterns[0], 0, 0).note, 49);
    let out = render(song, 1.0);
    assert!((129..=132).contains(&rises(&out)), "{}", rises(&out));
    // The first channel is on the left.
    let (left, right): (f32, f32) = out.as_chunks::<2>().0.iter().fold((0.0, 0.0), |(l, r), f| (l + f[0].abs(), r + f[1].abs()));
    assert!(left > right * 1.5);
    // An octave up is twice as fast.
    let out = render(load(&module(&[0], &[(0, 0, 0, 214, 1, 0, 0)]), "x.mod").unwrap(), 1.0);
    assert!((259..=264).contains(&rises(&out)), "{}", rises(&out));
}

#[test]
fn a_song_lasts_as_long_as_its_rows_and_speed_say() {
    let length = |cells: &[(usize, usize, usize, u16, u8, u8, u8)], order: &[u8]| Player::duration(&Arc::new(load(&module(order, cells), "x.mod").unwrap())).unwrap();
    // 64 rows of 6 ticks at 125 BPM (50 ticks a second).
    assert!((length(&[], &[0]) - 7.68).abs() < 0.01);
    assert!((length(&[(0, 0, 0, 0, 0, 0xF, 3)], &[0]) - 3.84).abs() < 0.01, "speed 3");
    assert!((length(&[(0, 0, 0, 0, 0, 0xF, 250)], &[0]) - 3.84).abs() < 0.01, "250 BPM");
    assert!((length(&[(0, 15, 2, 0, 0, 0xD, 0)], &[0, 0]) - 1.92 * 2.0).abs() < 0.01, "a break after 16 rows, twice");
    // A jump back to the start ends the song, after the row with the jump.
    assert!((length(&[(1, 0, 0, 0, 0, 0xB, 0)], &[0, 1]) - 7.8).abs() < 0.01);
    // A pattern loop plays rows 0 to 3 three times; a pattern delay holds a row for 4 rows' time.
    assert!((length(&[(0, 3, 0, 0, 0, 0xE, 0x62)], &[0]) - 7.68 - 8.0 * 0.12).abs() < 0.01);
    assert!((length(&[(0, 5, 0, 0, 0, 0xE, 0xE3)], &[0]) - 7.68 - 3.0 * 0.12).abs() < 0.01);
}

#[test]
fn volume_and_cuts_are_heard() {
    let loud = render(load(&module(&[0], &[(0, 0, 0, 428, 1, 0, 0)]), "x.mod").unwrap(), 0.5);
    let quiet = render(load(&module(&[0], &[(0, 0, 0, 428, 1, 0xC, 16)]), "x.mod").unwrap(), 0.5);
    let level = |out: &[f32]| out[4800..].iter().map(|s| s.abs()).sum::<f32>();
    assert!((level(&loud) / level(&quiet) - 4.0).abs() < 0.2, "{}", level(&loud) / level(&quiet));
    let cut = render(load(&module(&[0], &[(0, 0, 0, 428, 1, 0xE, 0xC2)]), "x.mod").unwrap(), 0.5);
    assert!(cut[48_000..].iter().all(|s| *s == 0.0), "cut after two ticks");
    // A slide up raises the pitch as it goes.
    let slid = render(load(&module(&[0], &[(0, 0, 0, 428, 1, 1, 8), (0, 1, 0, 0, 0, 1, 8)]), "x.mod").unwrap(), 0.24);
    assert!(rises(&slid[..11_520]) < rises(&slid[11_520..]));
}

#[test]
fn the_pc_trackers_are_told_by_their_marks() {
    assert_eq!(pc_format(b"Extended Module: tune"), Some(Format::Xm));
    assert_eq!(pc_format(b"IMPMtune"), Some(Format::It));
    let mut s3m = vec![0u8; 0x60];
    s3m[44..48].copy_from_slice(b"SCRM");
    assert_eq!(pc_format(&s3m), Some(Format::S3m));
    assert_eq!(pc_format(b"M.K."), None);
    assert!(named_as_pc_module("C:\\music\\2ND_PM.S3M") && named_as_pc_module("a.it") && !named_as_pc_module("a.txt"));
}

#[test]
fn a_scream_tracker_module_is_read_and_played() {
    // The header, one 16-bit sample of a square at 8363 Hz, and a pattern playing C-4 on channel 1.
    let mut b = vec![0u8; 0x60];
    b[..4].copy_from_slice(b"Test");
    b[0x1C] = 0x1A;
    b[0x1D] = 16;
    b[0x20] = 2; // orders
    b[0x22] = 1; // instruments
    b[0x24] = 1; // patterns
    b[0x28..0x2A].copy_from_slice(&0x1320u16.to_le_bytes());
    b[0x2A] = 2; // unsigned samples
    b[44..48].copy_from_slice(b"SCRM");
    b[0x30] = 64;
    b[0x31] = 6;
    b[0x32] = 125;
    b[0x33] = 0xB0;
    b[0x40..0x60].fill(255);
    b[0x40] = 0;
    b[0x41] = 8;
    b.extend_from_slice(&[0, 255]); // order
    // Parapointers: instrument at paragraph 7 (0x70), pattern at 12 (0xC0).
    b.extend_from_slice(&7u16.to_le_bytes());
    b.extend_from_slice(&12u16.to_le_bytes());
    b.resize(0x70, 0);
    let mut instrument = vec![0u8; 0x50];
    instrument[0] = 1;
    instrument[0x0E..0x10].copy_from_slice(&16u16.to_le_bytes()); // data at 0x100
    instrument[0x10..0x14].copy_from_slice(&64u32.to_le_bytes());
    instrument[0x14..0x18].copy_from_slice(&0u32.to_le_bytes());
    instrument[0x18..0x1C].copy_from_slice(&64u32.to_le_bytes());
    instrument[0x1C] = 64;
    instrument[0x1F] = 1 | 4;
    instrument[0x20..0x24].copy_from_slice(&8363u32.to_le_bytes());
    instrument[0x30..0x34].copy_from_slice(b"sqr!");
    instrument[0x4C..0x50].copy_from_slice(b"SCRS");
    b.extend_from_slice(&instrument);
    b.resize(0xC0, 0);
    // Channel 0 with a note and instrument (0x20): C-4 (0x40), instrument 1; then the ends of 64 rows.
    let rows: Vec<u8> = std::iter::once(vec![0x20, 0x40, 1, 0]).chain(std::iter::repeat_n(vec![0], 64)).flatten().collect();
    b.extend_from_slice(&((rows.len() + 2) as u16).to_le_bytes());
    b.extend_from_slice(&rows);
    b.resize(0x100, 0);
    for i in 0..64u16 {
        b.extend_from_slice(&(if i < 32 { 0xC000u16 } else { 0x4000 }).to_le_bytes());
    }
    let song = load(&b, "t.s3m").unwrap();
    assert_eq!((song.format, song.title.as_str(), song.channels, song.tracker.as_str()), (Format::S3m, "Test", 2, "Scream Tracker 3.20"));
    assert_eq!(song.pans, [51, 204]);
    assert_eq!(song.samples[0].name, "sqr!");
    assert_eq!((song.samples[0].data.len(), song.samples[0].loop_kind, song.samples[0].bits), (64, Loop::Forward, 16));
    assert_eq!(song.note_name(song.cell(&song.patterns[0], 0, 0).note), "C-4");
    let out = render(song, 1.0);
    assert!((129..=132).contains(&rises(&out)), "{}", rises(&out));
}

/// Modules from the Mod Archive in ../testmedia/trackers, if they're there, played through and timed against
/// libopenmpt's own lengths (from ffprobe).
#[test]
fn real_modules_last_as_long_as_libopenmpt_says() {
    let folder = std::path::Path::new("../testmedia/trackers");
    if !folder.exists() {
        return;
    }
    let lengths = [
        ("space_debris.mod", 305.94),
        ("2nd_pm.s3m", 404.81),
        ("external.xm", 140.30),
        ("cgi-bass.it", 169.0),
    ];
    for (name, seconds) in lengths {
        let bytes = std::fs::read(folder.join(name)).unwrap();
        let song = Arc::new(load(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}")));
        let length = Player::duration(&song).unwrap_or(0.0);
        assert!((length - seconds).abs() < 1.0, "{name}: {length:.2} seconds, libopenmpt says {seconds}");
    }
}

/// Every module in ../testmedia/trackers timed, and its first 30 seconds written as a WAV to the folder named by
/// LIMONITER_RENDER, to set beside libopenmpt's: `cargo test --release -- --ignored render_every_module`.
#[test]
#[ignore]
fn render_every_module() {
    let folder = std::path::Path::new("../testmedia/trackers");
    let out_dir = std::env::var_os("LIMONITER_RENDER").map(std::path::PathBuf::from);
    let mut names: Vec<_> = std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()).collect();
    names.sort();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let song = match load(&bytes, &name) {
            Ok(song) => Arc::new(song),
            Err(e) => {
                println!("{name}\tERROR {e}");
                continue;
            }
        };
        println!("{name}\t{:.2}\t{}\t{}", Player::duration(&song).unwrap_or(-1.0), song.channels, song.instruments.len());
        if let Some(dir) = &out_dir {
            let mut player = Player::new(song, 44_100);
            let mut out = vec![0.0f32; 44_100 * 30 * 2];
            player.render(&mut out);
            let mut wav = Vec::new();
            let data = (out.len() * 2) as u32;
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&[16, 0, 0, 0, 1, 0, 2, 0]);
            wav.extend_from_slice(&44_100u32.to_le_bytes());
            wav.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
            wav.extend_from_slice(&[4, 0, 16, 0]);
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data.to_le_bytes());
            for s in out {
                wav.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
            }
            std::fs::write(dir.join(format!("{name}.wav")), wav).unwrap();
        }
    }
}

/// The rows played between two times in a module, for looking into a difference:
/// LIMONITER_TRACE="file from to" cargo test --release -- --ignored trace_rows --nocapture
#[test]
#[ignore]
fn trace_rows() {
    let spec = std::env::var("LIMONITER_TRACE").unwrap();
    let parts: Vec<&str> = spec.split(' ').collect();
    let (file, from, to): (&str, f64, f64) = (parts[0], parts[1].parse().unwrap(), parts[2].parse().unwrap());
    let bytes = std::fs::read(std::path::Path::new("../testmedia/trackers").join(file)).unwrap();
    let song = Arc::new(load(&bytes, file).unwrap());
    let mut player = Player::new(song.clone(), 44_100);
    let mut out = vec![0.0f32; 2 * 441];
    let mut last = None;
    while player.time < to && !player.ended {
        player.render(&mut out);
        let place = player.place();
        if player.time >= from && last != Some(place) {
            last = Some(place);
            let pattern = song.pattern(place.pattern).unwrap();
            let cells: Vec<String> = (0..song.channels).filter(|&c| song.cell(pattern, place.row, c) != Cell::default()).map(|c| format!("{c}: {}", song.cell_text(&song.cell(pattern, place.row, c)))).collect();
            let level = out.iter().map(|s| s.abs()).sum::<f32>() / out.len() as f32;
            println!("{:7.2} {:3} {:02X} {:3} {level:.3} | {}", player.time, place.position, place.pattern, place.row, cells.join(" | "));
        }
    }
}
