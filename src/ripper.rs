//! The ripper: music and graphics found in any file, and inside every file of a disk, tape or archive image.
//!
//! Music is found by the marks each kind carries (a ProTracker module's M.K. at byte 1080, a SID file's PSID, an
//! SNDH's tag, YM's header...; an old 15-sample Soundtracker module, which has none, by its instruments' st-01:
//! sample-disk names), and each find's length worked out from its own header where it says enough;
//! where it doesn't, the find runs to the next one or the file's end. Graphics are any bytes seen as the machines
//! kept their pictures: a bit a pixel, characters, the C64's multicolour and sprites, NES and Game Boy tiles,
//! Amiga and Atari ST bitplanes, a Spectrum screen, or a byte a pixel.

use std::sync::Arc;

/// A kind of music.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Music {
    ProTracker,
    /// An older module with 15 samples and no mark.
    Soundtracker,
    ScreamTracker,
    FastTracker,
    ImpulseTracker,
    Sid,
    Sndh,
    /// An SNDH packed with Pack-Ice, as most are kept.
    SndhPacked,
    Ym,
    /// A YM file packed with LHA, as they're usually kept.
    YmPacked,
    Pt3,
    Ay,
}

impl Music {
    pub fn name(self) -> &'static str {
        match self {
            Music::ProTracker => "ProTracker module",
            Music::Soundtracker => "Soundtracker module (15 samples)",
            Music::ScreamTracker => "Scream Tracker 3 module",
            Music::FastTracker => "FastTracker 2 module",
            Music::ImpulseTracker => "Impulse Tracker module",
            Music::Sid => "SID tune",
            Music::Sndh => "SNDH tune",
            Music::SndhPacked => "SNDH tune (Pack-Ice)",
            Music::Ym => "YM tune",
            Music::YmPacked => "YM tune (LHA)",
            Music::Pt3 => "ProTracker 3 tune",
            Music::Ay => "AY tune (ZXAYEMUL)",
        }
    }

    /// The file ending it's saved with.
    pub fn extension(self) -> &'static str {
        match self {
            Music::ProTracker | Music::Soundtracker => "mod",
            Music::ScreamTracker => "s3m",
            Music::FastTracker => "xm",
            Music::ImpulseTracker => "it",
            Music::Sid => "sid",
            Music::Sndh | Music::SndhPacked => "sndh",
            Music::Ym | Music::YmPacked => "ym",
            Music::Pt3 => "pt3",
            Music::Ay => "ay",
        }
    }
}

/// Music found: where, what, and its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The file it's in: the file itself, or a file inside an image (as "GAMES.D64 › INTRO").
    pub place: String,
    pub offset: usize,
    pub kind: Music,
    /// The title it gives itself, if any.
    pub title: String,
    pub bytes: Arc<Vec<u8>>,
    /// Whether its length is its own (from its header) rather than up to the next find or the file's end.
    pub sized: bool,
}

