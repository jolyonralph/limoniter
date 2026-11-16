//! Memory cards from two Z80 portables, as their emulators keep them: one image of the whole card.
//!
//! - **Cambridge Z88** EPROM and flash cards (`.epr`): a file card has `oz` in its last two bytes, a 64-byte header
//!   below them saying its size in 16K banks, and its files from the bottom of the card up, one after another, each
//!   a name's length, the name (starting `/` for a file's latest version, or a zero byte where an older version has
//!   been replaced), a 4-byte length, low byte first, and the file; then &FF bytes to the header. An application card
//!   has `OZ` there instead, and its applications are described by the operating system's own structures. From the
//!   Z88 Developers' Notes, Appendix F ("EPROM format") and section 3.1.
//! - **Amstrad NC100, NC150 and NC200** PCMCIA cards (`.card`): ZCN's CP/M file system, as zcntools (nc100em) reads
//!   it: each 256K of the card a drive, A: to D:, starting with a 1K boot block (`ZCN1` at byte 2, the drive's size
//!   in K at 6, the system blocks after it at 8, the directory's 1K blocks at 9), then the directory of 32-byte CP/M
//!   entries and 1K data blocks numbered from the directory's first. A card that holds a program for the NC's own
//!   operating system instead has `NC100PRG` at &200, and is described rather than listed.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

const BANK: usize = 16 * 1024;
const ZCN_DRIVE: usize = 256 * 1024;
/// The most of a card read: Z88 cards go to 1 MB, NC cards to 1 MB too.
const MOST_CARD: u64 = 4 * 1024 * 1024;

/// Whether an image is an NC card in ZCN's format, or holding an NC program.
pub fn looks_like_nc(head: &[u8], len: u64) -> bool {
    (32 * 1024..=MOST_CARD).contains(&len) && (head.get(2..6) == Some(b"ZCN1") || head.get(0x200..0x208) == Some(b"NC100PRG"))
}

#[derive(Clone)]
struct Z88File {
    name: String,
    start: usize,
    length: usize,
    latest: bool,
}

enum Kind {
    Z88Files { banks: usize, id: [u8; 4], files: Vec<Z88File>, free: usize },
    Z88Applications { banks: usize },
    /// The ZCN drives there are, each with its directory's entries: (user, name, extent, records, blocks).
    Zcn { drives: Vec<ZcnDrive> },
    NcProgram { name: String },
}

struct ZcnDrive {
    letter: char,
    base: usize,
    system_blocks: usize,
    files: Vec<ZcnFile>,
}

struct ZcnFile {
    user: u8,
    name: String,
    /// Its 1K blocks in order, and how many 128-byte records the last extent has.
    blocks: Vec<u8>,
    size: usize,
}

pub struct Card {
    bytes: Arc<Vec<u8>>,
    kind: Kind,
    notes: Vec<String>,
}

impl Card {
    pub fn open(source: Arc<dyn Source>, ending: &str) -> Result<Self, String> {
        if source.len() > MOST_CARD {
            return Err("This is larger than any Z88 or NC memory card.".into());
        }
        let bytes = Arc::new(source.read_vec(0, source.len() as usize)?);
        let (kind, notes) = if ending == "epr" || bytes.ends_with(b"oz") || bytes.ends_with(b"OZ") { z88(&bytes)? } else { nc(&bytes)? };
        Ok(Self { bytes, kind, notes })
    }
}

