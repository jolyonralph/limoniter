//! Cartridge images (.crt), as VICE's CRT format lays them out: a 64-byte header naming the machine, the cartridge
//! hardware and its EXROM and GAME lines, then a CHIP packet for each ROM or RAM chip, with its bank and the address
//! it's seen at. Each chip is shown as a file. A plain 8K or 16K cartridge can be made from a binary too.

use super::{Entry, InfoSection, Source, Volume};
use std::sync::Arc;

/// The C64 hardware types, numbered as the CRT format numbers them (VICE's cartridge.h; cartconv --types).
const C64_TYPES: [&str; 86] = [
    "Normal cartridge",
    "Action Replay V5",
    "KCS Power Cartridge",
    "The Final Cartridge III",
    "Simons' BASIC",
    "Ocean",
    "Expert Cartridge",
    "Fun Play, Power Play",
    "Super Games",
    "Atomic Power",
    "Epyx FastLoad",
    "Westermann Learning",
    "REX Utility",
    "The Final Cartridge",
    "Magic Formel",
    "C64 Games System, System 3",
    "Warp Speed",
    "Dinamic",
    "Zaxxon, Super Zaxxon (Sega)",
    "Magic Desk, Domark, HES Australia",
    "Super Snapshot V5",
    "Comal 80",
    "Structured BASIC",
    "ROSS",
    "Dela EP64",
    "Dela EP7x8",
    "Dela EP256",
    "REX 256K EPROM Cart",
    "Mikro Assembler",
    "Final Cartridge Plus",
    "Action Replay MK4",
    "Stardos",
    "EasyFlash",
    "EasyFlash Xbank",
    "Capture",
    "Action Replay MK3",
    "Retro Replay",
    "MMC64",
    "MMC Replay",
    "IDE64",
    "Super Snapshot V4",
    "IEEE-488 Interface",
    "Game Killer",
    "Prophet64",
    "EXOS",
    "Freeze Frame",
    "Freeze Machine",
    "Snapshot 64",
    "Super Explode V5.0",
    "Magic Voice",
    "Action Replay MK2",
    "MACH 5",
    "Diashow-Maker",
    "Pagefox",
    "Kingsoft",
    "Silverrock 128K Cartridge",
    "Formel 64",
    "RGCD",
    "RR-Net MK3",
    "Easy Calc Result",
    "GMod2",
    "MAX Basic",
    "GMod3",
    "ZIPP-CODE 48",
    "Blackbox V8",
    "Blackbox V3",
    "Blackbox V4",
    "REX RAM-Floppy",
    "BIS-Plus",
    "SD-BOX",
    "MultiMAX",
    "Blackbox V9",
    "Lt. Kernal Host Adaptor",
    "RAMLink",
    "Drean",
    "IEEE Flash! 64",
    "HES Turtle Graphics II",
    "Freeze Frame MK2",
    "Partner 64",
    "Hyper-BASIC MK2",
    "Universal Cartridge 1",
    "Universal Cartridge 1.5",
    "Universal Cartridge 2",
    "BMP Data Turbo 2000",
    "Profi-DOS",
    "Magic Desk 16",
];

const VIC20_TYPES: [&str; 6] = ["Generic cartridge", "Mega-Cart", "Behr Bonz", "Vic Flash Plugin", "UltiMem", "Final Expansion"];
const PLUS4_TYPES: [&str; 4] = ["Generic cartridge", "c264 magic cart", "Plus4 multi cart", "1MB Cartridge"];

/// The machines a CRT's signature names.
const SIGNATURES: [(&[u8; 16], &str); 5] = [
    (b"C64 CARTRIDGE   ", "C64"),
    (b"C128 CARTRIDGE  ", "C128"),
    (b"VIC20 CARTRIDGE ", "VIC-20"),
    (b"PLUS4 CARTRIDGE ", "Plus/4"),
    (b"CBM2 CARTRIDGE  ", "CBM-II"),
];

pub fn looks_like(head: &[u8]) -> bool {
    head.len() >= 16 && SIGNATURES.iter().any(|(s, _)| head[..16] == s[..])
}

