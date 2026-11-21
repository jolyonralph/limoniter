//! Workbench icons: the .info files the Amiga's Workbench shows a program, file, drawer or disk by. Four kinds
//! are read and shown as their pictures:
//!
//! - the original icon, a DiskObject (workbench.i in the NDK) with one or two planar images, as every Workbench
//!   since 1.0 reads it;
//! - NewIcons (1990s), a palette picture packed into tool types named IM1= and IM2=, kept by older Workbenches as
//!   ordinary tool types;
//! - GlowIcons (OS 3.5 and later), an IFF FORM ICON appended after the original icon, with a palette picture for
//!   each state;
//! - OS 4's PNG icons, a PNG (or two, the second for when it's selected) with the icon's details in an icOn chunk.
//!
//! What can be changed (the kind of icon, its default tool, tool types, stack and place) is changed in the parts
//! that hold it, and nothing else: an icon whose details aren't changed is written back byte for byte as it was.

use crate::pictures::Picture;

/// What every classic icon starts with: WB_DISKMAGIC ($E310) and WB_DISKVERSION (1).
const MAGIC: [u8; 4] = [0xE3, 0x10, 0x00, 0x01];
/// The DiskObject as it's written (do_SIZEOF): the magic, version, a Gadget, and the icon's own fields.
const HEADER: usize = 78;
/// OldDrawerData_SIZEOF: a NewWindow (48 bytes) and the drawer's scrolled place (two longs).
const DRAWER_DATA: usize = 56;
/// dd_Flags and dd_ViewModes, which OS 2 and later write after everything else.
const DRAWER_DATA_MORE: usize = 6;
/// struct Image: LeftEdge, TopEdge, Width, Height, Depth, ImageData, PlanePick, PlaneOnOff, NextImage.
const IMAGE: usize = 20;
/// NO_ICON_POSITION: an icon Workbench places itself.
const NO_POSITION: u32 = 0x8000_0000;
/// The line NewIcons puts before its pictures in the tool types.
const NEWICONS_MARK: &str = "*** DON'T EDIT THE FOLLOWING LINES!! ***";
/// The most bytes a string in an icon is believed to have: more means the file isn't what it seems.
const MOST_STRING: usize = 64 * 1024;

// Where the DiskObject's fields are (workbench.i, with the Gadget from intuition.i at 4).
const GADGET_WIDTH: usize = 12;
const GADGET_HEIGHT: usize = 14;
const GADGET_FLAGS: usize = 16;
const GADGET_RENDER: usize = 22;
const SELECT_RENDER: usize = 26;
const USER_DATA: usize = 44;
const TYPE: usize = 48;
const DEFAULT_TOOL: usize = 50;
const TOOL_TYPES: usize = 54;
const CURRENT_X: usize = 58;
const CURRENT_Y: usize = 62;
const DRAWER: usize = 66;
const TOOL_WINDOW: usize = 70;
const STACK: usize = 74;

/// The kinds of icon (do_Type), from WBDISK to WBAPPICON.
pub const TYPES: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
/// A program's icon, WBTOOL.
pub const TOOL: u8 = 3;

/// What a kind of icon is, in words.
pub fn type_name(kind: u8) -> &'static str {
    match kind {
        1 => "Disk",
        2 => "Drawer",
        3 => "Tool (a program)",
        4 => "Project (a file a program opens)",
        5 => "Trashcan",
        6 => "Device",
        7 => "Kickstart disk",
        8 => "AppIcon",
        _ => "Unknown",
    }
}

fn long(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([bytes[at], bytes[at + 1]])
}

fn too_short() -> String {
    crate::i18n::tr("The icon ends before it should: it's damaged or cut short.").to_string()
}

/// Amiga text is ISO 8859-1, a byte to a character.
fn from_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn to_latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::try_from(c as u32).unwrap_or(b'?')).collect()
}

/// A string as the icon keeps it: its bytes as stored, its closing nought among them, so it's written back as it
/// was even if something odd follows the nought.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stored(Vec<u8>);

impl Stored {
    fn text(&self) -> String {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        from_latin1(&self.0[..end])
    }

    fn new(text: &str) -> Self {
        let mut bytes = to_latin1(text);
        bytes.push(0);
        Stored(bytes)
    }

    fn read(bytes: &[u8], at: &mut usize) -> Result<Self, String> {
        let length = bytes.get(*at..*at + 4).map(|b| long(b, 0) as usize).ok_or_else(too_short)?;
        if length > MOST_STRING {
            return Err(crate::i18n::tr("A string in the icon is far too long: it isn't a Workbench icon, or it's damaged.").into());
        }
        let text = bytes.get(*at + 4..*at + 4 + length).ok_or_else(too_short)?.to_vec();
        *at += 4 + length;
        Ok(Stored(text))
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend((self.0.len() as u32).to_be_bytes());
        out.extend(&self.0);
    }
}

/// One of the original icon's pictures: an Image structure and its bitplanes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Planar {
    header: [u8; IMAGE],
    planes: Vec<u8>,
}

impl Planar {
    fn width(&self) -> usize {
        word(&self.header, 4) as usize
    }

    fn height(&self) -> usize {
        word(&self.header, 6) as usize
    }

    fn depth(&self) -> usize {
        word(&self.header, 8) as usize
    }

    fn row_bytes(width: usize) -> usize {
        width.div_ceil(16) * 2
    }

    /// icon.library reads Depth planes, each a whole number of words wide.
    fn read(bytes: &[u8], at: &mut usize) -> Result<Self, String> {
        let header: [u8; IMAGE] = bytes.get(*at..*at + IMAGE).ok_or_else(too_short)?.try_into().expect("twenty bytes");
        let (width, height, depth) = (word(&header, 4) as usize, word(&header, 6) as usize, word(&header, 8) as usize);
        if depth > 8 || width > 1024 || height > 1024 {
            return Err(crate::i18n::tr("The icon's picture has an impossible size: it isn't a Workbench icon, or it's damaged.").into());
        }
        let size = Self::row_bytes(width) * height * depth;
        let planes = bytes.get(*at + IMAGE..*at + IMAGE + size).ok_or_else(too_short)?.to_vec();
        *at += IMAGE + size;
        Ok(Planar { header, planes })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend(&self.header);
        out.extend(&self.planes);
    }

    /// The pen each pixel is drawn in. Planes PlanePick leaves out are filled from PlaneOnOff, as Intuition does.
    fn pens(&self) -> Vec<u8> {
        let (width, height, depth) = (self.width(), self.height(), self.depth());
        let (pick, on_off) = (self.header[14], self.header[15]);
        let row = Self::row_bytes(width);
        let mut pens = vec![0u8; width * height];
        let mut stored = 0;
        for plane in 0..depth.min(8) {
            let bit = 1u8 << plane;
            if pick & bit == 0 {
                if on_off & bit != 0 {
                    pens.iter_mut().for_each(|p| *p |= bit);
                }
                continue;
            }
            let base = stored * row * height;
            stored += 1;
            for y in 0..height {
                for x in 0..width {
                    if self.planes.get(base + y * row + x / 8).is_some_and(|b| b & (0x80 >> (x % 8)) != 0) {
                        pens[y * width + x] |= bit;
                    }
                }
            }
        }
        pens
    }

