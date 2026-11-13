//! sjasmplus's virtual devices: the memory of a ZX Spectrum 48K, 128K (and the larger clones) or Next, in pages
//! mapped into the Z80's 64K by slots, as `DEVICE`, `SLOT`, `PAGE`, `MMU` and `ORG address, page` set them up. Code
//! assembled goes into the pages, and the files the source asks for (SAVEBIN, SAVENEX, SAVESNA, SAVETAP, SAVE3DOS)
//! are written from them, as sjasmplus 1.24 writes them.
//!
//! A Spectrum's memory starts as sjasmplus sets it (as after CLEAR 23899: the attributes, system variables, the
//! UDGs and a small stack), so a snapshot saved from it starts like one loaded from tape.

/// What happens when code runs off the end of a slot (MMU's e, w and n).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotOption {
    None,
    Error,
    Warning,
    /// The next page is mapped into the slot, and the address goes back to its start.
    Next,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub slot_size: i64,
    /// The page mapped into each slot.
    pub slots: Vec<usize>,
    pub options: Vec<SlotOption>,
    pub initial: Vec<usize>,
    pub pages: usize,
    pub memory: Vec<u8>,
    /// The slot PAGE changes.
    pub current: usize,
    pub ramtop: i64,
    /// The slot written to last, and its option, to tell when writing runs into another.
    previous: usize,
    previous_option: SlotOption,
    reported: bool,
}

const ZX_RAMTOP: i64 = 0x5D5B;

