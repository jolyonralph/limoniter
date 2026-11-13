//! ZX Spectrum Next programs (.nex, versions 1.0 to 1.3), read as what they hold: the header's settings, the
//! loading screen and its palette, copper code, each 16K bank of memory, and anything appended after them, each
//! opened as a file of its own. The loading screen is also the file's picture.
//!
//! After the 512-byte header: the palette (unless the header says there's none), the screens in the order Layer 2,
//! ULA, LoRes, Timex HiRes, Timex HiColour, then V1.3's 320×256 or 640×256 Layer 2; the copper code; then the
//! banks the header lists, in the order 5, 2, 0, 1, 3, 4, 6, 7, then 8 on.

use super::{Entry, InfoSection, Source, Volume};
use std::sync::Arc;

const ORDER: [usize; 8] = [5, 2, 0, 1, 3, 4, 6, 7];

struct Part {
    name: String,
    kind: String,
    detail: String,
    at: u64,
    length: u64,
}

pub struct Nex {
    source: Arc<dyn Source>,
    header: Vec<u8>,
    parts: Vec<Part>,
    /// The loading screen's file, by its index among the parts, to draw it.
    screen: Option<usize>,
    notes: Vec<String>,
}

pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 8 && head.starts_with(b"NextV1.")
}

impl Nex {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let header = source.read_vec(0, 512.min(source.len() as usize))?;
        if header.len() < 512 || !looks_like(&header) {
            return Err("This isn't a .nex file: it has no \"NextV1\" header.".into());
        }
        let screen = header[10];
        let screen2 = header[152];
        let version = header[7];
        // What the file holds, in order: (name, kind, detail, length, whether it's the loading screen).
        let mut plan: Vec<(String, String, String, u64, bool)> = Vec::new();
        let mut add = |name: &str, kind: &str, detail: &str, length: u64, is_screen: bool| plan.push((name.into(), kind.into(), detail.into(), length, is_screen));
        if screen & 0x80 == 0 && screen & (0x01 | 0x04 | 0x40) != 0 {
            add("palette.nxp", "Palette", "256 colours, 9 bits each", 512, false);
        }
        if screen & 0x01 != 0 {
            add("loading.nxi", "Loading screen, Layer 2", "256\u{D7}192, 256 colours", 49152, true);
        }
        if screen & 0x02 != 0 {
            add("loading.scr", "Loading screen, ULA", "256\u{D7}192, the Spectrum's", 6912, true);
        }
        if screen & 0x04 != 0 {
            add("loading.slr", "Loading screen, LoRes", "128\u{D7}96, 256 colours", 12288, true);
        }
        if screen & 0x08 != 0 {
            add("loading.shr", "Loading screen, Timex HiRes", "512\u{D7}192, two colours", 12288, false);
        }
        if screen & 0x10 != 0 {
            add("loading.shc", "Loading screen, Timex HiColour", "256\u{D7}192, 8\u{D7}1 attributes", 12288, false);
        }
        if screen & 0x40 != 0 && matches!(screen2, 1 | 2) {
            if screen2 == 1 {
                add("loading.sl2", "Loading screen, Layer 2", "320\u{D7}256, 256 colours", 81920, true);
            } else {
                add("loading.nxi640", "Loading screen, Layer 2", "640\u{D7}256, 16 colours", 81920, true);
            }
        }
        if header[153] != 0 {
            add("copper.bin", "Copper code", "2048 bytes, started as the screen shows", 2048, false);
        }
        for bank in ORDER.iter().copied().chain(8..112).filter(|b| header[18 + b] != 0) {
            let detail = match bank {
                5 => "$4000 to $7FFF".to_string(),
                2 => "$8000 to $BFFF".to_string(),
                0 => "$C000 to $FFFF, as it starts".to_string(),
                b => format!("8K pages {} and {}", b * 2, b * 2 + 1),
            };
            add(&format!("bank {bank}.bin"), "16K bank", &detail, 16384, false);
        }
        let mut parts = Vec::new();
        let mut screen_part = None;
        let mut at = 512u64;
        for (name, kind, detail, length, is_screen) in plan {
            if is_screen {
                screen_part = Some(parts.len());
            }
            parts.push(Part { name, kind, detail, at, length });
            at += length;
        }
        let mut notes = Vec::new();
        if at > source.len() {
            notes.push("The file ends before all the header says it holds; the last parts may be cut short.".into());
            parts.retain(|p| p.at < source.len());
            if let Some(last) = parts.last_mut() {
                last.length = last.length.min(source.len() - last.at);
            }
        } else if source.len() > at {
            let rest = source.len() - at;
            parts.push(Part { name: "appended.bin".into(), kind: "Appended data".into(), detail: format!("{rest} bytes after the banks, for the program to read"), at, length: rest });
        }
        let _ = version;
        Ok(Self { source, header, parts, screen: screen_part, notes })
    }

    fn word(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.header[at], self.header[at + 1]])
    }

    fn palette(&self) -> Option<Vec<u8>> {
        let part = self.parts.iter().find(|p| p.name == "palette.nxp")?;
        self.source.read_vec(part.at, part.length as usize).ok()
    }
}