    /// An image from pens, each plane picked.
    fn from_pens(width: usize, height: usize, depth: usize, pens: &[u8]) -> Self {
        let row = Self::row_bytes(width);
        let mut planes = vec![0u8; row * height * depth];
        for plane in 0..depth {
            for y in 0..height {
                for x in 0..width {
                    if pens[y * width + x] & (1 << plane) != 0 {
                        planes[plane * row * height + y * row + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
        }
        let mut header = [0u8; IMAGE];
        header[4..6].copy_from_slice(&(width as u16).to_be_bytes());
        header[6..8].copy_from_slice(&(height as u16).to_be_bytes());
        header[8..10].copy_from_slice(&(depth as u16).to_be_bytes());
        // ImageData: any address but nought says the planes follow.
        header[10..14].copy_from_slice(&1u32.to_be_bytes());
        header[14] = ((1u16 << depth) - 1) as u8;
        Planar { header, planes }
    }
}

/// Workbench 1.x's colours: blue, white, black and orange.
const WORKBENCH_1: [[u8; 3]; 4] = [[0x00, 0x55, 0xAA], [0xFF, 0xFF, 0xFF], [0x00, 0x00, 0x22], [0xFF, 0x88, 0x00]];
/// Workbench 2 and 3's: grey, black, white and blue, then MagicWB's four more, which eight-colour icons expect.
const WORKBENCH_2: [[u8; 3]; 8] = [[0xAA, 0xAA, 0xAA], [0x00, 0x00, 0x00], [0xFF, 0xFF, 0xFF], [0x66, 0x88, 0xBB], [0x99, 0x99, 0x99], [0xBB, 0xBB, 0xBB], [0xBB, 0xAA, 0x99], [0xFF, 0xAA, 0x99]];

/// The original icon: the DiskObject and what follows it, each part kept as it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Classic {
    header: [u8; HEADER],
    drawer: Option<Vec<u8>>,
    normal: Option<Planar>,
    selected: Option<Planar>,
    default_tool: Option<Stored>,
    /// The table's first long (the number of entries and one more, times four, as written) and the entries.
    tool_types: Option<(u32, Vec<Stored>)>,
    tool_window: Option<Stored>,
    drawer_more: Option<Vec<u8>>,
    /// Whatever follows: OS 3.5's GlowIcons, as an IFF FORM ICON, or nothing.
    rest: Vec<u8>,
}

impl Classic {
    fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < HEADER || bytes[..4] != MAGIC {
            return Err(crate::i18n::tr("This isn't a Workbench icon.").into());
        }
        let header: [u8; HEADER] = bytes[..HEADER].try_into().expect("78 bytes");
        let mut at = HEADER;
        let has = |field: usize| long(&header, field) != 0;
        let drawer = if has(DRAWER) {
            let part = bytes.get(at..at + DRAWER_DATA).ok_or_else(too_short)?.to_vec();
            at += DRAWER_DATA;
            Some(part)
        } else {
            None
        };
        let normal = if has(GADGET_RENDER) { Some(Planar::read(bytes, &mut at)?) } else { None };
        let selected = if has(SELECT_RENDER) { Some(Planar::read(bytes, &mut at)?) } else { None };
        let default_tool = if has(DEFAULT_TOOL) { Some(Stored::read(bytes, &mut at)?) } else { None };
        let tool_types = if has(TOOL_TYPES) {
            let count = bytes.get(at..at + 4).map(|b| long(b, 0)).ok_or_else(too_short)?;
            at += 4;
            let entries = (count / 4).saturating_sub(1) as usize;
            if entries > 4096 {
                return Err(crate::i18n::tr("The icon says it has more tool types than any could: it's damaged.").into());
            }
            let mut list = Vec::with_capacity(entries);
            for _ in 0..entries {
                list.push(Stored::read(bytes, &mut at)?);
            }
            Some((count, list))
        } else {
            None
        };
        let tool_window = if has(TOOL_WINDOW) { Some(Stored::read(bytes, &mut at)?) } else { None };
        // The drawer's flags and view, which OS 2's icon.library writes for a drawer (revision 1 and on).
        let drawer_more = if drawer.is_some() && header[USER_DATA + 3] > 0 && bytes.len() >= at + DRAWER_DATA_MORE && !bytes[at..].starts_with(b"FORM") {
            let part = bytes[at..at + DRAWER_DATA_MORE].to_vec();
            at += DRAWER_DATA_MORE;
            Some(part)
        } else {
            None
        };
        Ok(Classic { header, drawer, normal, selected, default_tool, tool_types, tool_window, drawer_more, rest: bytes[at..].to_vec() })
    }

    fn write(&self) -> Vec<u8> {
        let mut out = self.header.to_vec();
        if let Some(drawer) = &self.drawer {
            out.extend(drawer);
        }
        for image in [&self.normal, &self.selected].into_iter().flatten() {
            image.write(&mut out);
        }
        if let Some(tool) = &self.default_tool {
            tool.write(&mut out);
        }
        if let Some((count, list)) = &self.tool_types {
            out.extend(count.to_be_bytes());
            for entry in list {
                entry.write(&mut out);
            }
        }
        if let Some(window) = &self.tool_window {
            window.write(&mut out);
        }
        if let Some(more) = &self.drawer_more {
            out.extend(more);
        }
        out.extend(&self.rest);
        out
    }

    fn set_long(&mut self, field: usize, value: u32) {
        self.header[field..field + 4].copy_from_slice(&value.to_be_bytes());
    }

    /// Where the NewIcons block starts in the tool types (the mark, and a blank line before it), if there is one.
    fn newicons_start(list: &[Stored]) -> Option<usize> {
        let mark = list.iter().position(|s| s.text() == NEWICONS_MARK).or_else(|| list.iter().position(|s| is_newicons_line(&s.text())))?;
        Some(if mark > 0 && list[mark - 1].text().trim().is_empty() { mark - 1 } else { mark })
    }

    fn visible_tool_types(&self) -> Vec<String> {
        let Some((_, list)) = &self.tool_types else {
            return Vec::new();
        };
        let end = Self::newicons_start(list).unwrap_or(list.len());
        list[..end].iter().map(Stored::text).collect()
    }

    fn newicons_lines(&self) -> Vec<String> {
        self.tool_types.as_ref().map(|(_, list)| list.iter().map(Stored::text).filter(|t| is_newicons_line(t)).collect()).unwrap_or_default()
    }

    fn set_tool_types(&mut self, lines: &[String]) {
        let hidden: Vec<Stored> = match &self.tool_types {
            Some((_, list)) => Self::newicons_start(list).map(|start| list[start..].to_vec()).unwrap_or_default(),
            None => Vec::new(),
        };
        let mut list: Vec<Stored> = lines.iter().map(|l| Stored::new(l)).collect();
        list.extend(hidden);
        if list.is_empty() {
            self.tool_types = None;
            self.set_long(TOOL_TYPES, 0);
        } else {
            if long(&self.header, TOOL_TYPES) == 0 {
                self.set_long(TOOL_TYPES, 1);
            }
            self.tool_types = Some((((list.len() + 1) * 4) as u32, list));
        }
    }

    /// The pictures this icon has: the original planar one, and NewIcons' and GlowIcons' if they're there.
    fn shown(&self) -> Vec<Shown> {
        let mut shown = Vec::new();
        if let Some(glow) = read_glow(&self.rest) {
            shown.push(glow);
        }
        if let Some(newicons) = read_newicons(&self.newicons_lines()) {
            shown.push(newicons);
        }
        if let Some(normal) = &self.normal {
            // Icons saved by Workbench 1.x (revision 0) were drawn for its colours.
            let old = self.header[USER_DATA + 3] == 0;
            let palette: &[[u8; 3]] = if old { &WORKBENCH_1 } else { &WORKBENCH_2 };
            let colour = |pen: u8| -> [u8; 4] {
                let [r, g, b] = palette.get(pen as usize).copied().unwrap_or([pen, pen, pen]);
                [r, g, b, 255]
            };
            let rgba = |pens: &[u8]| pens.iter().flat_map(|&p| colour(p)).collect::<Vec<u8>>();
            let pens = normal.pens();
            let (width, height) = (normal.width(), normal.height());
            let selected = match &self.selected {
                Some(image) if image.width() == width && image.height() == height => rgba(&image.pens()),
                // Without a second picture, Workbench shows a selected icon in the opposite pens.
                _ => {
                    let all = ((1u16 << normal.depth().min(8)) - 1) as u8;
                    rgba(&pens.iter().map(|p| p ^ all).collect::<Vec<u8>>())
                }
            };
            // Four-colour icons were drawn for Workbench's usual hires screen, whose pixels are twice as tall as
            // they're wide; eight-colour ones (MagicWB's) mostly for an interlaced one, with square pixels.
            let aspect = if normal.depth() <= 2 { 0.5 } else { 1.0 };
            let kind = if old { ImageKind::Planar1 } else { ImageKind::Planar };
            shown.push(Shown { kind, width, height, aspect, colours: 1 << normal.depth().min(8), normal: rgba(&pens), selected: Some(selected) });
        }
        shown
    }
}

/// Whether a tool type is one of NewIcons' picture lines.
fn is_newicons_line(text: &str) -> bool {
    text.len() > 4 && (text.starts_with("IM1=") || text.starts_with("IM2="))
}

/// What sort of picture an icon holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    /// The original planar picture, in Workbench 2 and 3's colours.
    Planar,
    /// The original planar picture, saved by Workbench 1.x and shown in its colours.
    Planar1,
    NewIcons,
    GlowIcons,
    Png,
}

impl ImageKind {
    pub fn name(self) -> &'static str {
        match self {
            ImageKind::Planar => "Original (planar)",
            ImageKind::Planar1 => "Original (planar, Workbench 1.x colours)",
            ImageKind::NewIcons => "NewIcons",
            ImageKind::GlowIcons => "GlowIcons (OS 3.5)",
            ImageKind::Png => "PNG (OS 4)",
        }
    }
}

