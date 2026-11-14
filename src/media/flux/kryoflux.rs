//! KryoFlux stream files, as the Software Preservation Society's board writes them: one file for each track and
//! side, named like `track00.0.raw`, so a disk is a folder of them. Each is the board's own stream: flux values as
//! counts of its sample clock (about 24 MHz) in one, two or three bytes, with no-op and overflow codes among them,
//! and out-of-band blocks (starting &0D) for the stream's position, each index pulse, the board's information
//! ("sck=" gives its sample clock) and the end.

use super::super::{Entry, Source, Volume, zip::Zip};
use super::Flux;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The board's usual sample clock: 18.432 MHz × 73 / 14 / 2.
const SAMPLE_CLOCK: f64 = 18_432_000.0 * 73.0 / 14.0 / 2.0;

/// Whether a file's first bytes are a stream's: an out-of-band block of one of the first four kinds.
pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 4 && head[0] == 0x0D && (1..=4).contains(&head[1]) && u16::from_le_bytes([head[2], head[3]]) < 0x1000
}

/// The track and side a stream file's name gives, and the part of the name before them: `track` for
/// `track00.0.raw`.
pub fn place_of(name: &str) -> Option<(String, (u8, u8))> {
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".raw")?;
    let bytes = stem.as_bytes();
    let n = bytes.len();
    if n < 4 || bytes[n - 2] != b'.' || !matches!(bytes[n - 1], b'0' | b'1') || !bytes[n - 4].is_ascii_digit() || !bytes[n - 3].is_ascii_digit() {
        return None;
    }
    let track = (bytes[n - 4] - b'0') * 10 + bytes[n - 3] - b'0';
    Some((name[..n - 4].to_string(), (track, bytes[n - 1] - b'0')))
}

/// A stream file's flux, its gaps in nanoseconds, and anything the board reported going wrong.
pub fn decode(stream: &[u8]) -> Result<(Flux, Option<String>), String> {
    let mut ticks = Vec::with_capacity(stream.len());
    let mut sample_clock = SAMPLE_CLOCK;
    let mut indexes = 0;
    let mut trouble = None;
    let mut overflow = 0u64;
    let mut i = 0;
    while i < stream.len() {
        let code = stream[i];
        match code {
            0x00..=0x07 => {
                let Some(&low) = stream.get(i + 1) else { break };
                ticks.push(overflow + (u64::from(code) << 8 | u64::from(low)));
                overflow = 0;
                i += 2;
            }
            0x08..=0x0A => i += usize::from(code - 7),
            0x0B => {
                overflow += 0x10000;
                i += 1;
            }
            0x0C => {
                let Some(pair) = stream.get(i + 1..i + 3) else { break };
                ticks.push(overflow + u64::from(u16::from_be_bytes([pair[0], pair[1]])));
                overflow = 0;
                i += 3;
            }
            0x0D => {
                let Some(head) = stream.get(i + 1..i + 4) else { break };
                let (kind, size) = (head[0], usize::from(u16::from_le_bytes([head[1], head[2]])));
                // The end of the file: its size bytes are &0D &0D, not a size.
                if kind == 0x0D {
                    break;
                }
                let body = stream.get(i + 4..i + 4 + size).ok_or("A KryoFlux stream ends partway through one of its blocks, so it's damaged.")?;
                match kind {
                    0x02 => indexes += 1,
                    // The stream's end, and whether the board kept up: 1 is a buffer overrun, 2 no index found.
                    0x03 if size >= 8 => match u32::from_le_bytes(body[4..8].try_into().unwrap_or_default()) {
                        0 => {}
                        1 => trouble = Some("the board's buffer overflowed while reading".to_string()),
                        2 => trouble = Some("no index pulse was seen".to_string()),
                        other => trouble = Some(format!("the board reported problem {other}")),
                    },
                    0x04 => {
                        let text = String::from_utf8_lossy(body);
                        if let Some(value) = text.split(',').filter_map(|part| part.trim().strip_prefix("sck=")).next()
                            && let Ok(clock) = value.trim_end_matches('\0').trim().parse::<f64>()
                            && (1e6..1e9).contains(&clock)
                        {
                            sample_clock = clock;
                        }
                    }
                    _ => {}
                }
                i += 4 + size;
            }
            _ => {
                ticks.push(overflow + u64::from(code));
                overflow = 0;
                i += 1;
            }
        }
    }
    let scale = 1e9 / sample_clock;
    let gaps = ticks.into_iter().map(|t| (t as f64 * scale) as f32).collect();
    Ok((Flux { gaps, revolutions: indexes.max(1) - 1 }, trouble))
}

/// Where a set's stream files are.
pub enum Files {
    /// In a folder, by track and side.
    Folder(BTreeMap<(u8, u8), PathBuf>),
    /// In a ZIP archive.
    Zip(Box<Zip>, BTreeMap<(u8, u8), Entry>),
    /// One stream on its own.
    Single((u8, u8), Arc<dyn Source>),
}

