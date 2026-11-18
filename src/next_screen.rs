//! A ZX Spectrum Next screen in any of its bitmap modes, as the picture converter makes it and the Layer 2 painter
//! draws it: Layer 2 at 256×192 or 320×256 in 256 colours, or 640×256 in 16, and LoRes at 128×96 in 256; each pixel
//! an index into a palette of the Next's 9-bit colours (RRRGGGBB, then the lowest blue bit).
//!
//! The pixels are kept row by row; the Next keeps 256×192 the same, but 320×256 and 640×256 down each column (the
//! second byte the pixel under the first), 640×256 two pixels a byte with the left in the high half. Saved as an
//! `.nxi` (the palette's 512 bytes, then the pixels), the pixels alone (`.sl2`, `.slr`) with the palette (`.nxp`),
//! a BMP (as `.bmpload` takes, at 256×192), and sjasmplus source that shows it, assembled into a `.nex`.

use crate::pictures::next::colour;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NextMode {
    /// Layer 2, 256×192, a byte a pixel.
    Layer2,
    /// Layer 2, 320×256 (over the border), a byte a pixel.
    Layer2Wide,
    /// Layer 2, 640×256, 16 colours.
    Layer2Hi,
    /// LoRes, 128×96 in 256 colours, each pixel 2×2.
    LoRes,
}

impl NextMode {
    pub const ALL: [NextMode; 4] = [NextMode::Layer2, NextMode::Layer2Wide, NextMode::Layer2Hi, NextMode::LoRes];

    pub fn name(self) -> &'static str {
        match self {
            NextMode::Layer2 => "Layer 2: 256\u{D7}192, 256 colours",
            NextMode::Layer2Wide => "Layer 2: 320\u{D7}256, 256 colours",
            NextMode::Layer2Hi => "Layer 2: 640\u{D7}256, 16 colours",
            NextMode::LoRes => "LoRes: 128\u{D7}96, 256 colours",
        }
    }

    pub fn size(self) -> (usize, usize) {
        match self {
            NextMode::Layer2 => (256, 192),
            NextMode::Layer2Wide => (320, 256),
            NextMode::Layer2Hi => (640, 256),
            NextMode::LoRes => (128, 96),
        }
    }

    /// How many colours a pixel can be.
    pub fn colours(self) -> usize {
        if self == NextMode::Layer2Hi { 16 } else { 256 }
    }

    /// How wide a pixel is for its height.
    pub fn aspect(self) -> f32 {
        if self == NextMode::Layer2Hi { 0.5 } else { 1.0 }
    }

    /// The bytes its pixels take.
    pub fn memory(self) -> usize {
        match self {
            NextMode::Layer2 => 49152,
            NextMode::Layer2Wide | NextMode::Layer2Hi => 81920,
            NextMode::LoRes => 12288,
        }
    }

    /// The ending of a file of its pixels alone.
    pub fn ending(self) -> &'static str {
        if self == NextMode::LoRes { "slr" } else { "sl2" }
    }
}

/// A 9-bit colour's two bytes, as the Next's palette registers and its files take them.
pub fn split(value: u16) -> [u8; 2] {
    [(value >> 1) as u8, (value & 1) as u8]
}

pub fn rgb(value: u16) -> [u8; 3] {
    let [first, second] = split(value);
    colour(first, second)
}

/// The nearest 9-bit colour to an 8-bit one: each of red, green and blue to 3 bits.
pub fn nearest_value([r, g, b]: [u8; 3]) -> u16 {
    let three = |v: u8| (u16::from(v) * 7 + 127) / 255;
    three(r) << 6 | three(g) << 3 | three(b)
}

/// The Next's palette at reset: each index's own RRRGGGBB, the lowest blue bit set when either other is.
pub fn default_palette() -> Vec<u16> {
    (0..256u16).map(|i| i << 1 | u16::from(i & 3 != 0)).collect()
}