/// One of an icon's pictures, as colours: how it looks, and how it looks selected (double-clicked).
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub kind: ImageKind,
    pub width: usize,
    pub height: usize,
    /// How wide a pixel is for its height.
    pub aspect: f32,
    /// How many colours it was drawn in; 0 for true colour.
    pub colours: usize,
    pub normal: Vec<u8>,
    pub selected: Option<Vec<u8>>,
}

/// Reads bits from the top of each byte down, as GlowIcons' and NewIcons' pictures are packed.
struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn take(&mut self, count: usize) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..count {
            let byte = *self.bytes.get(self.at / 8)?;
            value = value << 1 | u32::from(byte >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Some(value)
    }
}

/// GlowIcons' run-length packing: a byte (from the bit stream) of 0 to 127 says that many and one values follow;
/// 129 to 255 say the next value is repeated 257 less that many times. Values are `bits` wide.
fn unpack_glow(bytes: &[u8], bits: usize, wanted: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(wanted);
    let mut reader = Bits { bytes, at: 0 };
    while out.len() < wanted {
        let Some(code) = reader.take(8) else { break };
        match code {
            0..=127 => {
                for _ in 0..=code {
                    match reader.take(bits) {
                        Some(value) => out.push(value as u8),
                        None => return out,
                    }
                }
            }
            128 => {}
            _ => {
                let Some(value) = reader.take(bits) else { break };
                out.extend(std::iter::repeat_n(value as u8, 257 - code as usize));
            }
        }
    }
    out.truncate(wanted);
    out
}

