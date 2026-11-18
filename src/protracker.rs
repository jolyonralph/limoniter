//! ProTracker modules (.mod, or mod.name as they were called on the Amiga), read for what they say about
//! themselves: the title, the samples with their names, lengths and loops, the song's order and how many patterns
//! it has. Only read: nothing here plays or changes a module. Soundtracker's older 15-sample modules, which have
//! no tag, are read too, and the many-channel tags of the trackers that followed (6CHN, 8CHN, FLT8, 16CH...).

use crate::media::InfoSection;
use crate::preview::{Block, BlockKind, Span};

/// One of a module's sample slots.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    pub name: String,
    /// In bytes (the file keeps it in words).
    pub length: u32,
    /// -8 to 7, in eighths of a semitone.
    pub finetune: i8,
    /// 0 to 64.
    pub volume: u8,
    /// Where the loop starts and how long it is, in bytes. A loop of 2 bytes or less means it doesn't loop.
    pub loop_start: u32,
    pub loop_length: u32,
}

impl Sample {
    pub fn loops(&self) -> bool {
        self.loop_length > 2
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Module {
    pub title: String,
    /// The four letters at 1080, such as M.K.; empty for a 15-sample Soundtracker module.
    pub tag: String,
    /// Which tracker the tag says made it.
    pub format: String,
    pub channels: usize,
    /// All the slots: 31, or 15 for Soundtracker's.
    pub samples: Vec<Sample>,
    /// The positions the song plays, each a pattern number.
    pub order: Vec<u8>,
    /// The byte after the song length: where NoiseTracker restarts, 127 from ProTracker, or Soundtracker's tempo.
    pub restart: u8,
    pub patterns: usize,
    /// How long the file should be, from what it says, and how long it is.
    pub expected_size: u64,
    pub size: u64,
    /// Anything odd about it, said plainly.
    pub notes: Vec<String>,
}

const HEADER_31: usize = 1084;
const HEADER_15: usize = 600;

/// The channels a 31-sample module's tag says it has, and the tracker it names.
fn tag_channels(tag: &[u8]) -> Option<(usize, &'static str)> {
    let digit = |b: u8| b.is_ascii_digit().then(|| (b - b'0') as usize);
    match tag {
        b"M.K." => Some((4, "ProTracker")),
        b"M!K!" => Some((4, "ProTracker, with over 64 patterns")),
        b"M&K!" | b"N.T." => Some((4, "NoiseTracker")),
        b"FLT4" => Some((4, "StarTrekker")),
        b"FLT8" => Some((8, "StarTrekker, 8 channels")),
        b"CD81" | b"OKTA" | b"OCTA" => Some((8, "Oktalyzer or Falcon, 8 channels")),
        [n, b'C', b'H', b'N'] => digit(*n).filter(|n| *n > 0).map(|n| (n, "FastTracker")),
        [a, b, b'C', b'H'] | [a, b, b'C', b'N'] => match (digit(*a), digit(*b)) {
            (Some(a), Some(b)) if (1..=99).contains(&(a * 10 + b)) => Some((a * 10 + b, "FastTracker or TakeTracker")),
            _ => None,
        },
        [b'T', b'D', b'Z', n] => digit(*n).filter(|n| *n > 0).map(|n| (n, "TakeTracker")),
        _ => None,
    }
}

/// Text from a module's fixed-length fields: up to the first zero, with anything unprintable shown as a space.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| if (0x20..0x7F).contains(&b) || b >= 0xA0 { b as char } else { ' ' }).collect::<String>().trim_end().to_string()
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]))
}

/// Whether the bytes are a module with a tag Limoniter knows, which is all a 31-sample module needs to be told.
pub fn has_module_tag(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER_31 && tag_channels(&bytes[1080..1084]).is_some()
}

/// Whether a file's name is one a module goes by: name.mod, or mod.name as the Amiga had it.
pub fn named_as_module(name: &str) -> bool {
    let lower = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    lower.ends_with(".mod") || (lower.starts_with("mod.") && lower.len() > 4)
}

/// Reads a module. `name` lets a 15-sample module, which has no tag, be taken for one when its name says it is.
pub fn read(bytes: &[u8], name: &str) -> Result<Module, String> {
    if has_module_tag(bytes) {
        return read_with(bytes, 31);
    }
    if named_as_module(name) && bytes.len() >= HEADER_15 {
        let module = read_with(bytes, 15)?;
        if plausible_15(bytes, &module) {
            return Ok(module);
        }
    }
    Err(crate::i18n::tr("This isn't a ProTracker module: there's no tag such as M.K. at byte 1080, and it isn't laid out as an older 15-sample Soundtracker module either.").into())
}