const ZX_SYSVARS: [u8; 256] = [
    0xff, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x14, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x06, 0x00, 0x0b, 0x00, 0x01, 0x00, 0x01, 0x00, 0x06, 0x00, 0x10, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x40, 0x00, 0xff, 0xcc, 0x01, 0x54, 0xff, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0xcb, 0x5c, 0x00, 0x00, 0xb6, 0x5c, 0xb6, 0x5c, 0xcb, 0x5c, 0x00, 0x00, 0xca, 0x5c, 0xcc, 0x5c, 0xcc, 0x5c, 0xcc, 0x5c, 0x00, //
    0x00, 0xce, 0x5c, 0xce, 0x5c, 0xce, 0x5c, 0x00, 0x92, 0x5c, 0x10, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x58, 0xff, 0x00, 0x00, 0x21, //
    0x5b, 0x00, 0x21, 0x17, 0x00, 0x40, 0xe0, 0x50, 0x21, 0x18, 0x21, 0x17, 0x01, 0x38, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x57, 0xff, 0xff, 0xff, 0xf4, 0x09, 0xa8, 0x10, 0x4b, 0xf4, 0x09, 0xc4, 0x15, 0x53, //
    0x81, 0x0f, 0xc4, 0x15, 0x52, 0xf4, 0x09, 0xc4, 0x15, 0x50, 0x80, 0x80, 0x0d, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The return address left on the stack: the ROM's, after a USR call from BASIC.
pub const ZX_STACK: [u8; 4] = [0x03, 0x13, 0x00, 0x3e];

/// The UDGs A to U, as the ROM sets them.
const ZX_UDGS: [u8; 168] = [
    0x00, 0x3c, 0x42, 0x42, 0x7e, 0x42, 0x42, 0x00, 0x00, 0x7c, 0x42, 0x7c, 0x42, 0x42, 0x7c, 0x00, 0x00, 0x3c, 0x42, 0x40, 0x40, 0x42, 0x3c, 0x00, 0x00, 0x78, 0x44, 0x42, 0x42, 0x44, 0x78, 0x00, //
    0x00, 0x7e, 0x40, 0x7c, 0x40, 0x40, 0x7e, 0x00, 0x00, 0x7e, 0x40, 0x7c, 0x40, 0x40, 0x40, 0x00, 0x00, 0x3c, 0x42, 0x40, 0x4e, 0x42, 0x3c, 0x00, 0x00, 0x42, 0x42, 0x7e, 0x42, 0x42, 0x42, 0x00, //
    0x00, 0x3e, 0x08, 0x08, 0x08, 0x08, 0x3e, 0x00, 0x00, 0x02, 0x02, 0x02, 0x42, 0x42, 0x3c, 0x00, 0x00, 0x44, 0x48, 0x70, 0x48, 0x44, 0x42, 0x00, 0x00, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7e, 0x00, //
    0x00, 0x42, 0x66, 0x5a, 0x42, 0x42, 0x42, 0x00, 0x00, 0x42, 0x62, 0x52, 0x4a, 0x46, 0x42, 0x00, 0x00, 0x3c, 0x42, 0x42, 0x42, 0x42, 0x3c, 0x00, 0x00, 0x7c, 0x42, 0x42, 0x7c, 0x40, 0x40, 0x00, //
    0x00, 0x3c, 0x42, 0x42, 0x52, 0x4a, 0x3c, 0x00, 0x00, 0x7c, 0x42, 0x42, 0x7c, 0x44, 0x42, 0x00, 0x00, 0x3c, 0x40, 0x3c, 0x02, 0x42, 0x3c, 0x00, 0x00, 0xfe, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00, //
    0x00, 0x42, 0x42, 0x42, 0x42, 0x42, 0x3c, 0x00,
];

impl Device {
    /// A device by its sjasmplus name, with RAMTOP for a Spectrum (None for the usual, Some(-1)... see DEVICE).
    pub fn new(name: &str, ramtop: Option<i64>) -> Option<Device> {
        let upper = name.trim().to_ascii_uppercase();
        let zx128 = [7, 5, 2, 0];
        let (slot_size, pages, initial, zx): (i64, usize, Vec<usize>, bool) = match upper.as_str() {
            "ZXSPECTRUM48" => (0x4000, 4, vec![0, 1, 2, 3], true),
            "ZXSPECTRUM128" => (0x4000, 8, zx128.to_vec(), true),
            "ZXSPECTRUM256" => (0x4000, 16, zx128.to_vec(), true),
            "ZXSPECTRUM512" => (0x4000, 32, zx128.to_vec(), true),
            "ZXSPECTRUM1024" => (0x4000, 64, zx128.to_vec(), true),
            "ZXSPECTRUM2048" => (0x4000, 128, zx128.to_vec(), true),
            "ZXSPECTRUM4096" => (0x4000, 256, zx128.to_vec(), true),
            "ZXSPECTRUM8192" => (0x4000, 512, zx128.to_vec(), true),
            "ZXSPECTRUMNEXT" => (0x2000, 224, vec![14, 15, 10, 11, 4, 5, 0, 1], false),
            "NOSLOT64K" => (0x10000, 32, vec![0], false),
            "AMSTRADCPC464" => (0x4000, 4, vec![0, 1, 2, 3], false),
            "AMSTRADCPC6128" => (0x4000, 8, vec![0, 1, 2, 3], false),
            _ => return None,
        };
        let slots = initial.len();
        let ramtop = match ramtop {
            Some(-1) => -1,
            Some(top) if (0x5D00..=0xFFFF).contains(&top) => top,
            _ => ZX_RAMTOP,
        };
        let mut device = Device {
            name: upper,
            slot_size,
            slots: initial.clone(),
            options: vec![SlotOption::None; slots],
            initial,
            pages,
            memory: vec![0; slot_size as usize * pages],
            current: slots - 1,
            ramtop: if zx { ramtop } else { 0 },
            previous: 0,
            previous_option: SlotOption::None,
            reported: false,
        };
        if zx && ramtop != -1 {
            // As after CLEAR RAMTOP: LOAD "" CODE: RANDOMIZE USR start.
            for address in 0x5800..0x5B00 {
                device.poke(address, 0x38);
            }
            for (i, byte) in ZX_UDGS.iter().enumerate() {
                device.poke(0xFF58 + i as i64, *byte);
            }
            for (i, byte) in ZX_SYSVARS.iter().enumerate() {
                device.poke(0x5C00 + i as i64, *byte);
            }
            device.poke(0x5C00 + 0xB2, ramtop as u8);
            device.poke(0x5C00 + 0xB3, (ramtop >> 8) as u8);
            let stack = ramtop + 1 - ZX_STACK.len() as i64;
            device.poke(0x5C00 + 0x3D, stack as u8);
            device.poke(0x5C00 + 0x3E, (stack >> 8) as u8);
            for (i, byte) in ZX_STACK.iter().enumerate() {
                device.poke(stack + i as i64, *byte);
            }
        }
        Some(device)
    }

    pub fn is_spectrum(&self) -> bool {
        self.name.starts_with("ZXSPECTRUM") && self.name != "ZXSPECTRUMNEXT"
    }

    pub fn slot_of(&self, address: i64) -> Option<usize> {
        let slot = (address / self.slot_size) as usize;
        (0..0x10000).contains(&address).then_some(slot).filter(|s| *s < self.slots.len())
    }

    /// The page at an address in the 64K, as it's mapped now.
    pub fn page_at(&self, address: i64) -> Option<usize> {
        self.slot_of(address).map(|slot| self.slots[slot])
    }

    /// Where an address is in the device's whole memory.
    fn offset(&self, address: i64) -> Option<usize> {
        let slot = self.slot_of(address)?;
        Some(self.slots[slot] * self.slot_size as usize + (address % self.slot_size) as usize)
    }

    pub fn poke(&mut self, address: i64, value: u8) {
        if let Some(at) = self.offset(address) {
            self.memory[at] = value;
        }
    }

    pub fn peek(&self, address: i64) -> u8 {
        self.offset(address & 0xFFFF).map_or(0, |at| self.memory[at])
    }

    /// Bytes from the 64K as it's mapped now.
    pub fn read(&self, address: i64, length: usize) -> Vec<u8> {
        (0..length as i64).map(|i| self.peek(address + i)).collect()
    }

    /// A page's bytes, or several pages' in a row, from an offset in the first.
    pub fn page_bytes(&self, page: usize, offset: usize, length: usize) -> Option<&[u8]> {
        let start = page * self.slot_size as usize + offset;
        self.memory.get(start..start + length)
    }

    /// Maps a page into the slot PAGE and ORG work on.
    pub fn set_page(&mut self, page: i64) -> Result<(), String> {
        if page < 0 || page as usize >= self.pages {
            return Err(format!("page {page} isn't one of the device's 0 to {}", self.pages - 1));
        }
        self.slots[self.current] = page as usize;
        self.reset_tracking(None);
        Ok(())
    }

    /// A slot by its number, or by the exact address it starts at.
    pub fn slot_number(&self, value: i64) -> i64 {
        if (0..self.slots.len() as i64).contains(&value) {
            return value;
        }
        if value >= 0 && value % self.slot_size == 0 && value / self.slot_size < self.slots.len() as i64 {
            return value / self.slot_size;
        }
        value
    }

    /// Forgets which slot was written to, as ORG and the paging directives do, so writing on doesn't count as
    /// running off the slot.
    pub fn reset_tracking(&mut self, address: Option<i64>) {
        let slot = address.and_then(|a| self.slot_of(a & 0xFFFF)).unwrap_or(self.previous);
        self.previous = slot;
        self.previous_option = self.options[slot];
        self.reported = false;
    }

    /// Writes a byte where code is being put. The address may come back changed: when MMU's n option is on a slot
    /// and the byte filled it, the next page is mapped into the slot and the address goes back to its start, as
    /// sjasmplus does at once (so a label after the last byte is already in the next page). A problem is reported
    /// once.
    pub fn emit(&mut self, address: i64, value: u8) -> (i64, Option<String>) {
        let mut problem = None;
        let slot = self.slot_of(address);
        if slot != Some(self.previous) {
            if matches!(self.previous_option, SlotOption::Error | SlotOption::Warning) && !self.reported {
                problem = Some(format!("code at ${address:04X} runs outside its memory slot"));
                self.reported = true;
            }
            if let Some(s) = slot {
                let carried = self.reported && problem.is_some();
                self.previous = s;
                self.previous_option = self.options[s];
                self.reported = carried;
            }
        }
        match self.offset(address) {
            Some(at) => self.memory[at] = value,
            None if !self.reported => {
                problem.get_or_insert_with(|| format!("${address:X} is outside the device's 64K"));
                self.reported = true;
            }
            None => {}
        }
        let mut next = address + 1;
        if let Some(s) = slot
            && self.options[s] == SlotOption::Next
            && next == (s as i64 + 1) * self.slot_size
        {
            if self.slots[s] + 1 < self.pages {
                self.slots[s] += 1;
                next -= self.slot_size;
            } else {
                problem.get_or_insert_with(|| "there are no more pages to map into the slot".to_string());
                self.options[s] = SlotOption::None;
            }
        }
        (next, problem)
    }
}

/// A file the source asked to be written, with what's in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The name as the source gives it, from the folder of the file assembled.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// SAVEBIN's bytes: from an address in the 64K as mapped now, a length (0 or less: to the end).
pub fn save_bin(device: &Device, start: i64, length: i64) -> Result<Vec<u8>, String> {
    if !(-0x10000..=0xFFFF).contains(&start) {
        return Err("SAVEBIN's start must be in $0000 to $FFFF".into());
    }
    let start = if start < 0 { start + 0x10000 } else { start };
    let mut length = if length <= 0 { length + 0x10000 - start } else { length };
    if length <= 0 {
        return Err("SAVEBIN has nothing to save".into());
    }
    if start + length > 0x10000 {
        length = 0x10000 - start;
    }
    Ok(device.read(start, length as usize))
}

/// A +3DOS file of bytes from the 64K: SAVE3DOS's type (3 for code), and its two header words.
pub fn save_3dos(device: &Device, address: i64, size: i64, kind: i64, w2: Option<i64>, w3: Option<i64>) -> Result<Vec<u8>, String> {
    if address < 0 || size < 1 || address + size > 0x10000 {
        return Err("SAVE3DOS's address and size go outside the 64K".into());
    }
    let w3 = w3.unwrap_or(size);
    let w2 = match kind {
        0 => w2.unwrap_or(0x8000),
        1 | 2 => w2.unwrap_or(-1),
        3 => w2.unwrap_or(address),
        _ => return Err("SAVE3DOS's type is 0 to 3".into()),
    };
    let mut header = b"PLUS3DOS\x1A\x01\x00".to_vec();
    header.resize(128, 0);
    header[11..15].copy_from_slice(&((128 + size) as u32).to_le_bytes());
    header[15] = kind as u8;
    header[16..18].copy_from_slice(&(size as u16).to_le_bytes());
    header[18..20].copy_from_slice(&(w2 as u16).to_le_bytes());
    header[20..22].copy_from_slice(&(w3 as u16).to_le_bytes());
    let sum = header[..127].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
    header[127] = sum;
    header.extend(device.read(address, size as usize));
    Ok(header)
}

/// A snapshot of a Spectrum device (SAVESNA), started at `start`: a 48K one for ZXSPECTRUM48, a 128K one for the
/// others. Memory where the default stack was, if it's still as it was, gets the start address pushed on it.
pub fn save_sna(device: &mut Device, start: i64) -> Result<Vec<u8>, String> {
    if !device.is_spectrum() {
        return Err("SAVESNA needs DEVICE ZXSPECTRUM48 or ZXSPECTRUM128".into());
    }
    let mut header: [u8; 31] = [
        0x3F, 0x58, 0x27, 0x9B, 0x36, 0x00, 0x00, 0x44, 0x00, 0x2B, 0x2D, 0xDC, 0x5C, 0x00, 0x00, //
        0x3A, 0x5C, 0x3C, 0xFF, 0x00, 0x00, 0x54, 0x00, 0x00, 0x00, 0x01, 0x07, //
        0x00, 0x00, 0x00, 0x00,
    ];
    header[13] = start as u8;
    header[14] = (start >> 8) as u8;
    let is48 = device.name == "ZXSPECTRUM48";
    let mut stack = device.ramtop + 1 - ZX_STACK.len() as i64;
    if stack < 0x4002 {
        stack = 0x4002;
    }
    // The stack is checked in the pages first mapped, whatever's mapped now.
    let initial_at = |device: &Device, address: i64| -> usize {
        let slot = (address / device.slot_size) as usize;
        device.initial[slot] * device.slot_size as usize + (address % device.slot_size) as usize
    };
    let mut default_stack = true;
    for i in if is48 { -2i64 } else { 0 }..ZX_STACK.len() as i64 {
        let expected = if i >= 0 { ZX_STACK[i as usize] } else { 0 };
        default_stack &= device.memory[initial_at(device, stack + i)] == expected;
    }
    if default_stack {
        if is48 {
            stack -= 1;
            let at = initial_at(device, stack);
            device.memory[at] = (start >> 8) as u8;
            stack -= 1;
            let at = initial_at(device, stack);
            device.memory[at] = start as u8;
        }
        header[23] = stack as u8;
        header[24] = (stack >> 8) as u8;
    } else if is48 {
        header[23] = 0x00;
        header[24] = 0x40;
        let page = device.slot_size as usize;
        device.memory[page] = start as u8;
        device.memory[page + 1] = (start >> 8) as u8;
    } else {
        header[23] = 0x00;
        header[24] = 0x60;
    }
    let mut out = header[..27].to_vec();
    let size = device.slot_size as usize;
    let top = device.slots[3];
    let pages: [usize; 3] = if is48 { [1, 2, 3] } else { [5, 2, top] };
    for page in pages {
        out.extend_from_slice(&device.memory[page * size..(page + 1) * size]);
    }
    if !is48 {
        out.extend([start as u8, (start >> 8) as u8, 0x10 + top as u8, 0]);
        for page in 0..8 {
            if page != top && page != 2 && page != 5 {
                out.extend_from_slice(&device.memory[page * size..(page + 1) * size]);
            }
        }
    }
    Ok(out)
}

/// A tape header block and data block (SAVETAP in its BASIC, CODE, NUMBERS and CHARS forms), or a data block on
/// its own (HEADLESS), from memory as mapped now.
pub fn tap_block(device: &Device, kind: &str, name: Option<&str>, start: i64, length: Option<i64>, param2: Option<i64>, param3: Option<i64>) -> Result<Vec<u8>, String> {
    let mut length = length.unwrap_or(-1);
    if length + start > 0x10000 {
        length = -1;
    }
    if length <= 0 {
        length = 0x10000 - start;
    }
    let (flag, p2, p3) = match kind {
        "basic" => (0u8, param2.unwrap_or(0x8000), param3.unwrap_or(length)),
        "numbers" | "chars" => {
            let base = if kind == "chars" { 0xC0 } else { 0x80 };
            let letter = param2.filter(|p| *p > 0).unwrap_or(1) & 0x1F;
            (if kind == "chars" { 2 } else { 1 }, (letter | base) << 8, 0x8000)
        }
        "code" => (3, param2.unwrap_or(start), param3.unwrap_or(0x8000)),
        _ => (param3.unwrap_or(0xFF) as u8, 0, 0),
    };
    let block = |flag: u8, data: &[u8]| -> Vec<u8> {
        let mut out = ((data.len() + 2) as u16).to_le_bytes().to_vec();
        out.push(flag);
        out.extend_from_slice(data);
        out.push(data.iter().fold(flag, |sum, b| sum ^ b));
        out
    };
    let mut out = Vec::new();
    let data_flag = if kind == "headless" {
        flag
    } else {
        if let Some(name) = name {
            let mut header = vec![flag];
            let mut title: Vec<u8> = name.bytes().take(10).collect();
            title.resize(10, b' ');
            header.extend(title);
            header.extend((length as u16).to_le_bytes());
            header.extend((p2 as u16).to_le_bytes());
            header.extend((p3 as u16).to_le_bytes());
            out.extend(block(0, &header));
        }
        0xFF
    };
    out.extend(block(data_flag, &device.read(start, length as usize)));
    Ok(out)
}

/// The order a .nex keeps its 16K banks in: 5, 2, 0, 1, 3, 4, 6, 7, then 8 on.
fn nex_index(bank: i64) -> Option<i64> {
    if (8..112).contains(&bank) {
        return Some(bank);
    }
    [5, 2, 0, 1, 3, 4, 6, 7].iter().position(|b| *b == bank).map(|i| i as i64)
}

fn nex_bank(index: i64) -> Option<i64> {
    match index {
        0..=7 => Some([5, 2, 0, 1, 3, 4, 6, 7][index as usize]),
        8..=111 => Some(index),
        _ => None,
    }
}

/// A .nex being made, by SAVENEX's commands in turn, as sjasmplus makes one.
#[derive(Clone, Debug)]
pub struct Nex {
    pub name: String,
    header: Vec<u8>,
    data: Vec<u8>,
    last_bank: i64,
    palette: Option<Vec<u8>>,
    palette_set: bool,
    copper: Option<Vec<u8>>,
    banks_offset: Option<usize>,
    version: u8,
    wanted_version: i64,
}

impl Nex {
    pub fn open(name: String, start: i64, stack: i64, entry_bank: i64, version: i64) -> Result<Nex, String> {
        if !(0..112).contains(&entry_bank) {
            return Err("SAVENEX's entry bank is 0 to 111".into());
        }
        if version != 0 && !(2..=3).contains(&version) {
            return Err("SAVENEX's file version is 2 (V1.2) or 3 (V1.3)".into());
        }
        let mut header = vec![0u8; 512];
        header[..8].copy_from_slice(b"NextV1.2");
        header[12..14].copy_from_slice(&(stack as u16).to_le_bytes());
        header[14..16].copy_from_slice(&(start as u16).to_le_bytes());
        // The minimum core: 2.0.28.
        header[135..138].copy_from_slice(&[2, 0, 28]);
        header[139] = entry_bank as u8;
        Ok(Nex { name, header, data: Vec::new(), last_bank: -1, palette: None, palette_set: false, copper: None, banks_offset: None, version: if version == 3 { 3 } else { 2 }, wanted_version: version })
    }

    fn needs_v13(&mut self, what: &str) -> Result<(), String> {
        if self.wanted_version == 2 {
            return Err(format!("{what} is a NEX V1.3 feature, and OPEN asked for V1.2"));
        }
        self.version = 3;
        Ok(())
    }

    pub fn core(&mut self, major: i64, minor: i64, sub: i64) {
        self.header[135..138].copy_from_slice(&[major as u8, minor as u8, sub as u8]);
    }

    pub fn cfg(&mut self, border: i64, handle: Option<i64>, preserve: Option<i64>, ram: Option<i64>) {
        self.header[11] = (border & 7) as u8;
        self.header[140..142].copy_from_slice(&(handle.unwrap_or(0) as u16).to_le_bytes());
        self.header[134] = preserve.unwrap_or(0) as u8;
        self.header[8] = ram.unwrap_or(0) as u8;
    }

    pub fn cfg3(&mut self, crc: i64, preserve_bus: Option<i64>, cli: Option<i64>, cli_size: Option<i64>) -> Result<(), String> {
        self.needs_v13("CFG3")?;
        self.header[143] = (crc != 0) as u8;
        self.header[142] = (preserve_bus.unwrap_or(0) != 0) as u8;
        self.header[148..150].copy_from_slice(&(cli.unwrap_or(0) as u16).to_le_bytes());
        self.header[150..152].copy_from_slice(&(cli_size.unwrap_or(0) as u16).to_le_bytes());
        Ok(())
    }

    pub fn bar(&mut self, on: i64, colour: i64, start_delay: Option<i64>, bank_delay: Option<i64>, pos_y: Option<i64>) {
        self.header[130] = on as u8;
        self.header[131] = colour as u8;
        self.header[133] = start_delay.unwrap_or(0) as u8;
        self.header[132] = bank_delay.unwrap_or(0) as u8;
        self.header[158] = pos_y.unwrap_or(254) as u8;
    }

    fn screen_flags(&self) -> u8 {
        self.header[10]
    }

    pub fn palette_default(&mut self) -> Result<(), String> {
        self.palette_check()?;
        self.palette_set = true;
        self.palette = Some((0..256u32).flat_map(|i| [i as u8, u8::from(i & 3 != 0)]).collect());
        Ok(())
    }

    pub fn palette_none(&mut self) -> Result<(), String> {
        self.palette_check()?;
        self.palette_set = true;
        Ok(())
    }

    pub fn palette_mem(&mut self, device: &Device, page: i64, offset: i64) -> Result<(), String> {
        self.palette_check()?;
        self.palette_from(device, page, offset)
    }

    fn palette_check(&self) -> Result<(), String> {
        if self.palette_set || self.screen_flags() != 0 {
            return Err("a palette or screen was already given (give the palette once, before the screen)".into());
        }
        if self.last_bank != -1 {
            return Err("a bank was already stored (give the palette before them)".into());
        }
        Ok(())
    }

    /// A palette from memory, unless one was set already; page -1 means none.
    fn palette_from(&mut self, device: &Device, page: i64, offset: i64) -> Result<(), String> {
        if self.palette_set {
            return Ok(());
        }
        self.palette_set = true;
        if page == -1 {
            return Ok(());
        }
        let bytes = device.page_bytes(page as usize, offset as usize, 512).ok_or("the palette is outside the Next's memory")?;
        self.palette = Some(bytes.to_vec());
        Ok(())
    }

    /// The palette written ahead of a screen, or the "no palette" flag.
    fn write_palette(&mut self) {
        match (&self.palette, self.palette_set) {
            (Some(palette), true) => self.data.extend_from_slice(palette),
            _ => self.header[10] = 0x80,
        }
    }

    fn screen_check(&self) -> Result<(), String> {
        if self.screen_flags() != 0 {
            return Err("the NEX file already has its screen".into());
        }
        if self.last_bank != -1 {
            return Err("a bank was already stored (store the screen first)".into());
        }
        Ok(())
    }

    /// SCREEN L2, LR, L2_320 or L2_640: pixels from a page and offset (or the usual place), with a palette.
    pub fn screen_bitmap(&mut self, device: &Device, kind: &str, page: Option<i64>, offset: i64, palette: Option<(i64, i64)>) -> Result<(), String> {
        self.screen_check()?;
        if matches!(kind, "l2_320" | "l2_640") {
            self.needs_v13("A 320x256 or 640x256 screen")?;
            self.header[138] = 0;
        }
        let size = match kind {
            "l2" => 0xC000,
            "lr" => 0x3000,
            _ => 0x14000,
        };
        if let Some(page) = page
            && device.page_bytes(page as usize, offset as usize, size).is_none()
        {
            return Err("the screen's pixels are outside the Next's memory".into());
        }
        let (palette_page, palette_offset) = palette.unwrap_or((-1, 0));
        self.palette_from(device, palette_page, palette_offset)?;
        self.write_palette();
        match kind {
            "l2" => self.header[10] |= 0x01,
            "lr" => self.header[10] |= 0x04,
            "l2_320" => {
                self.header[10] |= 0x40;
                self.header[152] = 1;
            }
            _ => {
                self.header[10] |= 0x40;
                self.header[152] = 2;
            }
        }
        match page {
            Some(page) => self.data.extend_from_slice(device.page_bytes(page as usize, offset as usize, size).expect("checked")),
            None if kind == "lr" => {
                // LoRes from where it shows: bank 5's two halves.
                self.data.extend_from_slice(device.page_bytes(10, 0, 0x1800).expect("bank 5"));
                self.data.extend_from_slice(device.page_bytes(11, 0, 0x1800).expect("bank 5"));
            }
            None => self.data.extend_from_slice(device.page_bytes(18, 0, size).ok_or("the screen's pages are outside the Next's memory")?),
        }
        Ok(())
    }

    /// SCREEN SCR, SHC or SHR: the ULA or Timex screen from bank 5, as it's displayed.
    pub fn screen_ula(&mut self, device: &Device, kind: &str, colour: Option<i64>) -> Result<(), String> {
        self.screen_check()?;
        let flag = match kind {
            "scr" => 0x02,
            "shc" => 0x10,
            _ => 0x08,
        };
        if flag == 0x08
            && let Some(colour) = colour
        {
            self.header[138] = ((colour & 7) << 3) as u8;
        }
        self.header[10] = flag;
        let size = if flag == 0x02 { 0x1B00 } else { 0x1800 };
        self.data.extend_from_slice(device.page_bytes(10, 0, size).expect("bank 5"));
        if flag != 0x02 {
            self.data.extend_from_slice(device.page_bytes(11, 0, size).expect("bank 5"));
        }
        Ok(())
    }

    /// SCREEN TILE: the tilemap's registers, and bank 5 stored as the first bank unless asked not to.
    pub fn screen_tile(&mut self, device: &Device, registers: [i64; 4], store_bank5: bool) -> Result<(), String> {
        self.screen_check()?;
        self.needs_v13("A tilemap screen")?;
        self.write_palette();
        self.header[10] |= 0x40;
        self.header[152] = 3;
        for (i, value) in registers.iter().enumerate() {
            self.header[154 + i] = *value as u8;
        }
        if store_bank5 {
            self.save_bank(device, 5, false)?;
        }
        Ok(())
    }

    pub fn copper(&mut self, device: &Device, page: i64, offset: i64) -> Result<(), String> {
        if self.wanted_version == 2 {
            return Err("COPPER is a NEX V1.3 feature, and OPEN asked for V1.2".into());
        }
        if self.last_bank != -1 {
            return Err("a bank was already stored (store the copper code first)".into());
        }
        self.version = 3;
        let bytes = device.page_bytes(page as usize, offset as usize, 0x800).ok_or("the copper code is outside the Next's memory")?;
        self.header[153] = 1;
        self.copper = Some(bytes.to_vec());
        Ok(())
    }

    /// Before the first bank: the copper code, and where the banks start.
    fn before_banks(&mut self) {
        if self.banks_offset.is_some() {
            return;
        }
        if let Some(copper) = &self.copper {
            self.data.extend_from_slice(copper);
        }
        self.banks_offset = Some(512 + self.data.len());
    }

    /// Stores a 16K bank, in the order a .nex keeps them; one of only zeros is left out if asked.
    pub fn save_bank(&mut self, device: &Device, bank: i64, only_non_zero: bool) -> Result<(), String> {
        let index = nex_index(bank).ok_or("SAVENEX's banks are 0 to 111")?;
        if index <= self.last_bank {
            return Err(format!("it's too late to store bank {bank}: they go in the order 5, 2, 0, 1, 3, 4, 6, 7, 8..."));
        }
        self.before_banks();
        self.last_bank = index;
        let bytes = device.page_bytes(bank as usize * 2, 0, 0x4000).ok_or("that bank is outside the Next's memory")?;
        if only_non_zero && bytes.iter().all(|b| *b == 0) {
            return Ok(());
        }
        self.header[18 + bank as usize] = 1;
        self.header[9] += 1;
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    /// AUTO: every bank with something in it, from the one after the last stored (or as given).
    pub fn auto(&mut self, device: &Device, from: Option<i64>, to: Option<i64>) -> Result<(), String> {
        if self.last_bank == 111 {
            return Err("every bank is stored already".into());
        }
        let from = from.unwrap_or_else(|| nex_bank(self.last_bank + 1).unwrap_or(111));
        let to = to.unwrap_or(111);
        let (Some(first), Some(last)) = (nex_index(from), nex_index(to)) else {
            return Err("SAVENEX AUTO's banks are 0 to 111".into());
        };
        if last < first {
            return Err("SAVENEX AUTO's last bank comes before its first".into());
        }
        for index in first..=last {
            self.save_bank(device, nex_bank(index).expect("in range"), true)?;
        }
        Ok(())
    }

    /// CLOSE: the header finished, and any files to append after the banks.
    pub fn close(mut self, appended: &[Vec<u8>]) -> Result<Vec<u8>, String> {
        self.before_banks();
        for bytes in appended {
            self.data.extend_from_slice(bytes);
        }
        if self.version == 3 {
            self.header[7] = b'3';
            self.header[144..148].copy_from_slice(&(self.banks_offset.unwrap_or(0) as u32).to_le_bytes());
        } else {
            self.header[144..148].fill(0);
            self.header[158] = 0;
        }
        let mut out = self.header.clone();
        out.extend(&self.data);
        if self.version == 3 && out[143] != 0 {
            // CRC-32C of the file after the header, then of the header's first 508 bytes.
            let crc = crc32c(crc32c(0, &out[512..]), &out[..508]);
            out[508..512].copy_from_slice(&crc.to_le_bytes());
        }
        if self.header[8] == 0 && self.header[18 + 48..18 + 112].iter().any(|b| *b != 0) {
            return Err("banks 48 to 111 are stored, which need the 2MB Next: say so with SAVENEX CFG's fourth value".into());
        }
        Ok(out)
    }
}

/// CRC-32C (Castagnoli), carried on from an earlier part.
fn crc32c(crc: u32, bytes: &[u8]) -> u32 {
    let mut crc = !crc;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x82F6_3B78 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_start_with_sjasmplus_s_pages_and_spectrum_memory() {
        let next = Device::new("ZXSPECTRUMNEXT", None).unwrap();
        assert_eq!(next.slots, [14, 15, 10, 11, 4, 5, 0, 1]);
        assert_eq!(next.current, 7);
        assert_eq!(next.memory.len(), 224 * 8192);
        let zx = Device::new("zxspectrum48", None).unwrap();
        assert_eq!(zx.peek(0x5800), 0x38, "attributes: INK 0, PAPER 7");
        assert_eq!(zx.peek(0x5C00 + 0xB2) as i64 | (zx.peek(0x5C00 + 0xB3) as i64) << 8, 0x5D5B, "RAMTOP");
        assert_eq!(zx.read(0x5D58, 4), ZX_STACK);
        assert_eq!(crc32c(0, b"123456789"), 0xE306_9283, "the CRC-32C check value");
    }

    #[test]
    fn mmu_n_maps_the_next_page_when_code_runs_off_a_slot() {
        let mut next = Device::new("ZXSPECTRUMNEXT", None).unwrap();
        next.slots[6] = 20;
        next.options[6] = SlotOption::Next;
        next.reset_tracking(Some(0xDFFF));
        let (address, problem) = next.emit(0xDFFF, 1);
        assert_eq!((address, problem, next.slots[6]), (0xC000, None, 21), "page 21 mapped at once, and the address back at the slot's start");
        let (address, _) = next.emit(address, 2);
        assert_eq!(address, 0xC001);
        assert_eq!(next.page_bytes(20, 0x1FFF, 1).unwrap(), [1]);
        assert_eq!(next.page_bytes(21, 0, 1).unwrap(), [2]);
    }
}