/// OS 3.5's GlowIcons: a FORM ICON holding a FACE chunk (the size) and an IMAG chunk for each state.
fn read_glow(rest: &[u8]) -> Option<Shown> {
    if rest.len() < 12 || &rest[..4] != b"FORM" || &rest[8..12] != b"ICON" {
        return None;
    }
    let end = (8 + long(rest, 4) as usize).min(rest.len());
    let (mut width, mut height, mut aspect) = (0usize, 0usize, 1.0f32);
    let mut images: Vec<Vec<u8>> = Vec::new();
    let mut first_palette: Vec<[u8; 3]> = Vec::new();
    // The colours the first picture says it has (its palette may be packed short of them).
    let mut declared = 0;
    let mut at = 12;
    while at + 8 <= end {
        let (id, size) = (&rest[at..at + 4], long(rest, at + 4) as usize);
        let data = rest.get(at + 8..(at + 8 + size).min(end))?;
        match id {
            b"FACE" if data.len() >= 4 => {
                width = data[0] as usize + 1;
                height = data[1] as usize + 1;
                // The aspect, as a pixel's width in the high nibble and its height in the low.
                let (across, down) = (data[3] >> 4, data[3] & 15);
                if across > 0 && down > 0 {
                    aspect = f32::from(across) / f32::from(down);
                }
            }
            b"IMAG" if data.len() >= 10 && width > 0 => {
                let (transparent, colours, flags, packed_image, packed_palette, depth) = (data[0], data[1] as usize + 1, data[2], data[3], data[4], data[5] as usize);
                let image_bytes = word(data, 6) as usize + 1;
                let palette_bytes = word(data, 8) as usize + 1;
                let image = data.get(10..10 + image_bytes)?;
                let pens = if packed_image == 1 { unpack_glow(image, depth.clamp(1, 8), width * height) } else { image.to_vec() };
                if images.is_empty() {
                    declared = colours;
                }
                if flags & 2 != 0 {
                    let palette = data.get(10 + image_bytes..10 + image_bytes + palette_bytes)?;
                    let palette = if packed_palette == 1 { unpack_glow(palette, 8, colours * 3) } else { palette.to_vec() };
                    first_palette = palette.as_chunks::<3>().0.to_vec();
                }
                let rgba = (0..width * height)
                    .flat_map(|i| {
                        let pen = pens.get(i).copied().unwrap_or(0);
                        if flags & 1 != 0 && pen == transparent {
                            return [0, 0, 0, 0];
                        }
                        let [r, g, b] = first_palette.get(pen as usize).copied().unwrap_or([0, 0, 0]);
                        [r, g, b, 255]
                    })
                    .collect();
                images.push(rgba);
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let mut images = images.into_iter();
    let normal = images.next()?;
    Some(Shown { kind: ImageKind::GlowIcons, width, height, aspect, colours: declared, normal, selected: images.next() })
}

/// The 7-bit values NewIcons' characters stand for, and its runs of nought bits; each line on its own, any bits
/// left at a line's end thrown away, as NewIcons wrote them.
fn newicons_bits(line: &[u8], out: &mut Vec<bool>) {
    for &c in line {
        match c {
            0x20..=0x6F | 0xA1..=0xD0 => {
                let value = if c <= 0x6F { c - 0x20 } else { 0x50 + (c - 0xA1) };
                out.extend((0..7).rev().map(|bit| value >> bit & 1 != 0));
            }
            0xD1..=0xFF => out.extend(std::iter::repeat_n(false, 7 * (c - 0xD0) as usize)),
            _ => {}
        }
    }
}

/// One NewIcons picture, from its IM1= or IM2= lines: a header (transparency, width, height, colours), the
/// palette, and the pixels.
fn read_newicon(lines: &[&[u8]], palette_from: Option<&[[u8; 3]]>) -> Option<(usize, usize, Vec<u8>, Vec<[u8; 3]>)> {
    let first = lines.first()?;
    if first.len() < 5 {
        return None;
    }
    let transparent = first[0] == b'B';
    let width = first[1].checked_sub(0x21)? as usize;
    let height = first[2].checked_sub(0x21)? as usize;
    let colours = ((first[3].checked_sub(0x21)? as usize) << 6) + first[4].checked_sub(0x21)? as usize;
    if width == 0 || height == 0 || colours == 0 || colours > 256 {
        return None;
    }
    let depth = (usize::BITS - (colours - 1).leading_zeros()).max(1) as usize;
    let mut palette_bytes: Vec<u8> = Vec::new();
    let mut pens: Vec<u8> = Vec::new();
    for line in std::iter::once(&first[5..]).chain(lines[1..].iter().copied()) {
        let mut bits = Vec::new();
        newicons_bits(line, &mut bits);
        let value = |chunk: &[bool]| chunk.iter().fold(0u8, |v, &b| v << 1 | u8::from(b));
        // The palette comes first, a byte to each of red, green and blue; the pixels start on the line after it.
        if palette_bytes.len() < colours * 3 {
            palette_bytes.extend(bits.as_chunks::<8>().0.iter().map(|c| value(c)).take(colours * 3 - palette_bytes.len()));
        } else {
            pens.extend(bits.chunks_exact(depth).map(value));
        }
    }
    let palette: Vec<[u8; 3]> = palette_bytes.as_chunks::<3>().0.to_vec();
    let palette: Vec<[u8; 3]> = if palette.len() == colours { palette } else { palette_from?.to_vec() };
    let rgba = (0..width * height)
        .flat_map(|i| {
            let pen = pens.get(i).copied().unwrap_or(0);
            if transparent && pen == 0 {
                return [0, 0, 0, 0];
            }
            let [r, g, b] = palette.get(pen as usize).copied().unwrap_or([0, 0, 0]);
            [r, g, b, 255]
        })
        .collect();
    Some((width, height, rgba, palette))
}

/// NewIcons' pictures, from the tool types.
fn read_newicons(lines: &[String]) -> Option<Shown> {
    let latin: Vec<Vec<u8>> = lines.iter().map(|l| to_latin1(l)).collect();
    let picture = |number: u8| -> Vec<&[u8]> { latin.iter().filter(|l| l.starts_with(&[b'I', b'M', b'0' + number, b'='])).map(|l| &l[4..]).collect() };
    let (width, height, normal, palette) = read_newicon(&picture(1), None)?;
    let colours = palette.len();
    let selected = read_newicon(&picture(2), Some(&palette)).filter(|(w, h, ..)| *w == width && *h == height).map(|(_, _, rgba, _)| rgba);
    Some(Shown { kind: ImageKind::NewIcons, width, height, aspect: 1.0, colours, normal, selected })
}

/// The icOn chunk's attributes (from AROS's icon.library, which reads OS 4's icons): which are strings.
const ATTR_ICONX: u32 = 0x8000_1001;
const ATTR_ICONY: u32 = 0x8000_1002;
const ATTR_TOOLWINDOW: u32 = 0x8000_1008;
const ATTR_STACKSIZE: u32 = 0x8000_1009;
const ATTR_DEFAULTTOOL: u32 = 0x8000_100A;
const ATTR_TOOLTYPE: u32 = 0x8000_100B;
const ATTR_TYPE: u32 = 0x8000_100F;

fn is_string_attribute(tag: u32) -> bool {
    matches!(tag, ATTR_DEFAULTTOOL | ATTR_TOOLTYPE | ATTR_TOOLWINDOW)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Number(u32),
    Text(Vec<u8>),
}

/// An OS 4 icon: a PNG, perhaps with a second for its selected state, and its details in an icOn chunk.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PngIcon {
    bytes: Vec<u8>,
    /// Where the icOn chunk is (from its length field to the end of its CRC), if it has one.
    chunk: Option<(usize, usize)>,
    attributes: Vec<(u32, Value)>,
    /// Where the first PNG ends: a second, if there is one, follows.
    first_end: usize,
}

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";

/// The chunks of a PNG starting at `start`: each one's place, type and data's place; and where it ends.
fn png_chunks(bytes: &[u8], start: usize) -> Option<(Vec<(usize, [u8; 4], usize, usize)>, usize)> {
    if bytes.get(start..start + 8)? != PNG_SIGNATURE {
        return None;
    }
    let mut chunks = Vec::new();
    let mut at = start + 8;
    while at + 12 <= bytes.len() {
        let size = long(bytes, at) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().ok()?;
        let end = at.checked_add(12)?.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        chunks.push((at, kind, at + 8, size));
        at = end;
        if &kind == b"IEND" {
            return Some((chunks, at));
        }
    }
    None
}

impl PngIcon {
    fn read(bytes: &[u8]) -> Option<Self> {
        let (chunks, first_end) = png_chunks(bytes, 0)?;
        let found = chunks.iter().find(|(_, kind, ..)| kind == b"icOn")?;
        let (at, _, data_at, size) = *found;
        let data = &bytes[data_at..data_at + size];
        let mut attributes = Vec::new();
        let mut i = 0;
        while i + 4 <= data.len() {
            let tag = long(data, i);
            i += 4;
            if is_string_attribute(tag) {
                let end = data[i..].iter().position(|&b| b == 0).map_or(data.len(), |p| i + p);
                attributes.push((tag, Value::Text(data[i..end].to_vec())));
                i = (end + 1).min(data.len());
            } else {
                let value = data.get(i..i + 4).map(|v| long(v, 0))?;
                attributes.push((tag, Value::Number(value)));
                i += 4;
            }
        }
        Some(PngIcon { bytes: bytes.to_vec(), chunk: Some((at, 12 + size)), attributes, first_end })
    }

    fn number(&self, tag: u32) -> Option<u32> {
        self.attributes.iter().find_map(|(t, v)| match v {
            Value::Number(n) if *t == tag => Some(*n),
            _ => None,
        })
    }

    fn texts(&self, tag: u32) -> Vec<String> {
        self.attributes
            .iter()
            .filter_map(|(t, v)| match v {
                Value::Text(text) if *t == tag => Some(from_latin1(text)),
                _ => None,
            })
            .collect()
    }

    /// Changes the attributes with a tag to these values: in the place the first was, or at the end.
    fn set(&mut self, tag: u32, values: Vec<Value>) {
        let place = self.attributes.iter().position(|(t, _)| *t == tag).unwrap_or(self.attributes.len());
        self.attributes.retain(|(t, _)| *t != tag);
        let place = place.min(self.attributes.len());
        self.attributes.splice(place..place, values.into_iter().map(|v| (tag, v)));
    }

    /// The bytes again, with the icOn chunk made from the attributes as they are now.
    fn rebuild(&mut self) {
        let mut data = Vec::new();
        for (tag, value) in &self.attributes {
            data.extend(tag.to_be_bytes());
            match value {
                Value::Number(n) => data.extend(n.to_be_bytes()),
                Value::Text(text) => {
                    data.extend(text);
                    data.push(0);
                }
            }
        }
        let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
        chunk.extend(b"icOn");
        chunk.extend(&data);
        chunk.extend(crate::binary_tools::crc32(&chunk[4..]).to_be_bytes());
        // In place of the old one, or straight after IHDR.
        let (at, length) = self.chunk.unwrap_or((33, 0));
        self.bytes.splice(at..at + length, chunk.iter().copied());
        let grown = chunk.len() as isize - length as isize;
        self.first_end = (self.first_end as isize + grown) as usize;
        self.chunk = Some((at, chunk.len()));
    }

    fn shown(&self) -> Vec<Shown> {
        let Ok((width, height, normal)) = crate::screen_designer::read_png(&self.bytes[..self.first_end]) else {
            return Vec::new();
        };
        let selected = crate::screen_designer::read_png(&self.bytes[self.first_end..]).ok().filter(|(w, h, _)| *w == width && *h == height).map(|(_, _, rgba)| rgba);
        vec![Shown { kind: ImageKind::Png, width, height, aspect: 1.0, colours: 0, normal, selected }]
    }
}

/// What can be changed in an icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    /// do_Type: see `TYPES`.
    pub kind: u8,
    /// The program a project is opened with; empty for none.
    pub default_tool: String,
    /// The tool types, a line each, without NewIcons' picture lines.
    pub tool_types: Vec<String>,
    /// The stack a program is started with, in bytes.
    pub stack: u32,
    /// Where it sits in its window, or None for Workbench to place it.
    pub position: Option<(i32, i32)>,
}

/// A Workbench icon, read from its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon(Kind);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Classic(Box<Classic>),
    Png(PngIcon),
}

/// Whether bytes are a Workbench icon: the DiskObject's magic, or a PNG with an icOn chunk.
pub fn looks_like(bytes: &[u8]) -> bool {
    if bytes.len() >= HEADER && bytes[..4] == MAGIC {
        return true;
    }
    bytes.starts_with(PNG_SIGNATURE) && png_chunks(bytes, 0).is_some_and(|(chunks, _)| chunks.iter().any(|(_, kind, ..)| kind == b"icOn"))
}

impl Icon {
    pub fn read(bytes: &[u8]) -> Result<Icon, String> {
        if bytes.starts_with(PNG_SIGNATURE) {
            return PngIcon::read(bytes).map(|p| Icon(Kind::Png(p))).ok_or_else(|| crate::i18n::tr("This PNG isn't an OS 4 icon: it has no icOn chunk.").to_string());
        }
        Classic::read(bytes).map(|c| Icon(Kind::Classic(Box::new(c))))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.0 {
            Kind::Classic(classic) => classic.write(),
            Kind::Png(png) => png.bytes.clone(),
        }
    }