/// Whether bytes read as a 15-sample module make sense: printable names, volumes up to 64, a song of 1 to 128
/// positions, and patterns that fit the file.
fn plausible_15(bytes: &[u8], module: &Module) -> bool {
    let names_ok = std::iter::once(&bytes[..20]).chain((0..15).map(|i| &bytes[20 + i * 30..20 + i * 30 + 22])).all(|field| {
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        field[..end].iter().all(|&b| (0x20..0x7F).contains(&b) || b >= 0xA0) && field[end..].iter().all(|&b| b == 0 || (0x20..0x7F).contains(&b))
    });
    let samples_ok = module.samples.iter().all(|s| s.volume <= 64 && s.length <= 0x1_0000 * 2);
    let order_ok = (1..=128).contains(&module.order.len()) && bytes[472..600].iter().all(|&p| p < 64);
    names_ok && samples_ok && order_ok && module.patterns > 0 && (HEADER_15 + module.patterns * 1024) as u64 <= module.size
}

fn read_with(bytes: &[u8], slots: usize) -> Result<Module, String> {
    let header = if slots == 31 { HEADER_31 } else { HEADER_15 };
    if bytes.len() < header {
        return Err(crate::i18n::tr("The file is too short to be a module.").into());
    }
    let mut module = Module { title: text(&bytes[..20]), size: bytes.len() as u64, ..Module::default() };
    let at = 20 + slots * 30;
    let song_length = usize::from(bytes[at]);
    module.restart = bytes[at + 1];
    let all_orders = &bytes[at + 2..at + 2 + 128];
    module.order = all_orders[..song_length.min(128)].to_vec();
    // ProTracker counts the patterns from all 128 positions, played or not, and so does every player since.
    let highest = all_orders.iter().copied().max().unwrap_or(0);
    if slots == 31 {
        let tag = &bytes[1080..1084];
        let (channels, format) = tag_channels(tag).ok_or(crate::i18n::tr("The tag at byte 1080 isn't one a tracker writes."))?;
        module.tag = String::from_utf8_lossy(tag).into_owned();
        module.channels = channels;
        module.format = format.to_string();
    } else {
        module.channels = 4;
        module.format = crate::i18n::tr("Soundtracker (15 samples)").into();
    }
    // StarTrekker's 8 channels are pairs of 4-channel patterns, numbered in steps of two.
    let (patterns, pattern_bytes) = if module.tag == "FLT8" { (usize::from(highest) / 2 + 1, 2048) } else { (usize::from(highest) + 1, 64 * 4 * module.channels) };
    module.patterns = patterns;
    for i in 0..slots {
        let s = &bytes[20 + i * 30..20 + (i + 1) * 30];
        let finetune = (s[24] & 0x0F) as i8;
        module.samples.push(Sample {
            name: text(&s[..22]),
            length: word(s, 22) * 2,
            finetune: if finetune > 7 { finetune - 16 } else { finetune },
            volume: s[25],
            loop_start: word(s, 26) * 2,
            loop_length: word(s, 28) * 2,
        });
    }
    if song_length == 0 || song_length > 128 {
        module.notes.push(crate::i18n::tf!("The song length is {song_length}, which no tracker writes (1 to 128)."));
    }
    let sample_bytes: u64 = module.samples.iter().map(|s| u64::from(s.length)).sum();
    module.expected_size = header as u64 + (patterns * pattern_bytes) as u64 + sample_bytes;
    for (i, sample) in module.samples.iter_mut().enumerate() {
        if sample.volume > 64 {
            module.notes.push(crate::i18n::tf!("Sample {} has volume {}, above the 64 Paula plays at most.", i + 1, sample.volume));
        }
        if sample.loops() && sample.loop_start + sample.loop_length > sample.length {
            // Soundtracker and some early trackers kept the loop's start in bytes rather than words.
            let in_bytes = sample.loop_start / 2;
            if in_bytes + sample.loop_length <= sample.length {
                sample.loop_start = in_bytes;
                module.notes.push(crate::i18n::tf!("Sample {}'s loop start is in bytes, as older trackers wrote it.", i + 1));
            } else {
                module.notes.push(crate::i18n::tf!("Sample {}'s loop runs past its end ({} + {} of {} bytes).", i + 1, sample.loop_start, sample.loop_length, sample.length));
            }
        }
    }
    match module.size.cmp(&module.expected_size) {
        std::cmp::Ordering::Less => module.notes.push(crate::i18n::tf!("The file is {} bytes shorter than its patterns and samples need: it may have been cut short.", group(module.expected_size - module.size))),
        std::cmp::Ordering::Greater => module.notes.push(crate::i18n::tf!("There are {} more bytes after the samples than the module says it has.", group(module.size - module.expected_size))),
        std::cmp::Ordering::Equal => {}
    }
    Ok(module)
}