fn z88(bytes: &[u8]) -> Result<(Kind, Vec<String>), String> {
    if bytes.len() < BANK || !bytes.len().is_multiple_of(BANK) {
        return Err("A Z88 card image is a whole number of 16K banks.".into());
    }
    let top = &bytes[bytes.len() - BANK..];
    let banks = usize::from(top[0x3FFC]).max(1);
    let mut notes = Vec::new();
    if banks * BANK != bytes.len() {
        notes.push(format!("The card says it's {} banks ({}K), and the image is {}K.", banks, banks * 16, bytes.len() / 1024));
    }
    if &top[0x3FFE..] == b"OZ" {
        notes.push("This is an application card: its applications are found by the Z88's own structures, which aren't read here. Its bytes can be seen in hex.".into());
        return Ok((Kind::Z88Applications { banks }, notes));
    }
    if &top[0x3FFE..] != b"oz" {
        return Err("This isn't a Z88 card: there's no oz or OZ at its very top.".into());
    }
    let id = [top[0x3FF8], top[0x3FF9], top[0x3FFA], top[0x3FFB]];
    // The files run up from the bottom of the card to the header at the top of its last bank.
    let end = bytes.len() - 0x40;
    let mut files = Vec::new();
    let mut at = 0;
    while at < end && bytes[at] != 0xFF {
        let name_length = usize::from(bytes[at]);
        if name_length < 2 || at + 1 + name_length + 4 > end {
            notes.push(format!("The files stop making sense at byte &{at:X}, so the list stops there."));
            break;
        }
        let name_bytes = &bytes[at + 1..at + 1 + name_length];
        let latest = name_bytes[0] == b'/';
        let name: String = name_bytes[1..].iter().map(|b| if b.is_ascii_graphic() { char::from(*b) } else { '?' }).collect();
        let length_at = at + 1 + name_length;
        let length = u32::from_le_bytes([bytes[length_at], bytes[length_at + 1], bytes[length_at + 2], bytes[length_at + 3]]) as usize;
        let start = length_at + 4;
        if start + length > end {
            notes.push(format!("{name} says it's {length} bytes, more than the card has left, so the list stops there."));
            break;
        }
        files.push(Z88File { name, start, length, latest });
        at = start + length;
    }
    let free = end.saturating_sub(at);
    Ok((Kind::Z88Files { banks, id, files, free }, notes))
}

fn nc(bytes: &[u8]) -> Result<(Kind, Vec<String>), String> {
    if bytes.get(0x200..0x208) == Some(b"NC100PRG") {
        // After the mark, a jump at &210, then the program's name.
        let name: String = bytes[0x213..0x220.min(bytes.len())].iter().take_while(|b| **b != 0).map(|b| if b.is_ascii_graphic() || *b == b' ' { char::from(*b) } else { '?' }).collect();
        let notes = vec!["This card holds a program the NC runs from its card, starting at &C210. Its bytes can be seen in hex.".into()];
        return Ok((Kind::NcProgram { name: name.trim().to_string() }, notes));
    }
    let mut drives = Vec::new();
    let mut notes = Vec::new();
    for (index, base) in (0..bytes.len()).step_by(ZCN_DRIVE).enumerate().take(4) {
        let boot = &bytes[base..(base + 1024).min(bytes.len())];
        if boot.get(2..6) != Some(b"ZCN1") {
            continue;
        }
        let letter = char::from(b'A' + index as u8);
        let system_blocks = usize::from(boot[8]);
        let directory_blocks = usize::from(boot[9]).max(1);
        let directory = base + 1024 * (1 + system_blocks);
        let mut files: Vec<ZcnFile> = Vec::new();
        for entry in (0..directory_blocks * 32).map(|n| directory + n * 32) {
            let Some(raw) = bytes.get(entry..entry + 32) else {
                break;
            };
            if raw[0] == 0xE5 || raw[0] > 15 {
                continue;
            }
            let name = cpm_name(&raw[1..12]);
            let extent = usize::from(raw[12]) + 32 * usize::from(raw[14]);
            let records = usize::from(raw[15]);
            // A drive is at most 256K: an extent past that, or a count past a whole one, isn't a real entry.
            if extent >= 16 || records > 128 {
                continue;
            }
            let blocks: Vec<u8> = raw[16..32].iter().copied().take_while(|b| *b != 0).collect();
            let file = match files.iter_mut().find(|f| f.user == raw[0] && f.name == name) {
                Some(file) => file,
                None => {
                    files.push(ZcnFile { user: raw[0], name, blocks: Vec::new(), size: 0 });
                    files.last_mut().expect("just pushed")
                }
            };
            // Each extent is 16K: its blocks go at their place in the file.
            let place = extent * 16;
            if file.blocks.len() < place + blocks.len() {
                file.blocks.resize(place + blocks.len(), 0);
            }
            file.blocks[place..place + blocks.len()].copy_from_slice(&blocks);
            file.size = file.size.max(extent * 16384 + records * 128);
        }
        drives.push(ZcnDrive { letter, base, system_blocks, files });
    }
    if drives.is_empty() {
        return Err("This card has no ZCN drive on it. Cards the NC's own software keeps files on aren't read yet.".into());
    }
    if !bytes.len().is_multiple_of(ZCN_DRIVE) && bytes.len() > ZCN_DRIVE {
        notes.push("The card isn't a whole number of 256K drives; the last part is read as far as it goes.".into());
    }
    Ok((Kind::Zcn { drives }, notes))
}