impl Files {
    pub fn places(&self) -> Vec<(u8, u8)> {
        match self {
            Files::Folder(files) => files.keys().copied().collect(),
            Files::Zip(_, files) => files.keys().copied().collect(),
            Files::Single(place, _) => vec![*place],
        }
    }

    pub fn read(&self, place: (u8, u8)) -> Result<Vec<u8>, String> {
        match self {
            Files::Folder(files) => std::fs::read(files.get(&place).ok_or("That track isn't in the set.")?).map_err(|e| e.to_string()),
            Files::Zip(zip, files) => zip.read(files.get(&place).ok_or("That track isn't in the set.")?),
            Files::Single(_, source) => source.read_vec(0, source.len() as usize),
        }
    }
}

/// The set a stream file belongs to: every file beside it with the same name before its track and side.
pub fn folder_set(path: &Path) -> Option<Files> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let (prefix, _) = place_of(&name)?;
    let folder = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut files = BTreeMap::new();
    for item in std::fs::read_dir(folder).ok()?.flatten() {
        let other = item.file_name().to_string_lossy().into_owned();
        if let Some((other_prefix, place)) = place_of(&other)
            && other_prefix.eq_ignore_ascii_case(&prefix)
        {
            files.insert(place, item.path());
        }
    }
    (!files.is_empty()).then_some(Files::Folder(files))
}

/// The set in a ZIP archive, when it holds one disk's streams: two or more stream files, all in one folder and
/// with one name before their tracks. An archive of several disks' streams is left to open as an archive.
pub fn zip_set(source: Arc<dyn Source>) -> Option<Files> {
    let zip = Zip::open(source).ok()?;
    let mut sets: BTreeMap<String, BTreeMap<(u8, u8), Entry>> = BTreeMap::new();
    let mut folders: Vec<(Option<Entry>, String, usize)> = vec![(None, String::new(), 0)];
    while let Some((folder, path, depth)) = folders.pop() {
        for entry in zip.list(folder.as_ref()).ok()? {
            if entry.is_dir {
                if depth < 4 {
                    folders.push((Some(entry.clone()), format!("{path}{}/", entry.name), depth + 1));
                }
            } else if let Some((prefix, place)) = place_of(&entry.name) {
                sets.entry(format!("{path}{}", prefix.to_ascii_lowercase())).or_default().insert(place, entry);
            }
        }
    }
    if sets.len() != 1 {
        return None;
    }
    let files = sets.into_values().next()?;
    (files.len() >= 2).then(|| Files::Zip(Box::new(zip), files))
}

#[cfg(test)]
pub(super) mod tests {
    /// A KryoFlux stream of gaps in nanoseconds, with the board's information block, index blocks and its end.
    pub fn stream_of(gaps: &[f32], revolutions: usize) -> Vec<u8> {
        let clock = super::SAMPLE_CLOCK;
        let mut out = Vec::new();
        let info = b"host_date=2026.11.13, name=KryoFlux DiskSystem, sck=24027428.5714285, ick=3003428.5714285625\0";
        out.extend([0x0D, 0x04]);
        out.extend((info.len() as u16).to_le_bytes());
        out.extend(info);
        for _ in 0..revolutions {
            out.extend([0x0D, 0x02, 12, 0]);
            out.extend([0u8; 12]);
            for gap in gaps {
                let mut ticks = (f64::from(*gap) * clock / 1e9).round() as u64;
                while ticks > 0xFFFF {
                    out.push(0x0B);
                    ticks -= 0x10000;
                }
                match ticks {
                    0x0E..=0xFF => out.push(ticks as u8),
                    0..=0x7FF => out.extend([(ticks >> 8) as u8, ticks as u8]),
                    _ => {
                        out.push(0x0C);
                        out.extend((ticks as u16).to_be_bytes());
                    }
                }
                // A no-op now and then, as the board pads its buffers.
                if ticks.is_multiple_of(97) {
                    out.push(0x08);
                }
            }
        }
        out.extend([0x0D, 0x02, 12, 0]);
        out.extend([0u8; 12]);
        out.extend([0x0D, 0x03, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend([0x0D, 0x0D, 0x0D, 0x0D]);
        out
    }

    #[test]
    fn stream_names_give_their_tracks() {
        assert_eq!(super::place_of("track07.1.raw"), Some(("track".into(), (7, 1))));
        assert_eq!(super::place_of("Disk 1 79.0.RAW"), Some(("Disk 1 ".into(), (79, 0))));
        assert_eq!(super::place_of("Whole disk (sectors).raw"), None);
        assert_eq!(super::place_of("track7.1.raw"), None);
    }
}