/// A number with its thousands marked, as File properties shows sizes.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Module {
    /// The samples that have something in them, with their slot numbers from 1.
    pub fn used_samples(&self) -> impl Iterator<Item = (usize, &Sample)> {
        self.samples.iter().enumerate().filter(|(_, s)| s.length > 0).map(|(i, s)| (i + 1, s))
    }

    pub fn sample_bytes(&self) -> u64 {
        self.samples.iter().map(|s| u64::from(s.length)).sum()
    }

    /// The song's order as pattern numbers in hex, as the trackers show them, sixteen to a line.
    pub fn order_text(&self) -> String {
        self.order.chunks(16).enumerate().map(|(row, chunk)| format!("{:02X}: {}", row * 16, chunk.iter().map(|p| format!("{p:02X}")).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("\n")
    }

    /// One line about a sample: its length, loop, volume and finetune.
    pub fn sample_line(sample: &Sample) -> String {
        let looped = if sample.loops() { format!("loops {} to {}", sample.loop_start, sample.loop_start + sample.loop_length) } else { "no loop".into() };
        let finetune = if sample.finetune == 0 { String::new() } else { format!(", finetune {:+}", sample.finetune) };
        format!("{} bytes, {looped}, volume {}{finetune}", group(u64::from(sample.length)), sample.volume)
    }

    fn summary(&self) -> Vec<(String, String)> {
        let format = if self.tag.is_empty() { self.format.clone() } else { format!("{} ({})", self.format, self.tag) };
        let mut rows = vec![
            ("Title".to_string(), if self.title.is_empty() { "(none)".into() } else { self.title.clone() }),
            ("Format".into(), format),
            ("Channels".into(), self.channels.to_string()),
            (crate::i18n::tr("Song length").into(), format!("{} positions", self.order.len())),
            ("Patterns".into(), self.patterns.to_string()),
            ("Samples".into(), crate::i18n::tf!("{} of {} slots used, {} bytes", self.used_samples().count(), self.samples.len(), group(self.sample_bytes()))),
        ];
        if self.restart != 127 && self.restart != 0 && !self.tag.is_empty() {
            rows.push((crate::i18n::tr("Restarts at").into(), format!("position {}", self.restart)));
        }
        rows
    }

    /// What File properties shows: the module, its samples and its order.
    pub fn sections(&self) -> Vec<InfoSection> {
        let mut first = self.summary();
        first.extend(self.notes.iter().map(|n| (String::new(), n.clone())));
        let samples = self.used_samples().map(|(n, s)| (format!("{n:02} {}", s.name), Self::sample_line(s))).collect();
        vec![
            InfoSection { title: crate::i18n::tr("Music module").into(), rows: first },
            InfoSection { title: "Samples".into(), rows: samples },
            InfoSection { title: crate::i18n::tr("Song order").into(), rows: vec![(String::new(), self.order.iter().map(|p| format!("{p:02X}")).collect::<Vec<_>>().join(" "))] },
        ]
    }

    /// The preview: the summary, a table of the samples, and the order.
    pub fn blocks(&self) -> Vec<Block> {
        let plain = |text: String| Span { text, ..Span::default() };
        let strong = |text: String| Span { text, strong: true, ..Span::default() };
        let block = |kind: BlockKind, spans: Vec<Span>| Block { kind, spans, indent: 0, quote: 0, marker: None };
        let mut blocks = vec![block(BlockKind::Heading(2), vec![plain(if self.title.is_empty() { crate::i18n::tr("Untitled module").into() } else { self.title.clone() })])];
        for (label, value) in self.summary().into_iter().skip(1) {
            blocks.push(block(BlockKind::Paragraph, vec![strong(format!("{label}: ")), plain(value)]));
        }
        for note in &self.notes {
            blocks.push(block(BlockKind::Paragraph, vec![Span { text: note.clone(), italic: true, ..Span::default() }]));
        }
        blocks.push(block(BlockKind::Heading(3), vec![plain("Samples".into())]));
        let mut rows = vec![["#", "Name", "Length", "Loop", "Volume", "Finetune"].map(|h| vec![strong(h.into())]).to_vec()];
        for (n, sample) in self.samples.iter().enumerate() {
            // Empty slots with names are kept: trackers' users wrote messages and credits in them.
            if sample.length == 0 && sample.name.trim().is_empty() {
                continue;
            }
            let looped = if sample.loops() { format!("{}\u{2013}{}", sample.loop_start, sample.loop_start + sample.loop_length) } else { String::new() };
            let cells = [format!("{:02}", n + 1), sample.name.clone(), if sample.length == 0 { String::new() } else { group(u64::from(sample.length)) }, looped, if sample.length == 0 { String::new() } else { sample.volume.to_string() }, if sample.finetune == 0 { String::new() } else { format!("{:+}", sample.finetune) }];
            rows.push(cells.map(|c| vec![plain(c)]).to_vec());
        }
        blocks.push(block(BlockKind::Table(rows), Vec::new()));
        blocks.push(block(BlockKind::Heading(3), vec![plain(crate::i18n::tr("Song order").into())]));
        blocks.push(block(BlockKind::Code, vec![Span { text: self.order_text(), code: true, ..Span::default() }]));
        blocks
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A module made up here: a title, samples given as (name, length, volume, finetune nibble, loop start,
    /// loop length) with lengths and loops in words, an order, and the tag; patterns and samples are zeros.
    pub fn synthetic(title: &str, samples: &[(&str, u16, u8, u8, u16, u16)], order: &[u8], tag: &[u8; 4], channels: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_31];
        bytes[..title.len()].copy_from_slice(title.as_bytes());
        for (i, (name, length, volume, finetune, start, looped)) in samples.iter().enumerate() {
            let at = 20 + i * 30;
            bytes[at..at + name.len()].copy_from_slice(name.as_bytes());
            bytes[at + 22..at + 24].copy_from_slice(&length.to_be_bytes());
            bytes[at + 24] = *finetune;
            bytes[at + 25] = *volume;
            bytes[at + 26..at + 28].copy_from_slice(&start.to_be_bytes());
            bytes[at + 28..at + 30].copy_from_slice(&looped.to_be_bytes());
        }
        bytes[950] = order.len() as u8;
        bytes[951] = 127;
        bytes[952..952 + order.len()].copy_from_slice(order);
        bytes[1080..1084].copy_from_slice(tag);
        let patterns = usize::from(*order.iter().max().unwrap_or(&0)) + 1;
        bytes.resize(HEADER_31 + patterns * 256 * channels, 0);
        for (_, length, ..) in samples {
            bytes.resize(bytes.len() + usize::from(*length) * 2, 0);
        }
        bytes
    }

    #[test]
    fn a_protracker_module_says_its_title_samples_and_order() {
        let bytes = synthetic("demo tune", &[("bassdrum", 1000, 64, 0, 0, 1), ("strings", 4000, 48, 0x0F, 1000, 3000), ("", 0, 0, 0, 0, 1), ("by someone", 0, 0, 0, 0, 1)], &[0, 1, 1, 3, 2], b"M.K.", 4);
        let module = read(&bytes, "tune").unwrap();
        assert_eq!(module.title, "demo tune");
        assert_eq!((module.channels, module.patterns, module.tag.as_str()), (4, 4, "M.K."));
        assert_eq!(module.order, [0, 1, 1, 3, 2]);
        assert_eq!(module.samples.len(), 31);
        assert_eq!(module.samples[0], Sample { name: "bassdrum".into(), length: 2000, finetune: 0, volume: 64, loop_start: 0, loop_length: 2 });
        assert!(!module.samples[0].loops(), "a loop of one word is ProTracker's way of saying there's none");
        let strings = &module.samples[1];
        assert_eq!((strings.finetune, strings.loop_start, strings.loop_length, strings.loops()), (-1, 2000, 6000, true));
        assert_eq!(module.expected_size, module.size);
        assert!(module.notes.is_empty(), "{:?}", module.notes);
        assert_eq!(module.used_samples().map(|(n, _)| n).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(module.sample_bytes(), 10_000);
        assert_eq!(module.order_text(), "00: 00 01 01 03 02");
        // The preview lists the named empty slot, where a credit might be, but not the blank one.
        let blocks = module.blocks();
        let table = blocks.iter().find_map(|b| if let BlockKind::Table(rows) = &b.kind { Some(rows) } else { None }).unwrap();
        assert_eq!(table.len(), 4);
        assert_eq!(table[3][1][0].text, "by someone");
        let sections = module.sections();
        assert_eq!(sections[1].rows[1], ("02 strings".to_string(), "8,000 bytes, loops 2000 to 8000, volume 48, finetune -1".to_string()));
    }

    #[test]
    fn patterns_are_counted_from_all_128_positions() {
        let mut bytes = synthetic("", &[], &[0, 1], b"M.K.", 4);
        // A pattern number past the song's end still counts, as it does in ProTracker.
        bytes[952 + 5] = 2;
        bytes.resize(bytes.len() + 1024, 0);
        assert_eq!(read(&bytes, "x.mod").unwrap().patterns, 3);
    }

    #[test]
    fn other_trackers_tags_give_their_channels() {
        for (tag, channels) in [(b"6CHN", 6), (b"8CHN", 8), (b"16CH", 16), (b"FLT4", 4), (b"M!K!", 4), (b"CD81", 8), (b"TDZ3", 3)] {
            let bytes = synthetic("", &[], &[0], tag, channels);
            let module = read(&bytes, "").unwrap();
            assert_eq!(module.channels, channels, "{}", String::from_utf8_lossy(tag));
            assert!(module.notes.is_empty(), "{:?}", module.notes);
        }
        assert!(read(&synthetic("", &[], &[0], b"ABCD", 4), "x.mod").is_err());
    }

    #[test]
    fn a_cut_short_module_and_one_with_extra_bytes_say_so() {
        let mut bytes = synthetic("", &[("a", 100, 64, 0, 0, 1)], &[0], b"M.K.", 4);
        bytes.truncate(bytes.len() - 50);
        assert!(read(&bytes, "").unwrap().notes[0].contains("50 bytes shorter"));
        bytes.resize(bytes.len() + 1050, 0);
        assert!(read(&bytes, "").unwrap().notes[0].contains("1,000 more bytes"));
    }

    #[test]
    fn a_loop_start_in_bytes_is_recognised() {
        // 1000 words long, looping from byte 1200 for 400 words: in words that would run past the end.
        let bytes = synthetic("", &[("old", 1000, 64, 0, 1200, 400)], &[0], b"M.K.", 4);
        let module = read(&bytes, "").unwrap();
        assert_eq!((module.samples[0].loop_start, module.samples[0].loop_length), (1200, 800));
        assert!(module.notes[0].contains("in bytes"));
    }

    #[test]
    fn a_15_sample_soundtracker_module_is_read_only_when_named_as_one() {
        let mut bytes = vec![0u8; HEADER_15];
        bytes[..5].copy_from_slice(b"intro");
        bytes[20..25].copy_from_slice(b"drum1");
        bytes[42..44].copy_from_slice(&500u16.to_be_bytes());
        bytes[45] = 64;
        bytes[48..50].copy_from_slice(&1u16.to_be_bytes());
        bytes[470] = 2;
        bytes[471] = 120;
        bytes[472] = 0;
        bytes[473] = 1;
        bytes.resize(HEADER_15 + 2 * 1024 + 1000, 0);
        let module = read(&bytes, "mod.intro").unwrap();
        assert_eq!((module.samples.len(), module.patterns, module.channels), (15, 2, 4));
        assert_eq!(module.samples[0].length, 1000);
        assert_eq!(module.title, "intro");
        assert!(module.notes.is_empty(), "{:?}", module.notes);
        assert!(read(&bytes, "intro.bin").is_err(), "without a tag, only the name says it's a module");
        // Text named .mod (Modula-2 shares the ending) isn't taken for one.
        let modula = b"MODULE Hello;\nIMPORT InOut;\nBEGIN InOut.WriteString('hi') END Hello.\n".repeat(40);
        assert!(read(&modula, "Hello.mod").is_err());
    }

    #[test]
    fn module_names_are_recognised_both_ways_round() {
        assert!(named_as_module("song.MOD"));
        assert!(named_as_module("DH0:Music/mod.4mat-intro"));
        assert!(!named_as_module("mod."));
        assert!(!named_as_module("model.txt"));
    }

    /// Real modules, from a folder named by LIMONITER_TEST_MODS (they aren't Limoniter's to include): each must
    /// read whole, with its patterns and samples all there.
    #[test]
    fn real_modules_read_whole() {
        let Some(folder) = std::env::var_os("LIMONITER_TEST_MODS") else {
            eprintln!("LIMONITER_TEST_MODS isn't set, so no real modules were read.");
            return;
        };
        let entries = std::fs::read_dir(&folder).unwrap();
        for entry in entries.flatten() {
            let path = entry.path();
            let bytes = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let module = read(&bytes, &name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(module.patterns > 0 && !module.order.is_empty(), "{name}");
            assert!(module.size >= module.expected_size, "{name}: {:?}", module.notes);
            eprintln!("{name}: {:?} {} {} channels, {} positions, {} patterns, {} sample bytes in {} samples, {:?}", module.title, module.tag, module.channels, module.order.len(), module.patterns, module.sample_bytes(), module.used_samples().count(), module.notes);
        }
    }
}