/// A hardware type's name, for the machine the signature names.
pub fn type_name(machine: &str, kind: u16) -> String {
    let list: &[&str] = match machine {
        "C64" | "C128" => &C64_TYPES,
        "VIC-20" => &VIC20_TYPES,
        "Plus/4" => &PLUS4_TYPES,
        _ => &[],
    };
    match list.get(usize::from(kind)) {
        Some(name) => (*name).to_string(),
        None => format!("Type {kind}, which Limoniter doesn't know"),
    }
}

fn chip_type(kind: u16) -> &'static str {
    match kind {
        0 => "ROM",
        1 => "RAM",
        2 => "Flash ROM",
        3 => "EEPROM",
        _ => "Unknown",
    }
}

/// One CHIP packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub offset: u64,
    pub packet_length: u32,
    pub kind: u16,
    pub bank: u16,
    pub address: u16,
    pub size: u16,
}

pub struct Crt {
    source: Arc<dyn Source>,
    pub machine: &'static str,
    pub version: (u8, u8),
    pub hardware: u16,
    pub subtype: u8,
    pub exrom: u8,
    pub game: u8,
    pub name: String,
    pub chips: Vec<Chip>,
    notes: Vec<String>,
}

impl Crt {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let len = source.len();
        let head = source.read_vec(0, 64.min(len) as usize)?;
        if head.len() < 64 || !looks_like(&head) {
            return Err("This isn't a CRT cartridge image: it doesn't start with a signature such as C64 CARTRIDGE.".into());
        }
        let machine = SIGNATURES.iter().find(|(s, _)| head[..16] == s[..]).map_or("C64", |(_, m)| *m);
        let header_length = u64::from(u32::from_be_bytes([head[16], head[17], head[18], head[19]]));
        let mut notes = Vec::new();
        // Some early CRTs say $20 here, though their headers are $40 long, as VICE allows for.
        let header_length = if header_length < 0x40 {
            notes.push(format!("The header says it's {header_length} bytes long, under the 64 it is: read as 64."));
            0x40
        } else {
            header_length
        };
        let mut crt = Crt {
            source: source.clone(),
            machine,
            version: (head[20], head[21]),
            hardware: u16::from_be_bytes([head[22], head[23]]),
            exrom: head[24],
            game: head[25],
            subtype: head[26],
            name: head[32..64].iter().take_while(|b| **b != 0).map(|&b| if (0x20..0x7F).contains(&b) { char::from(b) } else { '?' }).collect::<String>().trim_end().to_string(),
            chips: Vec::new(),
            notes,
        };
        let mut at = header_length;
        while at + 16 <= len {
            let packet = source.read_vec(at, 16)?;
            if &packet[..4] != b"CHIP" {
                crt.notes.push(format!("{} bytes at offset {at} aren't a CHIP packet, so the rest wasn't read.", len - at));
                break;
            }
            let packet_length = u32::from_be_bytes([packet[4], packet[5], packet[6], packet[7]]);
            let chip = Chip { offset: at, packet_length, kind: u16::from_be_bytes([packet[8], packet[9]]), bank: u16::from_be_bytes([packet[10], packet[11]]), address: u16::from_be_bytes([packet[12], packet[13]]), size: u16::from_be_bytes([packet[14], packet[15]]) };
            if at + 16 + u64::from(chip.size) > len {
                crt.notes.push(format!("The chip for bank {} at ${:04X} is cut short.", chip.bank, chip.address));
            }
            if u64::from(packet_length) != 16 + u64::from(chip.size) {
                crt.notes.push(format!("The packet for bank {} at ${:04X} says it's {packet_length} bytes, where its {}-byte chip makes it {}.", chip.bank, chip.address, chip.size, 16 + u32::from(chip.size)));
            }
            // A packet's own length says where the next starts; one that says too little would go nowhere.
            at += u64::from(packet_length).max(16 + u64::from(chip.size));
            crt.chips.push(chip);
            if crt.chips.len() > 4096 {
                crt.notes.push("It has over 4,096 chips, so the rest weren't read.".into());
                break;
            }
        }
        if crt.chips.is_empty() {
            crt.notes.push("It has no chips: a cartridge with nothing in it.".into());
        }
        Ok(crt)
    }

    pub fn hardware_name(&self) -> String {
        type_name(self.machine, self.hardware)
    }

    /// What the EXROM and GAME lines (low is active, 0) make of the memory map, on a C64.
    pub fn mode(&self) -> &'static str {
        match (self.exrom != 0, self.game != 0) {
            (true, true) => "no cartridge ROM mapped at the start (the hardware banks it in)",
            (false, true) => "8K: ROML at $8000",
            (false, false) => "16K: ROML at $8000, ROMH at $A000",
            (true, false) => "Ultimax: ROML at $8000, ROMH at $E000, the rest of memory mostly gone",
        }
    }

    fn chip_name(chip: &Chip) -> String {
        format!("bank {:03} ${:04X}", chip.bank, chip.address)
    }
}