impl Volume for Nex {
    fn describe(&self) -> String {
        let start = self.word(14);
        format!("ZX Spectrum Next program (NEX {}) \u{B7} starts at ${start:04X}", String::from_utf8_lossy(&self.header[4..8]))
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .parts
            .iter()
            .enumerate()
            .map(|(index, part)| {
                let mut entry = Entry::file(part.name.clone(), part.length, index as u64);
                entry.kind = part.kind.clone();
                entry.detail = part.detail.clone();
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let part = self.parts.get(entry.location as usize).ok_or("That part isn't in the file.")?;
        let bytes = self.source.read_vec(part.at, part.length as usize)?;
        // A Layer 2 or LoRes screen comes with its palette in front, as its own files have it.
        if Some(entry.location as usize) == self.screen
            && matches!(part.name.as_str(), "loading.nxi" | "loading.slr" | "loading.sl2" | "loading.nxi640")
            && let Some(mut palette) = self.palette()
        {
            palette.extend(bytes);
            return Ok(palette);
        }
        Ok(bytes)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn info(&self) -> Vec<InfoSection> {
        let h = &self.header;
        let yes = |b: u8| if b != 0 { "Yes" } else { "No" };
        let mut rows = vec![
            ("Version".to_string(), String::from_utf8_lossy(&h[4..8]).into_owned()),
            ("Starts at".into(), format!("${:04X}{}", self.word(14), if self.word(14) == 0 { " (doesn't start: a file of data)" } else { "" })),
            ("Stack".into(), format!("${:04X}", self.word(12))),
            ("Bank at $C000".into(), h[139].to_string()),
            ("Needs core".into(), format!("{}.{}.{}", h[135], h[136], h[137])),
            ("Needs 2MB".into(), yes(h[8]).into()),
            ("Banks".into(), h[9].to_string()),
            ("Border while loading".into(), h[11].to_string()),
            ("Loading bar".into(), if h[130] != 0 { format!("Yes, colour {}", h[131]) } else { "No".into() }),
            ("Delays".into(), format!("{} frames before starting, {} after each bank", h[133], h[132])),
            ("Next registers".into(), if h[134] != 0 { "Kept as they are".into() } else { "Reset".into() }),
            ("File handle".into(), match self.word(140) {
                0 => "Closed".into(),
                1 => "Kept open, in BC".into(),
                at => format!("Kept open, written to ${at:04X}"),
            }),
        ];
        if h[7] == b'3' {
            rows.push(("Checksum".into(), yes(h[143]).into()));
            rows.push(("Expansion bus".into(), if h[142] != 0 { "Left alone".into() } else { "Turned off".into() }));
            if self.word(150) != 0 {
                rows.push(("Command line".into(), format!("{} bytes at ${:04X}", self.word(150), self.word(148))));
            }
        }
        vec![InfoSection { title: "Next program".into(), rows }]
    }

    fn picture(&self) -> Option<Vec<u8>> {
        let index = self.screen?;
        let part = &self.parts[index];
        let entry = Entry::file(part.name.clone(), part.length, index as u64);
        let bytes = self.read(&entry).ok()?;
        let picture = match part.name.as_str() {
            "loading.scr" => crate::pictures::decode(crate::pictures::Format::SpectrumScreen, &bytes).ok()?,
            "loading.nxi640" => crate::pictures::next::decode("nxi640", &bytes).ok()?,
            "loading.slr" => crate::pictures::next::decode("slr", &bytes).ok()?,
            _ => crate::pictures::next::decode("nxi", &bytes).ok()?,
        };
        crate::pictures::to_png(&picture).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{open, tests::names};
    use super::*;

    #[test]
    fn a_nex_lists_its_screen_banks_and_settings() {
        // As sjasmplus makes one: a Layer 2 screen with its palette, and banks 5 and 2.
        let mut file = vec![0u8; 512];
        file[..8].copy_from_slice(b"NextV1.2");
        file[9] = 2;
        file[10] = 0x01;
        file[12..14].copy_from_slice(&0xFF00u16.to_le_bytes());
        file[14..16].copy_from_slice(&0x8000u16.to_le_bytes());
        file[18 + 5] = 1;
        file[18 + 2] = 1;
        file.extend(crate::pictures::next::default_palette());
        file.extend(vec![0xE0u8; 49152]);
        file.extend(vec![5u8; 16384]);
        file.extend(vec![2u8; 16384]);
        file.extend(b"extra");
        let volume = open(Arc::new(file), "game.nex").unwrap();
        assert!(volume.describe().contains("starts at $8000"), "{}", volume.describe());
        let entries = volume.list(None).unwrap();
        assert_eq!(names(&entries), ["palette.nxp", "loading.nxi", "bank 5.bin", "bank 2.bin", "appended.bin"]);
        assert_eq!(volume.read(&entries[2]).unwrap(), vec![5u8; 16384]);
        assert_eq!(volume.read(&entries[1]).unwrap().len(), 512 + 49152, "the screen with its palette in front");
        assert_eq!(volume.read(&entries[4]).unwrap(), b"extra");
        assert!(volume.picture().is_some(), "the loading screen is the file's picture");
        let info = volume.info();
        assert!(info[0].rows.iter().any(|(l, v)| l == "Stack" && v == "$FF00"));
    }
}
