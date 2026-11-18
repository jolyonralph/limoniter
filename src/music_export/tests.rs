use super::*;

fn testmedia(path: &str) -> Option<Vec<u8>> {
    std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../testmedia").join(path)).ok()
}

#[test]
fn a_module_is_saved_as_a_wav_as_long_as_it_plays() {
    let Some(module) = testmedia("trackers/space_debris.mod") else { return };
    let frames = render(&module, "space_debris.mod", &[]).unwrap();
    let song = Arc::new(crate::tracker::load(&module, "space_debris.mod").unwrap());
    let seconds = Player::duration(&song).unwrap();
    let rendered = frames.len() as f64 / 2.0 / f64::from(RATE);
    assert!((rendered - seconds).abs() < 0.1, "{rendered} against {seconds}");
    assert!(frames.iter().any(|&s| s.abs() > 0.1));
    let file = wav(&frames, RATE);
    // Read back as any WAV is: the same length, and the same sound to 16 bits.
    let info = crate::sound::info(&file, "out.wav").unwrap();
    assert_eq!((info.rate, info.channels, info.bits), (RATE, 2, Some(16)));
    assert!((info.seconds.unwrap() - rendered).abs() < 0.01);
    let mut back = crate::sound::SoundPlayer::new(file, "out.wav", RATE).unwrap();
    let mut out = vec![0f32; 2 * 10_000];
    back.render(&mut out);
    for (a, b) in out.iter().zip(&frames) {
        assert!((a - b.clamp(-1.0, 1.0)).abs() < 1.0 / 16_000.0, "{a} {b}");
    }
}

#[test]
fn channels_switched_off_are_left_out() {
    let Some(module) = testmedia("trackers/space_debris.mod") else { return };
    let all = render(&module, "space_debris.mod", &[]).unwrap();
    let none = render(&module, "space_debris.mod", &[true; 4]).unwrap();
    assert!(all.iter().any(|&s| s.abs() > 0.1));
    assert!(none.iter().all(|&s| s == 0.0));
}

#[test]
fn a_sound_file_is_saved_as_a_wav_again() {
    let tone: Vec<f32> = (0..RATE as usize).flat_map(|i| {
        let v = (i as f32 * 440.0 / RATE as f32 * std::f32::consts::TAU).sin() * 0.5;
        [v, -v]
    }).collect();
    let file = wav(&tone, RATE);
    let again = render(&file, "tone.wav", &[]).unwrap();
    assert!(again.len().abs_diff(tone.len()) <= 4, "{} {}", again.len(), tone.len());
    for (a, b) in again.iter().zip(&tone).skip(2) {
        assert!((a - b).abs() < 0.001);
    }
}

#[test]
fn a_module_s_samples_come_out_as_8svx_with_their_loops() {
    let Some(module) = testmedia("trackers/space_debris.mod") else { return };
    let song = crate::tracker::load(&module, "space_debris.mod").unwrap();
    let files = samples(&module, "space_debris.mod").unwrap();
    assert_eq!(files.len(), song.samples.iter().filter(|s| !s.data.is_empty()).count());
    assert!(files.iter().all(|(name, _)| name.ends_with(".8svx")));
    for ((_, bytes), sample) in files.iter().zip(song.samples.iter().filter(|s| !s.data.is_empty())) {
        let pcm = crate::sound::read_8svx(bytes).unwrap();
        assert_eq!(pcm.rate, 8363);
        assert_eq!(pcm.samples.len(), sample.data.len());
        for (a, &b) in pcm.samples.iter().zip(&sample.data) {
            assert_eq!(*a, f32::from((b >> 8) as i8) / 128.0);
        }
        // The loop: the one-shot part, then the part repeated.
        let one_shot = u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as usize;
        let repeat = u32::from_be_bytes(bytes[24..28].try_into().unwrap()) as usize;
        if sample.loop_kind == Loop::None {
            assert_eq!((one_shot, repeat), (sample.data.len(), 0));
        } else {
            assert_eq!((one_shot, one_shot + repeat), (sample.loop_start, sample.loop_end));
        }
        if !sample.name.trim().is_empty() {
            assert!(pcm.info.tags.iter().any(|(k, v)| k == "Title" && v == sample.name.trim()));
        }
    }
    // And one opens and plays as a sound of its own.
    assert!(crate::sound::SoundPlayer::new(files[0].1.clone(), &files[0].0, RATE).is_ok());
}