impl Volume for Crt {
    fn describe(&self) -> String {
        format!("{} cartridge (CRT) \u{B7} {} \u{B7} {}", self.machine, if self.name.is_empty() { "no name" } else { &self.name }, self.hardware_name())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .chips
            .iter()
            .enumerate()
            .map(|(i, chip)| {
                let name = Crt::chip_name(chip);
                let mut entry = Entry::file(name.clone(), u64::from(chip.size), i as u64);
                entry.kind = chip_type(chip.kind).into();
                entry.detail = format!("bank {}, ${:04X}-${:04X}", chip.bank, chip.address, (u32::from(chip.address) + u32::from(chip.size).max(1) - 1).min(0xFFFF));
                entry.open_name = format!("{name}.bin");
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let chip = self.chips.get(entry.location as usize).ok_or("That chip isn't in the cartridge.")?;
        if chip.kind == 1 {
            return Err("This chip is RAM: the image keeps no bytes for it.".into());
        }
        let available = self.source.len().saturating_sub(chip.offset + 16);
        self.source.read_vec(chip.offset + 16, u64::from(chip.size).min(available) as usize)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn info(&self) -> Vec<InfoSection> {
        let line = |v: u8| if v == 0 { "0 (active)" } else { "1 (inactive)" };
        let mut rows = vec![
            ("Name".to_string(), if self.name.is_empty() { "(none)".into() } else { self.name.clone() }),
            ("Machine".into(), self.machine.to_string()),
            ("Hardware".into(), format!("{} (type {})", self.hardware_name(), self.hardware)),
            ("CRT version".into(), format!("{}.{:02}", self.version.0, self.version.1)),
            ("EXROM".into(), line(self.exrom).into()),
            ("GAME".into(), line(self.game).into()),
        ];
        if self.machine == "C64" || self.machine == "C128" {
            rows.push(("Mapped at the start".into(), self.mode().into()));
        }
        if self.subtype != 0 {
            rows.push(("Hardware revision".into(), self.subtype.to_string()));
        }
        let total: u64 = self.chips.iter().map(|c| u64::from(c.size)).sum();
        let banks: std::collections::BTreeSet<u16> = self.chips.iter().map(|c| c.bank).collect();
        rows.push(("Chips".into(), format!("{}, in {} banks, {} bytes", self.chips.len(), banks.len(), total)));
        let chips = self.chips.iter().map(|c| (Crt::chip_name(c), format!("{}, {} bytes, ${:04X}-${:04X}, at offset {}", chip_type(c.kind), c.size, c.address, (u32::from(c.address) + u32::from(c.size).max(1) - 1).min(0xFFFF), c.offset))).collect();
        vec![InfoSection { title: "Cartridge".into(), rows }, InfoSection { title: "Chips".into(), rows: chips }]
    }
}

/// A plain C64 cartridge (type 0) from a binary: 8K at $8000 (EXROM active), or 16K at $8000-$BFFF (EXROM and
/// GAME active), as cartconv makes them: one CHIP packet each.
pub fn normal_cartridge(binary: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let (exrom, game) = match binary.len() {
        8192 => (0u8, 1u8),
        16384 => (0, 0),
        n => return Err(format!("A normal cartridge is 8,192 or 16,384 bytes; this is {n}.")),
    };
    let mut out = Vec::with_capacity(64 + 16 + binary.len());
    out.extend_from_slice(b"C64 CARTRIDGE   ");
    out.extend_from_slice(&0x40u32.to_be_bytes());
    out.extend_from_slice(&[1, 0, 0, 0, exrom, game]);
    out.resize(32, 0);
    let mut title: Vec<u8> = name.bytes().map(|b| if (0x20..0x7F).contains(&b) { b.to_ascii_uppercase() } else { b'?' }).take(32).collect();
    title.resize(32, 0);
    out.extend_from_slice(&title);
    out.extend_from_slice(b"CHIP");
    out.extend_from_slice(&(16 + binary.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0x80, 0x00]);
    out.extend_from_slice(&(binary.len() as u16).to_be_bytes());
    out.extend_from_slice(binary);
    Ok(out)
}

/// The binary to make a cartridge from: the file as it is, or a C64 program loading at $8000 without its load
/// address and padded to 8K or 16K.
pub fn cartridge_binary(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() == 8192 || bytes.len() == 16384 {
        return Ok(bytes.to_vec());
    }
    if bytes.len() > 2 && bytes[..2] == [0x00, 0x80] {
        let mut body = bytes[2..].to_vec();
        let size = if body.len() <= 8192 { 8192 } else if body.len() <= 16384 { 16384 } else { return Err(format!("The program is {} bytes, over the 16K a normal cartridge holds.", body.len())) };
        body.resize(size, 0xFF);
        return Ok(body);
    }
    Err("A cartridge is made from an 8K or 16K binary, or from a program that loads at $8000 (with the CBM80 signature at its start).".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip(kind: u16, bank: u16, address: u16, data: &[u8]) -> Vec<u8> {
        let mut out = b"CHIP".to_vec();
        out.extend((16 + data.len() as u32).to_be_bytes());
        out.extend(kind.to_be_bytes());
        out.extend(bank.to_be_bytes());
        out.extend(address.to_be_bytes());
        out.extend((data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn a_cartridge_lists_its_chips_as_files() {
        let mut bytes = normal_cartridge(&[0xAA; 8192], "game").unwrap()[..64].to_vec();
        bytes[22..24].copy_from_slice(&32u16.to_be_bytes());
        bytes[24] = 0;
        bytes[25] = 1;
        for bank in 0..3u16 {
            bytes.extend(chip(2, bank, 0x8000, &[bank as u8; 8192]));
        }
        bytes.extend(chip(2, 0, 0xA000, &[9; 8192]));
        let crt = Crt::open(Arc::new(bytes)).unwrap();
        assert_eq!(crt.describe(), "C64 cartridge (CRT) \u{B7} GAME \u{B7} EasyFlash");
        let files = crt.list(None).unwrap();
        assert_eq!(files.len(), 4);
        assert_eq!((files[1].name.as_str(), files[1].kind.as_str(), files[1].detail.as_str()), ("bank 001 $8000", "Flash ROM", "bank 1, $8000-$9FFF"));
        assert_eq!(crt.read(&files[2]).unwrap(), vec![2; 8192]);
        assert_eq!(crt.read(&files[3]).unwrap()[0], 9);
        let info = crt.info();
        assert!(info[0].rows.contains(&("Chips".to_string(), "4, in 3 banks, 32768 bytes".to_string())), "{:?}", info[0].rows);
        assert!(info[0].rows.contains(&("Mapped at the start".to_string(), "8K: ROML at $8000".to_string())));
        assert!(crt.notes().is_empty(), "{:?}", crt.notes());
    }

    #[test]
    fn normal_cartridges_are_made_as_cartconv_makes_them() {
        let bin: Vec<u8> = (0..16384u32).map(|i| (i * 7) as u8).collect();
        let crt = normal_cartridge(&bin, "My Cart").unwrap();
        assert_eq!(crt.len(), 64 + 16 + 16384);
        let read = Crt::open(Arc::new(crt)).unwrap();
        assert_eq!((read.hardware, read.exrom, read.game, read.name.as_str()), (0, 0, 0, "MY CART"));
        assert_eq!(read.chips, [Chip { offset: 64, packet_length: 16400, kind: 0, bank: 0, address: 0x8000, size: 16384 }]);
        assert!(normal_cartridge(&[0; 100], "x").is_err());
        let mut prg = vec![0x00, 0x80];
        prg.extend([0x09, 0x80, 0x09, 0x80, 0xC3, 0xC2, 0xCD, 0x38, 0x30]);
        let padded = cartridge_binary(&prg).unwrap();
        assert_eq!((padded.len(), padded[4], padded[8191]), (8192, 0xC3, 0xFF));
        assert!(cartridge_binary(&[0x01, 0x08, 0]).is_err());
        assert_eq!(type_name("C64", 85), "Magic Desk 16");
        assert_eq!(type_name("VIC-20", 1), "Mega-Cart");
        assert!(type_name("C64", 200).contains("200"));
    }

    #[test]
    fn a_cut_short_cartridge_says_so() {
        let mut bytes = normal_cartridge(&[0x11; 8192], "x").unwrap();
        bytes.truncate(64 + 16 + 100);
        let crt = Crt::open(Arc::new(bytes)).unwrap();
        assert!(crt.notes().iter().any(|n| n.contains("cut short")));
        let files = crt.list(None).unwrap();
        assert_eq!(crt.read(&files[0]).unwrap().len(), 100);
    }

    /// With LIMONITER_CRT_DIR naming a folder of .crt files, each is read, and its chips agree with what
    /// `cartconv -f` (VICE's own) says of it, when cartconv is installed.
    #[test]
    fn real_cartridges_agree_with_cartconv() {
        let Some(dir) = std::env::var_os("LIMONITER_CRT_DIR").map(std::path::PathBuf::from) else {
            eprintln!("LIMONITER_CRT_DIR isn't set, so no real cartridges were read.");
            return;
        };
        let mut read = 0;
        let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("crt"))).collect();
        paths.sort();
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let crt = Crt::open(Arc::new(bytes)).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let Ok(out) = std::process::Command::new("cartconv").arg("-f").arg(&path).output() else {
                eprintln!("cartconv isn't installed, so nothing was compared.");
                return;
            };
            let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let name = path.display();
            // "Hardware ID: 5 (Ocean)", "Mode: exrom: 0 game: 0 (16k Game)", and a line per chip:
            // "$000040 CHIP ROM   #000 $8000 $2000 $2010".
            let id = said.lines().find_map(|l| l.strip_prefix("Hardware ID: ")).and_then(|v| v.split(' ').next()).and_then(|v| v.parse::<u16>().ok());
            assert_eq!(id, Some(crt.hardware), "{name}: {said}");
            assert!(said.contains(&format!("exrom: {} game: {}", crt.exrom, crt.game)), "{name}: {said}");
            assert!(said.contains(&format!("Name: {}", crt.name)), "{name}: {said}");
            let chips: Vec<String> = said.lines().filter(|l| l.contains(" CHIP ")).map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase()).collect();
            let mine: Vec<String> = crt.chips.iter().map(|c| format!("${:06X} CHIP {} #{:03} ${:04X} ${:04X} ${:04X}", c.offset, ["ROM", "RAM", "FLASH", "EEPROM"].get(usize::from(c.kind)).copied().unwrap_or("?"), c.bank, c.address, c.size, c.packet_length)).collect();
            assert_eq!(chips, mine, "{name}");
            eprintln!("{name}: agrees with cartconv: {}, {} chips", crt.hardware_name(), crt.chips.len());
            read += 1;
        }
        eprintln!("{read} cartridges compared with cartconv.");
    }

    /// The plain cartridges made here are byte for byte the ones cartconv makes, with LIMONITER_CRT_DIR naming a
    /// folder holding cartconv's n8.crt and n16.crt (from n8.bin and n16.bin, named NORMAL 8K and NORMAL 16K).
    #[test]
    fn normal_cartridges_are_made_as_cartconv_makes_them_byte_for_byte() {
        let Some(dir) = std::env::var_os("LIMONITER_CRT_DIR").map(std::path::PathBuf::from) else {
            return;
        };
        for (bin, crt, name) in [("n8.bin", "n8.crt", "NORMAL 8K"), ("n16.bin", "n16.crt", "NORMAL 16K")] {
            let (Ok(bin), Ok(theirs)) = (std::fs::read(dir.join(bin)), std::fs::read(dir.join(crt))) else {
                continue;
            };
            assert_eq!(normal_cartridge(&bin, name).unwrap(), theirs, "{crt}");
            eprintln!("{crt}: the same bytes as cartconv's");
        }
    }
}