    pub fn details(&self) -> Details {
        match &self.0 {
            Kind::Classic(c) => {
                let (x, y) = (long(&c.header, CURRENT_X), long(&c.header, CURRENT_Y));
                Details {
                    kind: c.header[TYPE],
                    default_tool: c.default_tool.as_ref().map(Stored::text).unwrap_or_default(),
                    tool_types: c.visible_tool_types(),
                    stack: long(&c.header, STACK),
                    position: (x != NO_POSITION && y != NO_POSITION).then_some((x as i32, y as i32)),
                }
            }
            Kind::Png(p) => Details {
                kind: p.number(ATTR_TYPE).map_or(0, |t| t as u8),
                default_tool: p.texts(ATTR_DEFAULTTOOL).into_iter().next().unwrap_or_default(),
                tool_types: p.texts(ATTR_TOOLTYPE),
                stack: p.number(ATTR_STACKSIZE).unwrap_or(0),
                position: p.number(ATTR_ICONX).zip(p.number(ATTR_ICONY)).filter(|(x, y)| *x != NO_POSITION && *y != NO_POSITION).map(|(x, y)| (x as i32, y as i32)),
            },
        }
    }

    /// Changes what's different, and only that, so an icon given its own details back is unchanged.
    pub fn set_details(&mut self, new: &Details) {
        let old = self.details();
        if *new == old {
            return;
        }
        match &mut self.0 {
            Kind::Classic(c) => {
                if new.kind != old.kind {
                    c.header[TYPE] = new.kind;
                }
                if new.default_tool != old.default_tool {
                    if new.default_tool.is_empty() {
                        c.default_tool = None;
                        c.set_long(DEFAULT_TOOL, 0);
                    } else {
                        c.default_tool = Some(Stored::new(&new.default_tool));
                        if long(&c.header, DEFAULT_TOOL) == 0 {
                            c.set_long(DEFAULT_TOOL, 1);
                        }
                    }
                }
                if new.tool_types != old.tool_types {
                    c.set_tool_types(&new.tool_types);
                }
                if new.stack != old.stack {
                    c.set_long(STACK, new.stack);
                }
                if new.position != old.position {
                    let (x, y) = new.position.map_or((NO_POSITION, NO_POSITION), |(x, y)| (x as u32, y as u32));
                    c.set_long(CURRENT_X, x);
                    c.set_long(CURRENT_Y, y);
                }
            }
            Kind::Png(p) => {
                if new.kind != old.kind {
                    p.set(ATTR_TYPE, vec![Value::Number(u32::from(new.kind))]);
                }
                if new.default_tool != old.default_tool {
                    p.set(ATTR_DEFAULTTOOL, if new.default_tool.is_empty() { Vec::new() } else { vec![Value::Text(to_latin1(&new.default_tool))] });
                }
                if new.tool_types != old.tool_types {
                    p.set(ATTR_TOOLTYPE, new.tool_types.iter().map(|t| Value::Text(to_latin1(t))).collect());
                }
                if new.stack != old.stack {
                    p.set(ATTR_STACKSIZE, vec![Value::Number(new.stack)]);
                }
                if new.position != old.position {
                    let (x, y) = new.position.map_or((Vec::new(), Vec::new()), |(x, y)| (vec![Value::Number(x as u32)], vec![Value::Number(y as u32)]));
                    p.set(ATTR_ICONX, x);
                    p.set(ATTR_ICONY, y);
                }
                p.rebuild();
            }
        }
    }

    /// Its pictures, the best first: OS 4's PNG, GlowIcons, NewIcons, then the original.
    pub fn pictures(&self) -> Vec<Shown> {
        match &self.0 {
            Kind::Classic(c) => c.shown(),
            Kind::Png(p) => p.shown(),
        }
    }

    /// Which kinds of picture it has, in words.
    pub fn formats(&self) -> String {
        self.pictures().iter().map(|s| s.kind.name()).collect::<Vec<_>>().join(", ")
    }
}

/// The icon as a picture for the preview: its best picture, and beside it how it looks selected, with what it
/// says about itself below.
pub fn to_picture(bytes: &[u8]) -> Result<Picture, String> {
    let icon = Icon::read(bytes)?;
    let pictures = icon.pictures();
    let best = pictures.first().ok_or(crate::i18n::tr("This icon has no picture."))?;
    let gap = 4;
    let width = if best.selected.is_some() { best.width * 2 + gap } else { best.width };
    let mut rgba = vec![0u8; width * best.height * 4];
    for (n, image) in std::iter::once(&best.normal).chain(best.selected.iter()).enumerate() {
        let left = n * (best.width + gap);
        for y in 0..best.height {
            let from = y * best.width * 4;
            let to = (y * width + left) * 4;
            if let Some(row) = image.get(from..from + best.width * 4) {
                rgba[to..to + best.width * 4].copy_from_slice(row);
            }
        }
    }
    let mut picture = Picture { width, height: best.height, rgba, aspect: best.aspect, info: Vec::new(), cycling: None };
    let details = icon.details();
    let note = |picture: &mut Picture, label: &str, value: String| picture.info.push((label.to_string(), value));
    note(&mut picture, "Size", format!("{} \u{D7} {} pixels", best.width, best.height));
    if best.colours > 0 {
        note(&mut picture, "Colours", best.colours.to_string());
    }
    note(&mut picture, "Icon", type_name(details.kind).to_string());
    note(&mut picture, "Pictures", icon.formats());
    if !details.default_tool.is_empty() {
        note(&mut picture, "Default tool", details.default_tool.clone());
    }
    if !details.tool_types.is_empty() {
        note(&mut picture, "Tool types", details.tool_types.join("\n"));
    }
    if details.kind == TOOL || details.stack != 0 {
        note(&mut picture, "Stack", format!("{} bytes", details.stack));
    }
    note(&mut picture, "Place", details.position.map_or_else(|| "Placed by Workbench".to_string(), |(x, y)| format!("{x}, {y}")));
    Ok(picture)
}

/// Limoniter's own icon for a program: a window with a Shell prompt, in Workbench 2 and 3's four colours (grey,
/// black, white, blue), drawn for its hires pixels. Each letter is a pen.
const PROGRAM_ICON: [&str; 20] = [
    "0000000000000000000000000000000000000000",
    "0111111111111111111111111111111111111110",
    "0133333333333333333333333333333333232310",
    "0133333333333333333333333333333333232310",
    "0111111111111111111111111111111111111110",
    "0122222222222222222222222222222222222210",
    "0122111222222222222222222222222222222210",
    "0122221112222222222222222222222222222210",
    "0122222221112222222222222222222222222210",
    "0122222221112222222222222222222222222210",
    "0122221112222222222222222222222222222210",
    "0122111222222111111111122222222222222210",
    "0122222222222222222222222222222222222210",
    "0122222222222222222222222222222222222210",
    "0122222222222222222222222222222222222210",
    "0122222222222222222222222222222222222210",
    "0122222222222222222222222222222222222210",
    "0111111111111111111111111111111111111110",
    "0000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000",
];

/// The stack Workbench gives a program whose icon asks for none: AmigaDOS's own 4096 bytes.
pub const STACK_BYTES: u32 = 4096;

/// Limoniter's own program icon: a classic DiskObject every Workbench from 1.x on reads, a Tool, placed by
/// Workbench, with a 4096-byte stack.
pub fn own_program_icon() -> Vec<u8> {
    let (width, height) = (PROGRAM_ICON[0].len(), PROGRAM_ICON.len());
    let pens: Vec<u8> = PROGRAM_ICON.iter().flat_map(|row| row.bytes().map(|b| b - b'0')).collect();
    let normal = Planar::from_pens(width, height, 2, &pens);
    let mut header = [0u8; HEADER];
    header[..4].copy_from_slice(&MAGIC);
    header[GADGET_WIDTH..GADGET_WIDTH + 2].copy_from_slice(&(width as u16).to_be_bytes());
    header[GADGET_HEIGHT..GADGET_HEIGHT + 2].copy_from_slice(&(height as u16).to_be_bytes());
    // GFLG_GADGIMAGE: the render is an Image; highlighted by complementing it (GFLG_GADGHCOMP).
    header[GADGET_FLAGS..GADGET_FLAGS + 2].copy_from_slice(&4u16.to_be_bytes());
    // Activation: GACT_RELVERIFY and GACT_IMMEDIATE, and GadgetType GTYP_BOOLGADGET, as icon.library writes.
    header[18..20].copy_from_slice(&3u16.to_be_bytes());
    header[20..22].copy_from_slice(&1u16.to_be_bytes());
    header[GADGET_RENDER..GADGET_RENDER + 4].copy_from_slice(&1u32.to_be_bytes());
    // The revision in UserData: 1, an icon for Workbench 2 and later's colours.
    header[USER_DATA + 3] = 1;
    header[TYPE] = TOOL;
    header[CURRENT_X..CURRENT_X + 4].copy_from_slice(&NO_POSITION.to_be_bytes());
    header[CURRENT_Y..CURRENT_Y + 4].copy_from_slice(&NO_POSITION.to_be_bytes());
    header[STACK..STACK + 4].copy_from_slice(&STACK_BYTES.to_be_bytes());
    Classic { header, drawer: None, normal: Some(normal), selected: None, default_tool: None, tool_types: None, tool_window: None, drawer_more: None, rest: Vec::new() }.write()
}