impl Found {
    /// A name to save it as.
    pub fn file_name(&self) -> String {
        let base: String = self.title.trim().chars().map(|c| if c.is_alphanumeric() || " -_.()".contains(c) { c } else { '_' }).collect();
        let base = base.trim().trim_matches('.').to_string();
        let base = if base.is_empty() { format!("{}-{:06X}", crate::sfx::label_of(&self.place), self.offset) } else { base };
        format!("{base}.{}", self.kind.extension())
    }
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn be32(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn le16(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn le32(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

/// Text from a header: up to its first zero, without what isn't printable.
fn text(b: &[u8], at: usize, length: usize) -> String {
    let Some(part) = b.get(at..(at + length).min(b.len())) else {
        return String::new();
    };
    part.iter().take_while(|&&c| c != 0).map(|&c| if (32..127).contains(&c) { c as char } else { ' ' }).collect::<String>().trim().to_string()
}

/// Every place a mark is.
fn all(bytes: &[u8], mark: &[u8]) -> Vec<usize> {
    memchr::memmem::find_iter(bytes, mark).collect()
}

/// A ProTracker module (or one of its kin) whose mark is at `tag`: where it starts and how long it is.
fn protracker(bytes: &[u8], tag: usize) -> Option<(usize, usize, String)> {
    let start = tag.checked_sub(1080)?;
    let mark = bytes.get(tag..tag + 4)?;
    let channels = match mark {
        b"M.K." | b"M!K!" | b"FLT4" | b"4CHN" => 4,
        b"6CHN" => 6,
        b"8CHN" | b"FLT8" | b"CD81" | b"OKTA" => 8,
        [a, b'C', b'H', b'N'] if a.is_ascii_digit() => usize::from(a - b'0'),
        [a, b, b'C', b'H'] if a.is_ascii_digit() && b.is_ascii_digit() => usize::from(a - b'0') * 10 + usize::from(b - b'0'),
        _ => return None,
    };
    if channels == 0 || channels > 32 {
        return None;
    }
    let m = &bytes[start..];
    let song_length = usize::from(*m.get(950)?);
    if !(1..=128).contains(&song_length) {
        return None;
    }
    let mut samples = 0usize;
    for s in 0..31 {
        let at = 20 + s * 30;
        let length = be16(m, at + 22)? * 2;
        let finetune = m[at + 24];
        let volume = m[at + 25];
        if finetune > 15 || volume > 64 {
            return None;
        }
        samples += length;
    }
    let patterns = m.get(952..952 + 128)?.iter().copied().max()? as usize + 1;
    if patterns > 128 {
        return None;
    }
    let length = 1084 + patterns * 64 * channels * 4 + samples;
    // A module whose samples are cut short at the file's end is still kept: many rips are.
    let length = length.min(m.len());
    if length < 1084 + 64 * channels * 4 {
        return None;
    }
    Some((start, length, text(m, 0, 20)))
}

/// Whether a name field is text: printable up to its first zero, and only zeros or text after it.
fn name_field(field: &[u8]) -> bool {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    field[..end].iter().all(|&b| (0x20..0x7F).contains(&b) || b >= 0xA0) && field[end..].iter().all(|&b| b == 0 || (0x20..0x7F).contains(&b))
}

/// A 15-sample Soundtracker module laid out from `start`: the length it says it has (which may run past the
/// bytes there are), and its title. It has no mark, so everything it can be checked by is: text names, volumes up
/// to 64, a song of 1 to 128 positions, and patterns numbered under 64.
fn soundtracker(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    let m = bytes.get(start..)?;
    if m.len() < 600 + 1024 || protracker(bytes, start + 1080).is_some() {
        return None;
    }
    if !name_field(&m[..20]) {
        return None;
    }
    let mut samples = 0usize;
    for s in 0..15 {
        let at = 20 + s * 30;
        if !name_field(&m[at..at + 22]) || m[at + 24] > 15 || m[at + 25] > 64 {
            return None;
        }
        samples += be16(m, at + 22)? * 2;
    }
    if !(1..=128).contains(&m[470]) || m[472..600].iter().any(|&p| p >= 64) {
        return None;
    }
    let patterns = usize::from(*m[472..600].iter().max()?) + 1;
    let length = 600 + patterns * 1024 + samples;
    (m.len() >= 600 + patterns * 1024).then(|| (length, text(m, 0, 20)))
}

/// Where 15-sample modules may start: wherever an instrument is named for a Soundtracker sample disk, as most
/// were (st-01:strings), the module starts at that name's place among the 15.
fn soundtracker_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts: Vec<usize> = [&b"st-"[..], b"ST-", b"St-"]
        .iter()
        .flat_map(|mark| all(bytes, mark))
        .filter(|&at| bytes.get(at + 3..at + 6).is_some_and(|n| n[0].is_ascii_digit() && n[1].is_ascii_digit() && n[2] == b':'))
        .flat_map(|at| (0..15).filter_map(move |s| at.checked_sub(20 + s * 30)))
        .collect();
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// A Scream Tracker 3 module whose SCRM is at `mark`: its start, length and title.
fn scream_tracker(bytes: &[u8], mark: usize) -> Option<(usize, usize, String)> {
    let start = mark.checked_sub(44)?;
    let m = &bytes[start..];
    if *m.get(28)? != 0x1A || *m.get(29)? != 16 {
        return None;
    }
    let (orders, instruments, patterns) = (le16(m, 32)?, le16(m, 34)?, le16(m, 36)?);
    if orders > 256 || instruments > 100 || patterns > 256 {
        return None;
    }
    let mut end = 0x60 + orders + 2 * (instruments + patterns);
    for i in 0..instruments {
        let at = le16(m, 0x60 + orders + 2 * i)? * 16;
        end = end.max(at + 80);
        if *m.get(at)? == 1 {
            let memseg = usize::from(*m.get(at + 13)?) << 16 | le16(m, at + 14)?;
            let length = le32(m, at + 16)?;
            let flags = *m.get(at + 31)?;
            let bytes_each = if flags & 4 != 0 { 2 } else { 1 } * if flags & 2 != 0 { 2 } else { 1 };
            end = end.max(memseg * 16 + length * bytes_each);
        }
    }
    for p in 0..patterns {
        let at = le16(m, 0x60 + orders + 2 * (instruments + p))? * 16;
        if at > 0 {
            end = end.max(at + le16(m, at)?);
        }
    }
    Some((start, end.min(m.len()), text(m, 0, 28)))
}

/// A FastTracker 2 module at `start`: its length and title.
fn fast_tracker(bytes: &[u8], start: usize) -> Option<(usize, usize, String)> {
    let m = &bytes[start..];
    if *m.get(37)? != 0x1A {
        return None;
    }
    let header = le32(m, 60)?;
    let (patterns, instruments) = (le16(m, 70)?, le16(m, 72)?);
    if patterns > 256 || instruments > 128 || header < 20 {
        return None;
    }
    let mut at = 60 + header;
    for _ in 0..patterns {
        let length = le32(m, at)?;
        let packed = le16(m, at + 7)?;
        at += length + packed;
    }
    for _ in 0..instruments {
        let size = le32(m, at)?;
        let samples = le16(m, at + 27)?;
        if samples == 0 {
            at += size;
            continue;
        }
        let header_size = le32(m, at + 29)?;
        at += size;
        let mut data = 0;
        for _ in 0..samples {
            data += le32(m, at)?;
            at += header_size;
        }
        at += data;
    }
    if at > m.len() + 1024 {
        return None;
    }
    Some((start, at.min(m.len()), text(m, 17, 20)))
}

/// An Impulse Tracker module at `start`.
fn impulse_tracker(bytes: &[u8], start: usize) -> Option<(usize, usize, String)> {
    let m = &bytes[start..];
    let (orders, instruments, samples, patterns) = (le16(m, 0x20)?, le16(m, 0x22)?, le16(m, 0x24)?, le16(m, 0x26)?);
    if orders > 256 || instruments > 256 || samples > 256 || patterns > 256 {
        return None;
    }
    let table = 0xC0 + orders;
    let mut end = table + 4 * (instruments + samples + patterns);
    let mut whole = true;
    for i in 0..instruments {
        end = end.max(le32(m, table + 4 * i)? + 554);
    }
    for s in 0..samples {
        let at = le32(m, table + 4 * (instruments + s))?;
        end = end.max(at + 0x50);
        let flags = *m.get(at + 0x12)?;
        if flags & 1 != 0 {
            let (length, pointer) = (le32(m, at + 0x30)?, le32(m, at + 0x48)?);
            let channels = if flags & 4 != 0 { 2 } else { 1 };
            if flags & 8 != 0 {
                // Compressed: blocks of up to 32,768 samples (16,384 when 16-bit), each its length then its bits.
                let per = if flags & 2 != 0 { 0x4000 } else { 0x8000 };
                let mut p = pointer;
                for _ in 0..length.div_ceil(per) * channels {
                    match le16(m, p) {
                        Some(n) => p += 2 + n,
                        None => {
                            whole = false;
                            break;
                        }
                    }
                }
                end = end.max(p);
            } else {
                let each = if flags & 2 != 0 { 2 } else { 1 } * channels;
                end = end.max(pointer + length * each);
            }
        }
    }
    for p in 0..patterns {
        let at = le32(m, table + 4 * (instruments + samples + p))?;
        if at > 0 {
            end = end.max(at + 8 + le16(m, at)?);
        }
    }
    let end = if whole { end.min(m.len()) } else { m.len() };
    Some((start, end, text(m, 4, 26)))
}

/// A PSID or RSID header at `start`: its title, and whether it looks right.
fn sid(bytes: &[u8], start: usize) -> Option<String> {
    let version = be16(bytes, start + 4)?;
    let offset = be16(bytes, start + 6)?;
    if !(1..=4).contains(&version) || (offset != 0x76 && offset != 0x7C) {
        return None;
    }
    let songs = be16(bytes, start + 14)?;
    (1..=256).contains(&songs).then(|| {
        let name = text(bytes, start + 0x16, 32);
        let author = text(bytes, start + 0x36, 32);
        if author.is_empty() { name } else { format!("{name} - {author}") }
    })
}

/// A YM5 or YM6 file at `start`: its length and title.
fn ym(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    let m = &bytes[start..];
    if m.get(4..12)? != b"LeOnArD!" {
        return None;
    }
    let frames = be32(m, 12)?;
    let drums = be16(m, 20)?;
    let extra = be16(m, 32)?;
    let mut at = 34 + extra;
    for _ in 0..drums {
        at += 4 + be32(m, at)?;
    }
    let title = text(m, at, 64);
    for _ in 0..3 {
        at += m.get(at..)?.iter().position(|&c| c == 0)? + 1;
    }
    let end = at + frames.checked_mul(16)? + 4;
    (end <= m.len() + 4).then(|| (end.min(m.len()), title))
}

/// An LHA archive's header at `start` holding a YM file: its whole length and the name inside.
fn lha_ym(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    let m = &bytes[start..];
    let header = usize::from(*m.first()?);
    let level = *m.get(20)?;
    let packed = le32(m, 7)?;
    let name_length = usize::from(*m.get(21)?);
    let name = text(m, 22, name_length);
    if level > 1 || header < 22 || !name.to_ascii_lowercase().ends_with(".ym") {
        return None;
    }
    let length = 2 + header + packed;
    (length <= m.len() + 1).then(|| (length.min(m.len()), name))
}

/// Music in some bytes: each find, its start and its own length where it has one.
pub fn find_music(bytes: &[u8], place: &str) -> Vec<Found> {
    let mut finds: Vec<(usize, Option<usize>, Music, String)> = Vec::new();
    // The marks, and the numbered ones (6CHN, 10CH, 32CH) found by their letters.
    let mut tags: Vec<usize> = [&b"M.K."[..], b"M!K!", b"FLT4", b"FLT8", b"CD81", b"OKTA"].iter().flat_map(|mark| all(bytes, mark)).collect();
    tags.extend(all(bytes, b"CHN").into_iter().filter_map(|at| at.checked_sub(1)));
    tags.extend(all(bytes, b"CH").into_iter().filter_map(|at| at.checked_sub(2)).filter(|&at| bytes.get(at + 3) != Some(&b'N')));
    for tag in tags {
        if let Some((start, length, title)) = protracker(bytes, tag) {
            finds.push((start, Some(length), Music::ProTracker, title));
        }
    }
    for start in soundtracker_starts(bytes) {
        if let Some((length, title)) = soundtracker(bytes, start) {
            finds.push((start, Some(length.min(bytes.len() - start)), Music::Soundtracker, title));
        }
    }
    // A whole file laid out as one, named or not, whose length is the module's own.
    if let Some((length, title)) = soundtracker(bytes, 0)
        && bytes.len() >= length
        && bytes.len() - length < 64
    {
        finds.push((0, Some(length), Music::Soundtracker, title));
    }
    for mark in all(bytes, b"SCRM") {
        if let Some((start, length, title)) = scream_tracker(bytes, mark) {
            finds.push((start, Some(length), Music::ScreamTracker, title));
        }
    }
    for start in all(bytes, b"Extended Module: ") {
        if let Some((start, length, title)) = fast_tracker(bytes, start) {
            finds.push((start, Some(length), Music::FastTracker, title));
        }
    }
    for start in all(bytes, b"IMPM") {
        if let Some((start, length, title)) = impulse_tracker(bytes, start) {
            finds.push((start, Some(length), Music::ImpulseTracker, title));
        }
    }
    for mark in [&b"PSID"[..], b"RSID"] {
        for start in all(bytes, mark) {
            if let Some(title) = sid(bytes, start) {
                finds.push((start, None, Music::Sid, title));
            }
        }
    }
    // SNDH: the tag 12 bytes in, after the jumps to init, exit and play.
    for tag in all(bytes, b"SNDH") {
        if let Some(start) = tag.checked_sub(12)
            && bytes.get(start) == Some(&0x60)
        {
            let title = all(&bytes[tag..(tag + 512).min(bytes.len())], b"TITL").first().map(|&t| text(bytes, tag + t + 4, 64)).unwrap_or_default();
            finds.push((start, None, Music::Sndh, title));
        }
    }
    // Pack-Ice: its length is in its header; it's an SNDH if what it unpacks to is one.
    for start in all(bytes, b"ICE!") {
        let Some((_, packed, _)) = crate::st_packers::header(&bytes[start..]) else { continue };
        let length = packed as usize;
        if length < 12 || start + length > bytes.len() {
            continue;
        }
        if let Some(Ok(unpacked)) = crate::st_packers::unpack(&bytes[start..start + length])
            && unpacked.bytes.get(12..16) == Some(b"SNDH")
        {
            let head = &unpacked.bytes[..unpacked.bytes.len().min(512)];
            let title = all(head, b"TITL").first().map(|&t| text(head, t + 4, 64)).unwrap_or_default();
            finds.push((start, Some(length), Music::SndhPacked, title));
        }
    }
    for mark in [&b"YM5!"[..], b"YM6!"] {
        for start in all(bytes, mark) {
            if let Some((length, title)) = ym(bytes, start) {
                finds.push((start, Some(length), Music::Ym, title));
            }
        }
    }
    for method in [&b"-lh5-"[..], b"-lh0-"] {
        for at in all(bytes, method) {
            if let Some(start) = at.checked_sub(2)
                && let Some((length, name)) = lha_ym(bytes, start)
            {
                finds.push((start, Some(length), Music::YmPacked, name.trim_end_matches(".ym").trim_end_matches(".YM").to_string()));
            }
        }
    }
    for mark in [&b"ProTracker 3."[..], b"Vortex Tracker II"] {
        for start in all(bytes, mark) {
            if bytes.get(start + 30..start + 34) == Some(b"ompi") || bytes.get(start + 13..start + 14).is_some_and(|v| v[0].is_ascii_digit()) || mark.starts_with(b"Vortex") {
                finds.push((start, None, Music::Pt3, text(bytes, start + 30, 32).trim_start_matches("ompilation of").trim_start_matches(':').trim().to_string()));
            }
        }
    }
    for start in all(bytes, b"ZXAYEMUL") {
        finds.push((start, None, Music::Ay, String::new()));
    }
    finds.sort_by_key(|f| f.0);
    finds.dedup_by_key(|f| f.0);
    // A find inside an earlier one's own length is part of it (a module's sample that happens to say M.K.).
    let mut kept: Vec<(usize, Option<usize>, Music, String)> = Vec::new();
    for f in finds {
        if kept.iter().any(|k| k.1.is_some_and(|length| f.0 < k.0 + length)) {
            continue;
        }
        kept.push(f);
    }
    let starts: Vec<usize> = kept.iter().map(|k| k.0).collect();
    kept.into_iter()
        .enumerate()
        .map(|(i, (start, length, kind, title))| {
            let end = match length {
                Some(l) => start + l,
                None => starts.get(i + 1).copied().unwrap_or(bytes.len()),
            };
            Found { place: place.to_string(), offset: start, kind, title, bytes: Arc::new(bytes[start..end.min(bytes.len())].to_vec()), sized: length.is_some() }
        })
        .collect()
}

/// The biggest file looked through inside an image.
const MOST_INSIDE: usize = 64 * 1024 * 1024;

/// Music in a file, and, if it's a disk, tape or archive image, in every file inside it (and inside images inside
/// those), each find once.
pub fn find_music_everywhere(bytes: &[u8], name: &str) -> Vec<Found> {
    let mut finds = find_music(bytes, name);
    // A packed disk's tracks, unpacked: where demos and games keep music that isn't in a file.
    if name.to_ascii_lowercase().ends_with(".dms")
        && let Ok(disk) = crate::media::undms(&bytes.to_vec())
    {
        finds.extend(find_music(&disk, &format!("{name} \u{203A} its tracks")));
    }
    if crate::media::is_image(name, &bytes[..bytes.len().min(4096)], bytes.len() as u64) {
        let source: Arc<dyn crate::media::Source> = Arc::new(bytes.to_vec());
        if let Ok(volume) = crate::media::open(source, name) {
            inside(volume.as_ref(), None, name, &mut finds, 0);
        }
    }
    // Music found in the image's own bytes that's also in one of its files is kept as the file's: the image's
    // copy may be cut up, as a disk keeps a file in blocks.
    let in_files: std::collections::HashSet<(Music, String)> = finds.iter().filter(|f| f.place != name).map(|f| (f.kind, f.title.clone())).collect();
    finds.retain(|f| f.place != name || f.title.is_empty() || !in_files.contains(&(f.kind, f.title.clone())));
    // The same music found twice is kept once.
    let mut seen = std::collections::HashSet::new();
    let mut unique: Vec<Found> = Vec::new();
    for f in finds.into_iter().rev() {
        if seen.insert(f.bytes.clone()) {
            unique.push(f);
        }
    }
    unique.reverse();
    unique
}

fn inside(volume: &dyn crate::media::Volume, dir: Option<&crate::media::Entry>, place: &str, finds: &mut Vec<Found>, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = volume.list(dir) else { return };
    for entry in entries {
        let here = format!("{place} \u{203A} {}", entry.name);
        if entry.is_dir {
            inside(volume, Some(&entry), &here, finds, depth + 1);
            continue;
        }
        if entry.size as usize > MOST_INSIDE {
            continue;
        }
        let Ok(bytes) = volume.read(&entry) else { continue };
        finds.extend(find_music(&bytes, &here));
        if depth < 3 && crate::media::is_nested_image(&entry.open_name, &bytes) {
            let source: Arc<dyn crate::media::Source> = Arc::new(bytes);
            if let Ok(nested) = crate::media::open(source, &entry.open_name) {
                inside(nested.as_ref(), None, &here, finds, depth + 1);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Graphics.

/// How bytes are seen as pictures.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Layout {
    /// A bit a pixel, a row of `across` bytes after another.
    Bits,
    /// A bit a pixel in 8×8 characters of 8 bytes, `across` characters a row: fonts, UDGs, charsets.
    Characters,
    /// The C64's multicolour: two bits a pixel, twice as wide, in rows of `across` bytes.
    C64Multi,
    /// The C64's multicolour characters: 8 bytes each, `across` a row.
    C64MultiCharacters,
    /// The C64's sprites: 64 bytes each (24×21), `across` a row.
    C64Sprites,
    C64MultiSprites,
    /// NES tiles: 16 bytes each, the low bits' 8 rows, then the high bits'.
    Nes,
    /// Game Boy tiles: 16 bytes each, each row's two bytes together.
    GameBoy,
    /// Amiga bitplanes: `planes` whole planes one after another, each `across` bytes by the height.
    Amiga,
    /// Atari ST: `planes` planes interleaved a 16-pixel word at a time, `across` words of 16 pixels a row.
    AtariSt,
    /// A Spectrum screen: 6144 bytes of pixels in the screen's order, then 768 of colours.
    Spectrum,
    /// A byte a pixel, as the Next's 8-bit colours (RRRGGGBB).
    Bytes,
}

impl Layout {
    pub const ALL: [Layout; 12] = [Layout::Bits, Layout::Characters, Layout::C64Multi, Layout::C64MultiCharacters, Layout::C64Sprites, Layout::C64MultiSprites, Layout::Nes, Layout::GameBoy, Layout::Amiga, Layout::AtariSt, Layout::Spectrum, Layout::Bytes];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Bits => "1 bit a pixel, in rows",
            Layout::Characters => "8\u{D7}8 characters (fonts, UDGs)",
            Layout::C64Multi => "C64 multicolour, in rows",
            Layout::C64MultiCharacters => "C64 multicolour characters",
            Layout::C64Sprites => "C64 sprites",
            Layout::C64MultiSprites => "C64 multicolour sprites",
            Layout::Nes => "NES tiles",
            Layout::GameBoy => "Game Boy tiles",
            Layout::Amiga => "Amiga bitplanes",
            Layout::AtariSt => "Atari ST bitplanes",
            Layout::Spectrum => "Spectrum screen",
            Layout::Bytes => "A byte a pixel (Next)",
        }
    }

    /// What `across` counts.
    pub fn unit(self) -> &'static str {
        match self {
            Layout::Bits | Layout::C64Multi | Layout::Amiga | Layout::Bytes => "bytes across",
            Layout::Characters | Layout::C64MultiCharacters => "characters across",
            Layout::C64Sprites | Layout::C64MultiSprites => "sprites across",
            Layout::Nes | Layout::GameBoy => "tiles across",
            Layout::AtariSt => "words of 16 pixels across",
            Layout::Spectrum => "",
        }
    }

    pub fn usual_across(self) -> usize {
        match self {
            Layout::Bits | Layout::C64Multi => 1,
            Layout::Characters | Layout::C64MultiCharacters | Layout::Nes | Layout::GameBoy => 16,
            Layout::C64Sprites | Layout::C64MultiSprites => 8,
            Layout::Amiga => 40,
            Layout::AtariSt => 20,
            Layout::Spectrum => 1,
            Layout::Bytes => 16,
        }
    }
}

/// A view of bytes as a picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct View {
    pub layout: Layout,
    pub offset: usize,
    pub across: usize,
    /// Rows of pixels, or of characters, tiles or sprites.
    pub down: usize,
    pub planes: usize,
}

/// A picture made from bytes: its size and each pixel's colour.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// How many bytes it's made from, from the offset.
    pub bytes: usize,
}

/// The C64's colours (Pepto's).
const C64: [[u8; 3]; 16] = [[0, 0, 0], [255, 255, 255], [104, 55, 43], [112, 164, 178], [111, 61, 134], [88, 141, 67], [53, 40, 121], [184, 199, 111], [111, 79, 37], [67, 57, 0], [154, 103, 89], [68, 68, 68], [108, 108, 108], [154, 210, 132], [108, 94, 181], [149, 149, 149]];

fn grey(levels: usize, index: usize) -> [u8; 3] {
    let v = (index * 255 / (levels - 1).max(1)) as u8;
    [v, v, v]
}

/// The bytes seen as a picture.
pub fn picture(bytes: &[u8], view: &View) -> Picture {
    let at = |i: usize| bytes.get(view.offset + i).copied().unwrap_or(0);
    let across = view.across.max(1);
    let down = view.down.max(1);
    // Each pixel's index into the palette, row by row.
    let (width, height, pixels, palette, used): (usize, usize, Vec<usize>, Vec<[u8; 3]>, usize) = match view.layout {
        Layout::Bits => {
            let (w, h) = (across * 8, down);
            let p = (0..w * h).map(|i| usize::from(at((i / w) * across + (i % w) / 8) >> (7 - i % 8) & 1)).collect();
            (w, h, p, vec![[0, 0, 0], [255, 255, 255]], across * down)
        }
        Layout::Characters => {
            let (w, h) = (across * 8, down * 8);
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let cell = (y / 8) * across + x / 8;
                usize::from(at(cell * 8 + y % 8) >> (7 - x % 8) & 1)
            }).collect();
            (w, h, p, vec![[0, 0, 0], [255, 255, 255]], across * down * 8)
        }
        Layout::C64Multi => {
            let (w, h) = (across * 8, down);
            let p = (0..w * h).map(|i| usize::from(at((i / w) * across + (i % w) / 8) >> (6 - 2 * ((i % 8) / 2)) & 3)).collect();
            (w, h, p, vec![C64[6], C64[11], C64[1], C64[14]], across * down)
        }
        Layout::C64MultiCharacters => {
            let (w, h) = (across * 8, down * 8);
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let cell = (y / 8) * across + x / 8;
                usize::from(at(cell * 8 + y % 8) >> (6 - 2 * ((x % 8) / 2)) & 3)
            }).collect();
            (w, h, p, vec![C64[6], C64[11], C64[1], C64[14]], across * down * 8)
        }
        Layout::C64Sprites | Layout::C64MultiSprites => {
            let multi = view.layout == Layout::C64MultiSprites;
            let (w, h) = (across * 24, down * 21);
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let sprite = (y / 21) * across + x / 24;
                let (sx, sy) = (x % 24, y % 21);
                let byte = at(sprite * 64 + sy * 3 + sx / 8);
                if multi { usize::from(byte >> (6 - 2 * ((sx % 8) / 2)) & 3) } else { usize::from(byte >> (7 - sx % 8) & 1) * 3 }
            }).collect();
            (w, h, p, vec![C64[6], C64[11], C64[1], C64[14]], across * down * 64)
        }
        Layout::Nes | Layout::GameBoy => {
            let gb = view.layout == Layout::GameBoy;
            let (w, h) = (across * 8, down * 8);
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let tile = (y / 8) * across + x / 8;
                let row = y % 8;
                let (low, high) = if gb { (at(tile * 16 + row * 2), at(tile * 16 + row * 2 + 1)) } else { (at(tile * 16 + row), at(tile * 16 + 8 + row)) };
                let bit = 7 - x % 8;
                usize::from(low >> bit & 1) | usize::from(high >> bit & 1) << 1
            }).collect();
            let palette = if gb { vec![[224, 248, 208], [136, 192, 112], [52, 104, 86], [8, 24, 32]] } else { (0..4).map(|i| grey(4, i)).collect() };
            (w, h, p, palette, across * down * 16)
        }
        Layout::Amiga => {
            let planes = view.planes.clamp(1, 8);
            let (w, h) = (across * 8, down);
            let plane = across * down;
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                (0..planes).map(|n| usize::from(at(n * plane + y * across + x / 8) >> (7 - x % 8) & 1) << n).sum()
            }).collect();
            let levels = 1 << planes;
            (w, h, p, (0..levels).map(|i| grey(levels, i)).collect(), plane * planes)
        }
        Layout::AtariSt => {
            let planes = view.planes.clamp(1, 4);
            let (w, h) = (across * 16, down);
            let row = across * planes * 2;
            let p = (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let word = y * row + (x / 16) * planes * 2;
                let bit = 15 - x % 16;
                (0..planes).map(|n| {
                    let value = u16::from_be_bytes([at(word + n * 2), at(word + n * 2 + 1)]);
                    usize::from(value >> bit & 1) << n
                }).sum()
            }).collect();
            let levels = 1 << planes;
            (w, h, p, (0..levels).map(|i| grey(levels, i)).collect(), row * down)
        }
        Layout::Spectrum => {
            let (w, h) = (256, 192);
            let mut rgba = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                for x in 0..w {
                    let pixel_at = ((y & 0xC0) << 5) | ((y & 7) << 8) | ((y & 0x38) << 2) | (x / 8);
                    let attr = at(6144 + (y / 8) * 32 + x / 8);
                    let on = at(pixel_at) >> (7 - x % 8) & 1 != 0;
                    let colour = if on { attr & 7 } else { (attr >> 3) & 7 };
                    let level = if attr & 0x40 != 0 { 255 } else { 205 };
                    rgba.extend([if colour & 2 != 0 { level } else { 0 }, if colour & 4 != 0 { level } else { 0 }, if colour & 1 != 0 { level } else { 0 }, 255]);
                }
            }
            return Picture { width: w, height: h, rgba, bytes: 6912 };
        }
        Layout::Bytes => {
            let (w, h) = (across, down);
            let mut rgba = Vec::with_capacity(w * h * 4);
            for i in 0..w * h {
                let v = at(i);
                let r = (v >> 5) & 7;
                let g = (v >> 2) & 7;
                let b = ((v & 3) << 1) | u8::from(v & 3 != 0);
                rgba.extend([r * 36 + r / 2, g * 36 + g / 2, b * 36 + b / 2, 255]);
            }
            return Picture { width: w, height: h, rgba, bytes: w * h };
        }
    };
    let mut rgba = Vec::with_capacity(width * height * 4);
    for p in pixels {
        let c = palette.get(p).copied().unwrap_or([255, 0, 255]);
        rgba.extend([c[0], c[1], c[2], 255]);
    }
    Picture { width, height, rgba, bytes: used }
}

/// The view's bytes, as DB lines (or .byte for the 6502's).
pub fn as_source(bytes: &[u8], view: &View, directive: &str) -> String {
    let used = picture(bytes, view).bytes;
    let part = &bytes[view.offset.min(bytes.len())..(view.offset + used).min(bytes.len())];
    let mut out = crate::i18n::tf!("; {} bytes from offset ${:X}, as {}\n", part.len(), view.offset, view.layout.name());
    for chunk in part.chunks(16) {
        out.push_str(&format!("        {directive} {}\n", chunk.iter().map(|b| format!("${b:02X}")).collect::<Vec<_>>().join(",")));
    }
    out
}

#[cfg(test)]
mod tests;