/// A CP/M 8.3 name, its attribute bits off.
fn cpm_name(raw: &[u8]) -> String {
    let part = |bytes: &[u8]| bytes.iter().map(|b| char::from(b & 0x7F)).collect::<String>().trim_end().to_string();
    let (stem, ending) = (part(&raw[..8]), part(&raw[8..11]));
    if ending.is_empty() { stem } else { format!("{stem}.{ending}") }
}

impl Volume for Card {
    fn describe(&self) -> String {
        match &self.kind {
            Kind::Z88Files { banks, files, .. } => format!("Cambridge Z88 file card, {}K, {} files", banks * 16, files.iter().filter(|f| f.latest).count()),
            Kind::Z88Applications { banks } => format!("Cambridge Z88 application card, {}K", banks * 16),
            Kind::Zcn { drives } => format!("Amstrad NC memory card, ZCN, {}K, drive{} {}", self.bytes.len() / 1024, if drives.len() == 1 { "" } else { "s" }, drives.iter().map(|d| format!("{}:", d.letter)).collect::<Vec<_>>().join(" ")),
            Kind::NcProgram { name } if name.is_empty() => "Amstrad NC program card".into(),
            Kind::NcProgram { name } => format!("Amstrad NC program card: {name}"),
        }
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        match &self.kind {
            Kind::Z88Files { files, .. } => Ok(files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    let mut entry = Entry::file(tidy_name(&file.name), file.length as u64, index as u64);
                    entry.kind = if file.latest { "File".into() } else { "Older version".into() };
                    entry.detail = format!("at &{:05X}", file.start);
                    entry
                })
                .collect()),
            Kind::Zcn { drives } => match dir {
                None if drives.len() > 1 => Ok(drives.iter().enumerate().map(|(index, d)| Entry::dir(format!("{}:", d.letter), index as u64, 0)).collect()),
                None => Ok(zcn_entries(&drives[0], 0)),
                Some(folder) => drives.get(folder.location as usize).map(|d| zcn_entries(d, folder.location)).ok_or_else(|| "That drive isn't on the card.".to_string()),
            },
            Kind::Z88Applications { .. } | Kind::NcProgram { .. } => Ok(Vec::new()),
        }
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        match &self.kind {
            Kind::Z88Files { files, .. } => {
                let file = files.get(entry.location as usize).ok_or("That file isn't on the card.")?;
                Ok(self.bytes[file.start..file.start + file.length].to_vec())
            }
            Kind::Zcn { drives } => {
                let drive = drives.get((entry.location >> 16) as usize).ok_or("That drive isn't on the card.")?;
                let file = drive.files.get((entry.location & 0xFFFF) as usize).ok_or("That file isn't on the card.")?;
                let mut out = Vec::with_capacity(file.size);
                for block in &file.blocks {
                    let at = drive.base + 1024 * (usize::from(*block) + 1 + drive.system_blocks);
                    let chunk = self.bytes.get(at..at + 1024).ok_or("A block of the file is past the end of the card.")?;
                    out.extend_from_slice(chunk);
                }
                out.truncate(file.size);
                Ok(out)
            }
            Kind::Z88Applications { .. } | Kind::NcProgram { .. } => Err("There are no files to read on this card.".into()),
        }
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if let Kind::Z88Files { id, free, .. } = &self.kind {
            notes.push(format!("Card ID {:02X} {:02X} {:02X} {:02X}; {}K free.", id[0], id[1], id[2], id[3], free / 1024));
        }
        notes
    }

    fn reading_bytes(&self, _entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        // ZCN's files are CP/M's, whose text ends at a Ctrl+Z.
        if matches!(self.kind, Kind::Zcn { .. }) { super::cpm::cut_at_ctrl_z(bytes) } else { bytes }
    }
}