/// A palette from a file: 512 bytes of two each, or 256 of RRRGGGBB alone.
pub fn palette_from_bytes(bytes: &[u8]) -> Vec<u16> {
    if bytes.len() >= 512 {
        (0..256).map(|i| u16::from(bytes[i * 2]) << 1 | u16::from(bytes[i * 2 + 1] & 1)).collect()
    } else {
        (0..256).map(|i| bytes.get(i).map_or(0, |&b| u16::from(b) << 1 | u16::from(b & 3 != 0))).collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextScreen {
    pub mode: NextMode,
    /// Each pixel's colour index, row by row.
    pub pixels: Vec<u8>,
    /// 256 9-bit colours (640×256 uses the first 16).
    pub palette: Vec<u16>,
}

impl NextScreen {
    /// A screen of colour 0, in the Next's palette at reset (16 greys of it, for 640×256).
    pub fn new(mode: NextMode) -> NextScreen {
        let (w, h) = mode.size();
        let palette = if mode == NextMode::Layer2Hi { (0..256u16).map(|i| if i < 16 { (i * 7 / 15) * 0b1001001 } else { 0 }).collect() } else { default_palette() };
        NextScreen { mode, pixels: vec![0; w * h], palette }
    }

    pub fn width(&self) -> usize {
        self.mode.size().0
    }

    pub fn colour_of(&self, index: u8) -> [u8; 3] {
        rgb(self.palette[usize::from(index)])
    }

    pub fn rgba(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|&p| {
            let [r, g, b] = self.colour_of(p);
            [r, g, b, 255]
        }).collect()
    }

    /// The pixels as the Next keeps them.
    pub fn memory(&self) -> Vec<u8> {
        let (w, h) = self.mode.size();
        match self.mode {
            NextMode::Layer2 | NextMode::LoRes => self.pixels.clone(),
            NextMode::Layer2Wide => (0..w * h).map(|i| self.pixels[(i % 256) * w + i / 256]).collect(),
            NextMode::Layer2Hi => (0..81920).map(|i| {
                let (x, y) = ((i / 256) * 2, i % 256);
                (self.pixels[y * w + x] & 15) << 4 | (self.pixels[y * w + x + 1] & 15)
            }).collect(),
        }
    }

    pub fn from_memory(mode: NextMode, bytes: &[u8], palette: Vec<u16>) -> Option<NextScreen> {
        if bytes.len() != mode.memory() {
            return None;
        }
        let (w, h) = mode.size();
        let pixels = match mode {
            NextMode::Layer2 | NextMode::LoRes => bytes.to_vec(),
            NextMode::Layer2Wide => (0..w * h).map(|i| bytes[(i % w) * 256 + i / w]).collect(),
            NextMode::Layer2Hi => (0..w * h).map(|i| {
                let (x, y) = (i % w, i / w);
                let byte = bytes[(x / 2) * 256 + y];
                if x % 2 == 0 { byte >> 4 } else { byte & 15 }
            }).collect(),
        };
        Some(NextScreen { mode, pixels, palette })
    }

    /// The palette's 512 bytes, as an `.nxp` has it; 640×256's 16 colours, the rest 0.
    pub fn palette_bytes(&self) -> Vec<u8> {
        let count = self.mode.colours();
        (0..256).flat_map(|i| if i < count { split(self.palette[i]) } else { [0, 0] }).collect()
    }

    pub fn nxi(&self) -> Vec<u8> {
        let mut out = self.palette_bytes();
        out.extend(self.memory());
        out
    }

    /// A BMP of it: 8 bits a pixel, or 4 for 640×256.
    pub fn bmp(&self) -> Vec<u8> {
        let (w, h) = self.mode.size();
        let palette: Vec<[u8; 3]> = self.palette.iter().map(|&v| rgb(v)).collect();
        crate::pictures::bmp::encode(w, h, &self.pixels, &palette, if self.mode == NextMode::Layer2Hi { 4 } else { 8 })
    }

    /// A screen from a file: an `.nxi` with its palette, the pixels alone (`.sl2`, `.slr`) in the Next's palette at
    /// reset, or a BMP of one of its sizes. 81920 bytes with no palette are taken as 320×256 unless `wide` says
    /// otherwise.
    pub fn read(name: &str, bytes: &[u8]) -> Result<NextScreen, String> {
        let lower = name.to_lowercase();
        if crate::pictures::bmp::looks_like(bytes) {
            let image = crate::pictures::bmp::indexed(bytes).map_err(|e| crate::i18n::tf!("{e} A truecolour picture can be brought in through the picture converter."))?;
            let mode = match (image.width, image.height) {
                (256, 192) => NextMode::Layer2,
                (320, 256) => NextMode::Layer2Wide,
                (640, 256) if image.pixels.iter().all(|&p| p < 16) => NextMode::Layer2Hi,
                (128, 96) => NextMode::LoRes,
                (w, h) => return Err(crate::i18n::tf!("The BMP is {w}\u{D7}{h}: a Next screen is 256\u{D7}192, 320\u{D7}256, 640\u{D7}256 (16 colours) or 128\u{D7}96. Other pictures can be brought in through the picture converter.")),
            };
            let mut palette: Vec<u16> = image.palette.iter().map(|&c| nearest_value(c)).collect();
            palette.resize(256, 0);
            return Ok(NextScreen { mode, pixels: image.pixels, palette });
        }
        let (palette, memory) = match bytes.len() {
            n if [49152, 81920, 12288].contains(&(n.wrapping_sub(512))) => (palette_from_bytes(&bytes[..512]), &bytes[512..]),
            _ => (default_palette(), bytes),
        };
        let mode = match memory.len() {
            49152 => NextMode::Layer2,
            12288 => NextMode::LoRes,
            81920 => {
                // As Limoniter's reader tells them: a 640 file's palette has 16 colours and its bytes go above 15.
                let sixteen = bytes.len() > memory.len() && bytes[32..512].iter().all(|&b| b == 0) && memory.iter().any(|&b| b > 15);
                if sixteen || lower.ends_with(".nxi640") { NextMode::Layer2Hi } else { NextMode::Layer2Wide }
            }
            n => return Err(crate::i18n::tf!("A Next screen is 49152, 81920 or 12288 bytes of pixels (with 512 of palette in front, for an .nxi), and this is {n}.")),
        };
        NextScreen::from_memory(mode, memory, palette).ok_or_else(|| crate::i18n::tr("The screen couldn't be read.").into())
    }

    /// The same pixels in another mode: cut or filled out with colour 0, and to 16 colours for 640×256.
    pub fn change_mode(&mut self, mode: NextMode) {
        if mode == self.mode {
            return;
        }
        let (ow, oh) = self.mode.size();
        let (w, h) = mode.size();
        let mut pixels = vec![0u8; w * h];
        for y in 0..h.min(oh) {
            for x in 0..w.min(ow) {
                pixels[y * w + x] = self.pixels[y * ow + x];
            }
        }
        if mode == NextMode::Layer2Hi {
            pixels.iter_mut().for_each(|p| *p &= 15);
        }
        self.pixels = pixels;
        self.mode = mode;
    }

    /// sjasmplus source that shows the screen on the Next, with `name.nxp` and the pixels' file beside it, and
    /// writes `name.nex`.
    pub fn source(&self, name: &str) -> String {
        let mode = self.mode;
        let count = mode.colours();
        let pixels = format!("{name}.{}", mode.ending());
        let mut s = crate::i18n::tf!("; {name}: a picture for the ZX Spectrum Next, {}, made by Limoniter.\n; Assemble with sjasmplus (or F9 in Limoniter), with {name}.nxp and {pixels} beside it:\n; it writes {name}.nex, which shows the picture until the Next is reset.\n\n",
            mode.name()
        );
        s += "        DEVICE ZXSPECTRUMNEXT\n        org $8000\nstart:\n        di\n        nextreg $07,3                   ; 28 MHz\n";
        if mode == NextMode::LoRes {
            s += "        nextreg $15,%10000000           ; LoRes on, the layers in the usual order\n";
            s += "        nextreg $43,%00000000           ; the ULA's first palette (LoRes's) to be written\n";
        } else {
            let (resolution, bottom) = match mode {
                NextMode::Layer2 => ("%00000000          ; 256\u{D7}192", 191),
                NextMode::Layer2Wide => ("%00010000          ; 320\u{D7}256", 255),
                _ => ("%00100000          ; 640\u{D7}256", 255),
            };
            let right = if mode == NextMode::Layer2 { 255 } else { 159 };
            s += "        nextreg $12,9                   ; Layer 2's pixels in 16K banks 9 on\n";
            s += &format!("        nextreg $70,{resolution}, palette offset 0\n");
            s += "        nextreg $1c,1                   ; Layer 2's clip window, from its first value:\n";
            s += &format!("        nextreg $18,0                   ; left, right (in 320 and 640 modes, in 2s and 4s),\n        nextreg $18,{right}\n        nextreg $18,0                   ; top and bottom\n        nextreg $18,{bottom}\n");
            s += "        nextreg $43,%00010000           ; Layer 2's first palette to be written\n";
        }
        s += &format!(
            "        nextreg $40,0                   ; from colour 0, on by itself after each\n        ld hl,palette\n        ld b,{}\n.colour:\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; RRRGGGBB\n        ld a,(hl)\n        inc hl\n        nextreg $44,a                   ; then the lowest blue bit\n        djnz .colour\n",
            count & 255
        );
        if mode != NextMode::LoRes {
            s += "        nextreg $69,%10000000           ; Layer 2 shown\n";
        }
        s += "wait:   jr wait\n\n";
        s += &format!("palette:\n        incbin \"{name}.nxp\",0,{}\n\n", count * 2);
        let banks = if mode == NextMode::LoRes {
            s += &format!("        ; LoRes's pixels: the top half at $4000, the bottom at $6000.\n        org $4000\n        incbin \"{pixels}\",0,6144\n        org $6000\n        incbin \"{pixels}\",6144,6144\n\n");
            "5,2".to_string()
        } else {
            let last = 9 + mode.memory() / 16384 - 1;
            s += &format!("        ; The pixels, in 16K banks 9 to {last}: 8K pages 18 on, one after another in slot 0.\n        MMU 0 n, 18, $0000\n        incbin \"{pixels}\"\n\n");
            std::iter::once(2).chain(9..=last).map(|b| b.to_string()).collect::<Vec<_>>().join(",")
        };
        s += &format!("        SAVENEX OPEN \"{name}.nex\",start,$bffe\n        SAVENEX CORE 3,0,0\n        SAVENEX CFG 0\n        SAVENEX BANK {banks}\n        SAVENEX CLOSE\n");
        s
    }

    /// The source assembled by Limoniter, its files given from memory: the `.nex` it writes.
    pub fn nex(&self, name: &str) -> Result<Vec<u8>, String> {
        let files = [(format!("{name}.nxp"), self.palette_bytes()), (format!("{name}.{}", self.mode.ending()), self.memory())];
        assemble_nex(name, &self.source(name), &files)
    }

    /// Everything that can be saved, by file name.
    pub fn everything(&self, name: &str) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![
            (format!("{name}.nxi"), self.nxi()),
            (format!("{name}.{}", self.mode.ending()), self.memory()),
            (format!("{name}.nxp"), self.palette_bytes()),
            (format!("{name}.bmp"), self.bmp()),
            (format!("{name}.asm"), self.source(name).into_bytes()),
        ];
        if let Ok(nex) = self.nex(name) {
            files.push((format!("{name}.nex"), nex));
        }
        let (w, h) = self.mode.size();
        let picture = crate::pictures::Picture { width: w, height: h, rgba: self.rgba(), aspect: self.mode.aspect(), info: Vec::new(), cycling: None };
        if let Ok(png) = crate::pictures::to_png(&picture) {
            files.push((format!("{name}.png"), png));
        }
        files
    }
}

/// Next source assembled by Limoniter, the files it includes given from memory by name: the `.nex` it writes.
pub fn assemble_nex(name: &str, source: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let read = |path: &std::path::Path| -> Result<Vec<u8>, String> {
        let wanted = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        files.iter().find(|(n, _)| *n == wanted).map(|(_, b)| b.clone()).ok_or_else(|| crate::i18n::tf!("{wanted} isn't one of the picture's files"))
    };
    let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new(&format!("{name}.asm")), source, &read);
    if let Some(problem) = assembly.problems.first() {
        return Err(crate::i18n::tf!("The viewer didn't assemble: {}", problem.message));
    }
    assembly.outputs.into_iter().find(|o| o.name.ends_with(".nex")).map(|o| o.bytes).ok_or_else(|| crate::i18n::tr("The viewer made no .nex.").into())
}

#[cfg(test)]
mod tests;