/// The icon for a program built: the person's own icon (any kind) made a Tool with no default tool, for Workbench
/// to place, or Limoniter's own without one.
pub fn program_icon(own: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let Some(bytes) = own else {
        return Ok(own_program_icon());
    };
    let mut icon = Icon::read(bytes)?;
    let mut details = icon.details();
    details.kind = TOOL;
    details.default_tool.clear();
    details.position = None;
    if details.stack == 0 {
        details.stack = STACK_BYTES;
    }
    icon.set_details(&details);
    Ok(icon.to_bytes())
}

/// What a program's icon is called: its name with ".info" after it.
pub fn icon_name(program: &str) -> String {
    format!("{program}.info")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A planar icon to test with: a Tool, its pictures from pens, with tool types and a default tool.
    pub fn made(tool_types: &[&str], default_tool: Option<&str>, selected: bool) -> Vec<u8> {
        let mut icon = Icon::read(&own_program_icon()).unwrap();
        if let Kind::Classic(c) = &mut icon.0
            && selected
        {
            let pens: Vec<u8> = (0..40 * 20).map(|i| (i % 4) as u8).collect();
            c.selected = Some(Planar::from_pens(40, 20, 2, &pens));
            c.set_long(SELECT_RENDER, 1);
            // GFLG_GADGHIMAGE: the second picture is shown when it's selected.
            c.header[GADGET_FLAGS + 1] |= 2;
        }
        let mut details = icon.details();
        details.tool_types = tool_types.iter().map(|s| s.to_string()).collect();
        details.default_tool = default_tool.unwrap_or_default().to_string();
        icon.set_details(&details);
        icon.to_bytes()
    }

    #[test]
    fn the_made_icon_is_laid_out_as_workbench_i_says() {
        let bytes = own_program_icon();
        // DiskObject: do_Magic, do_Version, then the Gadget, whose Width and Height are the picture's.
        assert_eq!(&bytes[..4], &[0xE3, 0x10, 0x00, 0x01]);
        assert_eq!(word(&bytes, 4 + 8), 40, "gg_Width");
        assert_eq!(word(&bytes, 4 + 10), 20, "gg_Height");
        assert_eq!(bytes[48], 3, "do_Type is WBTOOL");
        assert_eq!(long(&bytes, 50), 0, "no do_DefaultTool");
        assert_eq!(long(&bytes, 54), 0, "no do_ToolTypes");
        assert_eq!(long(&bytes, 58), 0x8000_0000, "do_CurrentX is NO_ICON_POSITION");
        assert_eq!(long(&bytes, 66), 0, "no do_DrawerData");
        assert_eq!(long(&bytes, 74), 4096, "do_StackSize");
        // The Image follows the 78 bytes straight away: 40 by 20 in 2 planes, each row three words.
        assert_eq!((word(&bytes, 78 + 4), word(&bytes, 78 + 6), word(&bytes, 78 + 8)), (40, 20, 2));
        assert_eq!(bytes[78 + 14], 3, "PlanePick: both planes");
        assert_eq!(bytes.len(), 78 + 20 + 6 * 20 * 2, "nothing more");
        // Its corner's pen 0 (grey) and the title bar's pen 3 (blue: both planes set).
        let first_plane_row2 = 78 + 20 + 2 * 6;
        let second_plane_row2 = 78 + 20 + 6 * 20 + 2 * 6;
        assert_eq!(bytes[first_plane_row2] & 0x20, 0x20, "pen 3 at (2, 2) in plane 0");
        assert_eq!(bytes[second_plane_row2] & 0x20, 0x20, "pen 3 at (2, 2) in plane 1");
        let picture = to_picture(&bytes).unwrap();
        assert_eq!((picture.height, picture.aspect), (20, 0.5));
        assert_eq!(&picture.rgba[..4], &[0xAA, 0xAA, 0xAA, 255], "grey, Workbench 2's pen 0");
        let at = (2 * picture.width + 2) * 4;
        assert_eq!(&picture.rgba[at..at + 4], &[0x66, 0x88, 0xBB, 255], "blue, pen 3");
        // Selected, with no second picture, it's in the opposite pens: blue becomes grey.
        let at = (2 * picture.width + 44 + 2) * 4;
        assert_eq!(&picture.rgba[at..at + 4], &[0xAA, 0xAA, 0xAA, 255]);
    }

    #[test]
    fn details_are_changed_and_an_unchanged_icon_is_byte_for_byte_the_same() {
        let bytes = made(&["DONOTWAIT", "CX_PRIORITY=0"], Some("SYS:Utilities/MultiView"), true);
        let mut icon = Icon::read(&bytes).unwrap();
        assert_eq!(icon.to_bytes(), bytes);
        let details = icon.details();
        assert_eq!(details.tool_types, ["DONOTWAIT", "CX_PRIORITY=0"]);
        assert_eq!(details.default_tool, "SYS:Utilities/MultiView");
        assert_eq!(details.position, None);
        icon.set_details(&details.clone());
        assert_eq!(icon.to_bytes(), bytes, "its own details back change nothing");
        // The tool types and stack changed; the pictures are as they were.
        let mut changed = details.clone();
        changed.tool_types = vec!["PUBSCREEN=Workbench".into()];
        changed.stack = 20000;
        changed.default_tool.clear();
        changed.position = Some((10, 20));
        icon.set_details(&changed);
        let written = icon.to_bytes();
        let again = Icon::read(&written).unwrap();
        assert_eq!(again.details(), changed);
        assert_eq!(long(&written, 50), 0, "no default tool now");
        assert_eq!(again.pictures(), Icon::read(&bytes).unwrap().pictures());
        // Every tool type gone: the table goes too.
        let mut none = changed.clone();
        none.tool_types.clear();
        let mut icon = again.clone();
        icon.set_details(&none);
        assert_eq!(long(&icon.to_bytes(), 54), 0);
        assert_eq!(Icon::read(&icon.to_bytes()).unwrap().details(), none);
    }

    #[test]
    fn a_second_planar_picture_is_shown_selected() {
        let bytes = made(&[], None, true);
        let shown = Icon::read(&bytes).unwrap().pictures();
        let selected = shown[0].selected.as_ref().unwrap();
        // Its pixel 1 is pen 1 (black), pixel 2 pen 2 (white).
        assert_eq!(&selected[4..8], &[0, 0, 0, 255]);
        assert_eq!(&selected[8..12], &[255, 255, 255, 255]);
    }

    /// NewIcons' encoding of bits, as `newicons_bits` reads them: 7 bits to a character.
    fn newicons_chars(bits: &[bool]) -> Vec<u8> {
        bits.chunks(7)
            .map(|chunk| {
                let value = (0..7).fold(0u8, |v, i| v << 1 | u8::from(chunk.get(i).copied().unwrap_or(false)));
                if value < 0x50 { 0x20 + value } else { 0xA1 + value - 0x50 }
            })
            .collect()
    }

    /// The tool types NewIcons would add for a picture: IMn= lines, the palette on the first, pixels after.
    pub fn newicons_lines(number: u8, width: usize, height: usize, palette: &[[u8; 3]], pens: &[u8], transparent: bool) -> Vec<String> {
        let colours = palette.len();
        let depth = (usize::BITS - (colours - 1).leading_zeros()).max(1) as usize;
        let mut first = vec![if transparent { b'B' } else { b'C' }, 0x21 + width as u8, 0x21 + height as u8, 0x21 + (colours >> 6) as u8, 0x21 + (colours & 63) as u8];
        let bits: Vec<bool> = palette.iter().flatten().flat_map(|byte| (0..8).rev().map(move |b| byte >> b & 1 != 0)).collect();
        first.extend(newicons_chars(&bits));
        let mut lines = vec![first];
        // Each line a whole number of pixels, as bits left at a line's end are thrown away.
        let per_line = 60 * 7 / depth;
        for chunk in pens.chunks(per_line) {
            let bits: Vec<bool> = chunk.iter().flat_map(|pen| (0..depth).rev().map(move |b| pen >> b & 1 != 0)).collect();
            lines.push(newicons_chars(&bits));
        }
        lines.into_iter().map(|line| format!("IM{number}={}", from_latin1(&line))).collect()
    }

    #[test]
    fn newicons_pictures_are_read_from_the_tool_types_and_kept_out_of_the_way() {
        let palette = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [10, 20, 30], [200, 100, 50]];
        let (width, height) = (13, 7);
        let pens: Vec<u8> = (0..width * height).map(|i| (i * 7 % 5) as u8).collect();
        let selected: Vec<u8> = pens.iter().map(|p| 4 - p).collect();
        let mut lines = vec!["FILETYPE=text".to_string(), " ".to_string(), NEWICONS_MARK.to_string()];
        lines.extend(newicons_lines(1, width, height, &palette, &pens, true));
        lines.extend(newicons_lines(2, width, height, &palette, &selected, true));
        let all: Vec<&str> = lines.iter().map(String::as_str).collect();
        let bytes = made(&all, None, false);
        let mut icon = Icon::read(&bytes).unwrap();
        // Only the person's own tool types are shown to change.
        assert_eq!(icon.details().tool_types, ["FILETYPE=text"]);
        let shown = icon.pictures();
        assert_eq!(shown[0].kind, ImageKind::NewIcons, "the NewIcons picture before the original");
        assert_eq!(shown[1].kind, ImageKind::Planar);
        let newicons = &shown[0];
        assert_eq!((newicons.width, newicons.height, newicons.colours), (width, height, 5));
        for (i, &pen) in pens.iter().enumerate() {
            let expected = if pen == 0 { [0, 0, 0, 0] } else { let [r, g, b] = palette[pen as usize]; [r, g, b, 255] };
            assert_eq!(&newicons.normal[i * 4..i * 4 + 4], &expected, "pixel {i}");
        }
        let chosen = newicons.selected.as_ref().unwrap();
        assert_eq!(&chosen[..4], &[200, 100, 50, 255], "the selected picture's first pixel is pen 4");
        // Changing the tool types keeps the pictures, after them.
        let mut details = icon.details();
        details.tool_types = vec!["FILETYPE=ilbm".into(), "QUIET".into()];
        icon.set_details(&details);
        let again = Icon::read(&icon.to_bytes()).unwrap();
        assert_eq!(again.details().tool_types, ["FILETYPE=ilbm", "QUIET"]);
        assert_eq!(again.pictures()[0], shown[0]);
        let Kind::Classic(c) = &again.0 else { panic!() };
        let texts: Vec<String> = c.tool_types.as_ref().unwrap().1.iter().map(Stored::text).collect();
        assert_eq!(&texts[..4], ["FILETYPE=ilbm", "QUIET", " ", NEWICONS_MARK]);
    }

    /// GlowIcons' packing, the other way: literal runs of up to 128 values, each `bits` wide.
    fn pack_glow(values: &[u8], bits: usize) -> Vec<u8> {
        let mut stream: Vec<bool> = Vec::new();
        let mut push = |value: u32, count: usize| stream.extend((0..count).rev().map(|b| value >> b & 1 != 0));
        let mut i = 0;
        while i < values.len() {
            let run = values[i..].iter().take_while(|&&v| v == values[i]).count().min(128);
            if run >= 3 {
                push(257 - run as u32, 8);
                push(u32::from(values[i]), bits);
                i += run;
            } else {
                let take = (values.len() - i).min(128).min(2);
                push(take as u32 - 1, 8);
                for v in &values[i..i + take] {
                    push(u32::from(*v), bits);
                }
                i += take;
            }
        }
        stream.chunks(8).map(|c| (0..8).fold(0u8, |v, i| v << 1 | u8::from(c.get(i).copied().unwrap_or(false)))).collect()
    }

    /// A FORM ICON with a FACE and two IMAGs, the second without a palette of its own, both packed.
    pub fn glow_form(width: usize, height: usize, palette: &[[u8; 3]], pens: &[u8], selected: &[u8]) -> Vec<u8> {
        let depth = (usize::BITS - (palette.len() - 1).leading_zeros()).max(1) as usize;
        let mut chunks = Vec::new();
        let mut chunk = |id: &[u8; 4], data: Vec<u8>| {
            chunks.extend(id);
            chunks.extend((data.len() as u32).to_be_bytes());
            chunks.extend(&data);
            if data.len() % 2 == 1 {
                chunks.push(0);
            }
        };
        chunk(b"FACE", vec![width as u8 - 1, height as u8 - 1, 0, 0, 0, (palette.len() * 3 - 1) as u8]);
        for (n, image) in [pens, selected].into_iter().enumerate() {
            let packed = pack_glow(image, depth);
            let flat: Vec<u8> = palette.iter().flatten().copied().collect();
            let packed_palette = pack_glow(&flat, 8);
            let has_palette = n == 0;
            let mut data = vec![0, palette.len() as u8 - 1, if has_palette { 3 } else { 1 }, 1, 1, depth as u8];
            data.extend(((packed.len() - 1) as u16).to_be_bytes());
            data.extend(((packed_palette.len() - 1) as u16).to_be_bytes());
            data.extend(&packed);
            if has_palette {
                data.extend(&packed_palette);
            }
            chunk(b"IMAG", data);
        }
        let mut form = b"FORM".to_vec();
        form.extend(((chunks.len() + 4) as u32).to_be_bytes());
        form.extend(b"ICON");
        form.extend(chunks);
        form
    }

    #[test]
    fn glowicons_are_read_from_the_form_after_the_icon_and_kept_as_they_were() {
        let palette = [[0, 0, 0], [250, 200, 10], [10, 100, 200]];
        let (width, height) = (20, 9);
        let pens: Vec<u8> = (0..width * height).map(|i| if i < 30 { 1 } else { (i % 3) as u8 }).collect();
        let selected: Vec<u8> = pens.iter().map(|p| (p + 1) % 3).collect();
        let mut bytes = made(&["QUIET"], None, false);
        bytes.extend(glow_form(width, height, &palette, &pens, &selected));
        let mut icon = Icon::read(&bytes).unwrap();
        assert_eq!(icon.to_bytes(), bytes);
        let shown = icon.pictures();
        assert_eq!(shown[0].kind, ImageKind::GlowIcons);
        assert_eq!((shown[0].width, shown[0].height, shown[0].colours), (width, height, 3));
        for (i, &pen) in pens.iter().enumerate() {
            let expected = if pen == 0 { [0, 0, 0, 0] } else { let [r, g, b] = palette[pen as usize]; [r, g, b, 255] };
            assert_eq!(&shown[0].normal[i * 4..i * 4 + 4], &expected, "pixel {i}");
        }
        assert_eq!(&shown[0].selected.as_ref().unwrap()[..4], &[10, 100, 200, 255], "the selected picture, in the first's palette");
        // Tool types changed: the FORM is still at the end, as it was.
        let mut details = icon.details();
        details.tool_types.push("DONOTWAIT".into());
        icon.set_details(&details);
        let written = icon.to_bytes();
        assert!(written.ends_with(&glow_form(width, height, &palette, &pens, &selected)));
        assert_eq!(Icon::read(&written).unwrap().pictures()[0], shown[0]);
    }

    /// An OS 4 icon: a PNG with an icOn chunk after its IHDR, and a second PNG for its selected state.
    fn png_icon(attributes: &[(u32, Value)]) -> Vec<u8> {
        let png = |colour: [u8; 3]| {
            let mut picture = Picture { width: 3, height: 2, rgba: Vec::new(), aspect: 1.0, info: Vec::new(), cycling: None };
            picture.rgba = (0..6).flat_map(|_| [colour[0], colour[1], colour[2], 255]).collect();
            crate::pictures::to_png(&picture).unwrap()
        };
        let mut icon = PngIcon { bytes: png([1, 2, 3]), chunk: None, attributes: attributes.to_vec(), first_end: 0 };
        icon.first_end = icon.bytes.len();
        icon.rebuild();
        let mut bytes = icon.bytes;
        bytes.extend(png([9, 8, 7]));
        bytes
    }

    #[test]
    fn os4_png_icons_have_their_details_read_and_changed() {
        let bytes = png_icon(&[(ATTR_TYPE, Value::Number(3)), (ATTR_STACKSIZE, Value::Number(8192)), (ATTR_TOOLTYPE, Value::Text(b"DONOTWAIT".to_vec())), (0x8000_1010, Value::Number(1)), (ATTR_TOOLTYPE, Value::Text(b"PUBSCREEN=Workbench".to_vec()))]);
        assert!(looks_like(&bytes));
        assert_eq!(crate::pictures::detect("Tool.info", &bytes), Some(crate::pictures::Format::AmigaIcon));
        assert!(!looks_like(&crate::pictures::to_png(&Picture { width: 1, height: 1, rgba: vec![0; 4], aspect: 1.0, info: Vec::new(), cycling: None }).unwrap()), "a plain PNG isn't an icon");
        let mut icon = Icon::read(&bytes).unwrap();
        assert_eq!(icon.to_bytes(), bytes);
        let details = icon.details();
        assert_eq!((details.kind, details.stack), (3, 8192));
        assert_eq!(details.tool_types, ["DONOTWAIT", "PUBSCREEN=Workbench"]);
        let shown = icon.pictures();
        assert_eq!(&shown[0].normal[..4], &[1, 2, 3, 255]);
        assert_eq!(&shown[0].selected.as_ref().unwrap()[..4], &[9, 8, 7, 255], "the second PNG is the selected state");
        let mut changed = details.clone();
        changed.tool_types = vec!["QUIET".into()];
        changed.default_tool = "SYS:C/Ed".into();
        changed.stack = 4096;
        icon.set_details(&changed);
        let written = icon.to_bytes();
        // Still a sound PNG (the chunk's CRC right), with the frameless attribute kept and the second PNG after.
        let again = Icon::read(&written).unwrap();
        assert_eq!(again.details(), changed);
        assert!(crate::screen_designer::read_png(&written).is_ok());
        assert_eq!(again.pictures(), shown);
        let Kind::Png(p) = &again.0 else { panic!() };
        assert_eq!(p.number(0x8000_1010), Some(1));
        let (chunks, _) = png_chunks(&written, 0).unwrap();
        let (at, _, data_at, size) = *chunks.iter().find(|c| &c.1 == b"icOn").unwrap();
        assert_eq!(long(&written, data_at + size), crate::binary_tools::crc32(&written[at + 4..data_at + size]));
    }

    #[test]
    fn a_program_icon_is_made_from_the_persons_own() {
        assert_eq!(program_icon(None).unwrap(), own_program_icon());
        let theirs = made(&["TOOLPRI=1"], Some("SYS:Tools/IconEdit"), true);
        let mut placed = Icon::read(&theirs).unwrap();
        let mut details = placed.details();
        details.kind = 4;
        details.position = Some((5, 6));
        details.stack = 0;
        placed.set_details(&details);
        let made = Icon::read(&program_icon(Some(&placed.to_bytes())).unwrap()).unwrap();
        let details = made.details();
        assert_eq!((details.kind, details.default_tool.as_str(), details.position, details.stack), (TOOL, "", None, STACK_BYTES));
        assert_eq!(details.tool_types, ["TOOLPRI=1"]);
        assert_eq!(made.pictures(), placed.pictures());
        assert!(program_icon(Some(b"not an icon")).is_err());
        assert_eq!(icon_name("demo"), "demo.info");
    }

    #[test]
    fn damaged_icons_say_so_rather_than_panic() {
        let bytes = made(&["ONE", "TWO"], Some("C:Ed"), true);
        for length in [0, 4, 77, 78, 90, bytes.len() - 1] {
            let _ = Icon::read(&bytes[..length]);
            let _ = to_picture(&bytes[..length]);
        }
        let mut huge = bytes.clone();
        huge[78 + 8] = 0x7F;
        assert!(Icon::read(&huge).is_err());
        assert!(Icon::read(b"not an icon at all, but long enough to be one if it started right.............").is_err());
    }

    /// Every icon in the disk images and archives under ..\testmedia (or the folders LIMONITER_ICON_SOURCES names,
    /// separated by semicolons) is read, written back byte for byte, and shown. With LIMONITER_ICON_OUT naming a folder, each is saved there as a PNG to look at.
    #[test]
    fn real_icons_read_and_write_back_unchanged() {
        use crate::media;
        use std::sync::Arc;
        fn walk(volume: &dyn media::Volume, dir: Option<&media::Entry>, place: &str, depth: usize, found: &mut Vec<(String, Vec<u8>)>) {
            if depth > 6 {
                return;
            }
            let Ok(entries) = volume.list(dir) else { return };
            for entry in entries {
                if entry.is_dir {
                    walk(volume, Some(&entry), &format!("{place}/{}", entry.name), depth + 1, found);
                } else if entry.size < 1 << 20 && entry.open_name.to_lowercase().ends_with(".info")
                    && let Ok(bytes) = volume.read(&entry)
                {
                    found.push((format!("{place}/{}", entry.name), bytes));
                } else if entry.size < 16 << 20
                    && let Ok(bytes) = volume.read(&entry)
                    && media::is_nested_image(&entry.open_name, &bytes)
                    && let Ok(inner) = media::open(Arc::new(bytes), &entry.open_name)
                {
                    walk(&*inner, None, &format!("{place}/{}!", entry.name), depth + 1, found);
                }
            }
        }
        let mut found = Vec::new();
        let mut folders: Vec<std::path::PathBuf> = match std::env::var_os("LIMONITER_ICON_SOURCES") {
            Some(named) => named.to_string_lossy().split(';').filter(|p| !p.is_empty()).map(std::path::PathBuf::from).collect(),
            None => vec![std::path::PathBuf::from("../testmedia")],
        };
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = item.path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if !name.starts_with("limoniter-") {
                        folders.push(path);
                    }
                    continue;
                }
                let lower = name.to_lowercase();
                if !(lower.ends_with(".adf") || lower.ends_with(".lha") || lower.ends_with(".lzh") || lower.ends_with(".dms") || lower.ends_with(".hdf") || lower.ends_with(".info")) {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else { continue };
                if lower.ends_with(".info") {
                    found.push((path.display().to_string(), bytes));
                } else if let Ok(volume) = media::open(Arc::new(bytes), &name) {
                    walk(&*volume, None, &path.display().to_string(), 0, &mut found);
                }
            }
        }
        let out = std::env::var_os("LIMONITER_ICON_OUT").map(std::path::PathBuf::from);
        let (mut read, mut glow) = (0, 0);
        for (number, (place, bytes)) in found.iter().enumerate() {
            if !bytes.starts_with(&MAGIC) && !bytes.starts_with(PNG_SIGNATURE) {
                continue;
            }
            let icon = Icon::read(bytes).unwrap_or_else(|e| panic!("{place}: {e}"));
            assert_eq!(&icon.to_bytes(), bytes, "{place} is written back as it was");
            let Kind::Classic(c) = &icon.0 else { continue };
            assert!(c.rest.is_empty() || c.rest.starts_with(b"FORM"), "{place}: {} bytes left over", c.rest.len());
            glow += usize::from(icon.pictures().iter().any(|s| s.kind == ImageKind::GlowIcons));
            let mut same = icon.clone();
            same.set_details(&icon.details());
            assert_eq!(&same.to_bytes(), bytes, "{place}: its own details back");
            let picture = to_picture(bytes).unwrap_or_else(|e| panic!("{place}: {e}"));
            if let Some(out) = &out {
                std::fs::create_dir_all(out).unwrap();
                std::fs::write(out.join(format!("{number:04}.png")), crate::pictures::to_png(&picture).unwrap()).unwrap();
                std::fs::write(out.join(format!("{number:04}.txt")), format!("{place}\n{:#?}\n", icon.details())).unwrap();
            }
            read += 1;
        }
        eprintln!("{read} real icons read and written back, {glow} with GlowIcons");
    }
}