fn zcn_entries(drive: &ZcnDrive, number: u64) -> Vec<Entry> {
    drive
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let mut entry = Entry::file(tidy_name(&file.name), file.size as u64, (number << 16) | index as u64);
            entry.kind = "CP/M file".into();
            if file.user != 0 {
                entry.detail = format!("user {}", file.user);
            }
            entry
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z88_card(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut card = vec![0xFF; 2 * BANK];
        let mut at = 0;
        for (name, body, latest) in files {
            card[at] = name.len() as u8 + 1;
            card[at + 1] = if *latest { b'/' } else { 0 };
            card[at + 2..at + 2 + name.len()].copy_from_slice(name.as_bytes());
            at += 2 + name.len();
            card[at..at + 4].copy_from_slice(&(body.len() as u32).to_le_bytes());
            card[at + 4..at + 4 + body.len()].copy_from_slice(body);
            at += 4 + body.len();
        }
        let top = card.len() - 0x40;
        card[top..].fill(0);
        card[top + 0x37] = 1;
        card[top + 0x38..top + 0x3C].copy_from_slice(&[1, 2, 3, 4]);
        card[top + 0x3C] = 2;
        card[top + 0x3D] = 0x7E;
        card[top + 0x3E..].copy_from_slice(b"oz");
        card
    }

    #[test]
    fn a_z88_file_card_lists_its_files_and_their_older_versions() {
        let card = z88_card(&[("NOTES.TXT", b"first", false), ("NOTES.TXT", b"second", true), ("GAME.BAS", &[1, 2, 3], true)]);
        let volume = Card::open(Arc::new(card), "epr").unwrap();
        assert_eq!(volume.describe(), "Cambridge Z88 file card, 32K, 2 files");
        let entries = volume.list(None).unwrap();
        assert_eq!(entries.iter().map(|e| (e.name.as_str(), e.kind.as_str())).collect::<Vec<_>>(), [("NOTES.TXT", "Older version"), ("NOTES.TXT", "File"), ("GAME.BAS", "File")]);
        assert_eq!(volume.read(&entries[1]).unwrap(), b"second");
        assert!(volume.notes().iter().any(|n| n.starts_with("Card ID 01 02 03 04")));
    }

    #[test]
    fn a_z88_application_card_is_told_from_a_file_card() {
        let mut card = vec![0; BANK];
        card[BANK - 2..].copy_from_slice(b"OZ");
        card[0x3FFC] = 1;
        let volume = Card::open(Arc::new(card), "epr").unwrap();
        assert_eq!(volume.describe(), "Cambridge Z88 application card, 16K");
        assert!(volume.list(None).unwrap().is_empty());
    }

    #[test]
    fn an_nc_card_in_zcns_format_lists_its_cpm_files() {
        // As zcntools formats a 256K card: the boot block, then two blocks of directory.
        let mut card = vec![0u8; ZCN_DRIVE];
        card[..2].copy_from_slice(&[0xC9, 0x7E]);
        card[2..6].copy_from_slice(b"ZCN1");
        card[6] = 0;
        card[7] = 1;
        card[8] = 0;
        card[9] = 2;
        card[1024..3072].fill(0xE5);
        let entry = &mut card[1024..1056];
        entry.fill(0);
        entry[1..12].copy_from_slice(b"HELLO   TXT");
        entry[12] = 0;
        entry[15] = 2;
        entry[16] = 2;
        // Block 2 is the first after the directory's two.
        let data = 1024 * (2 + 1);
        card[data..data + 6].copy_from_slice(b"Hello\x1A");
        assert!(looks_like_nc(&card, card.len() as u64));
        let volume = Card::open(Arc::new(card), "card").unwrap();
        assert_eq!(volume.describe(), "Amstrad NC memory card, ZCN, 256K, drive A:");
        let entries = volume.list(None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "HELLO.TXT");
        assert_eq!(entries[0].size, 256);
        let bytes = volume.read(&entries[0]).unwrap();
        assert_eq!(volume.reading_bytes(&entries[0], bytes), b"Hello");
    }

    #[test]
    fn an_nc_program_card_says_what_it_is() {
        let mut card = vec![0u8; 64 * 1024];
        card[0x200..0x208].copy_from_slice(b"NC100PRG");
        card[0x210] = 0xC3;
        card[0x213..0x21A].copy_from_slice(b"SD BOOT");
        let volume = Card::open(Arc::new(card), "card").unwrap();
        assert_eq!(volume.describe(), "Amstrad NC program card: SD BOOT");
    }
}
