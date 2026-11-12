//! Pictures in the formats of older machines, decoded to show in the preview and to save as PNG.
//!
//! A format joins by adding a `Format`, saying how its files are recognised in `detect`, and decoding them in
//! `decode`. Each decoder hands back plain RGBA pixels, the shape of a pixel on the machine's own screen, and what
//! the file says about itself.

mod apple2;
mod atari_st;
mod c64;
mod gem;
mod ilbm;
pub use ilbm::Cycling;
mod macpaint;
mod pcx;
pub mod photo_cd;
mod riscos;
mod spectrum;

/// The most bytes a picture file may have to be looked at: more than any of these formats would need.
pub const MOST_PICTURE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Amiga IFF ILBM (planar) and PBM (chunky), as Deluxe Paint and nearly everything on the Amiga saved them.
    Ilbm,
    /// ZX Spectrum screen memory: 6144 bytes of bitmap and 768 of colour.
    SpectrumScreen,
    /// A Kodak Photo CD image pack, at the largest size it holds.
    PhotoCd,
    /// A Commodore 64 bitmap: Koala Painter, Art Studio, Advanced Art Studio or Doodle!.
    C64,
    /// An Atari ST screen: Degas, Degas Elite or NEOchrome.
    AtariSt,
    MacPaint,
    /// A RISC OS sprite file, of one sprite or many.
    Sprite,
    /// ZSoft PC Paintbrush.
    Pcx,
    /// A GEM bit image.
    GemImg,
    /// An Apple II hi-res screen.
    AppleHires,
}

impl Format {
    pub const ALL: [Self; 10] = [Self::Ilbm, Self::SpectrumScreen, Self::PhotoCd, Self::C64, Self::AtariSt, Self::MacPaint, Self::Sprite, Self::Pcx, Self::GemImg, Self::AppleHires];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ilbm => "IFF ILBM picture",
            Self::SpectrumScreen => "ZX Spectrum screen",
            Self::PhotoCd => "Kodak Photo CD image",
            Self::C64 => "Commodore 64 picture",
            Self::AtariSt => "Atari ST picture",
            Self::MacPaint => "MacPaint picture",
            Self::Sprite => "RISC OS sprite file",
            Self::Pcx => "PCX picture",
            Self::GemImg => "GEM bit image",
            Self::AppleHires => "Apple II hi-res screen",
        }
    }
}

/// A decoded picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: usize,
    pub height: usize,
    /// Four bytes a pixel, row by row from the top.
    pub rgba: Vec<u8>,
    /// How wide a pixel is for its height on the machine's screen: 0.5 for an Amiga's hires pixels, 1 for square.
    pub aspect: f32,
    /// What the file says about itself and how it was drawn, for showing beside it.
    pub info: Vec<(String, String)>,
    /// A palette that cycles, for pictures that have one switched on.
    pub cycling: Option<Box<Cycling>>,
}

impl Picture {
    fn new(width: usize, height: usize) -> Self {
        Self { width, height, rgba: vec![0; width * height * 4], aspect: 1.0, info: Vec::new(), cycling: None }
    }

    fn set(&mut self, x: usize, y: usize, [r, g, b]: [u8; 3]) {
        let at = (y * self.width + x) * 4;
        self.rgba[at..at + 4].copy_from_slice(&[r, g, b, 255]);
    }

    fn note(&mut self, label: &str, value: impl Into<String>) {
        self.info.push((label.to_string(), value.into()));
    }
}

/// Which picture format a file is, from its first bytes or, for formats with no signature, its name and size.
pub fn detect(name: &str, bytes: &[u8]) -> Option<Format> {
    if ilbm::looks_like(bytes) {
        return Some(Format::Ilbm);
    }
    if photo_cd::looks_like(bytes) {
        return Some(Format::PhotoCd);
    }
    let ending = ending(name);
    // Those with a signature, or a size and load address no other file is likely to share, first.
    if riscos::looks_like(&ending, bytes) {
        return Some(Format::Sprite);
    }
    if c64::looks_like(bytes) {
        return Some(Format::C64);
    }
    if atari_st::looks_like(&ending, bytes) {
        return Some(Format::AtariSt);
    }
    if macpaint::looks_like(&ending, bytes) {
        return Some(Format::MacPaint);
    }
    if pcx::looks_like(&ending, bytes) {
        return Some(Format::Pcx);
    }
    if gem::looks_like(&ending, bytes) {
        return Some(Format::GemImg);
    }
    if spectrum::looks_like(&ending, bytes) {
        return Some(Format::SpectrumScreen);
    }
    if apple2::looks_like(&ending, bytes) {
        return Some(Format::AppleHires);
    }
    None
}

/// A file's ending, in lower case. RISC OS files copied elsewhere often keep their type as ",ff9" instead.
fn ending(name: &str) -> String {
    let lower = name.to_lowercase();
    if let Some((_, kind)) = lower.rsplit_once(',')
        && kind.len() == 3
        && kind.chars().all(|c| c.is_ascii_hexdigit())
    {
        return kind.to_string();
    }
    std::path::Path::new(&lower).extension().and_then(|e| e.to_str()).unwrap_or_default().to_string()
}

/// Decodes a picture. `name` is the file's name, which a few formats need to tell their kinds apart.
pub fn decode_named(format: Format, name: &str, bytes: &[u8]) -> Result<Picture, String> {
    match format {
        Format::AtariSt => {
            if bytes.len() > MOST_PICTURE_BYTES {
                return Err("This file is too large to be a picture of this kind.".into());
            }
            atari_st::decode(&ending(name), bytes)
        }
        _ => decode(format, bytes),
    }
}

pub fn decode(format: Format, bytes: &[u8]) -> Result<Picture, String> {
    if bytes.len() > MOST_PICTURE_BYTES {
        return Err("This file is too large to be a picture of this kind.".into());
    }
    match format {
        Format::Ilbm => ilbm::decode(bytes),
        Format::SpectrumScreen => spectrum::decode(bytes),
        Format::PhotoCd => photo_cd::decode(bytes),
        Format::C64 => c64::decode(bytes),
        // Packed Degas pictures are told by their ending, which this hasn't been given; the others don't need it.
        Format::AtariSt => atari_st::decode("pc1", bytes),
        Format::MacPaint => macpaint::decode(bytes),
        Format::Sprite => riscos::decode(bytes),
        Format::Pcx => pcx::decode(bytes),
        Format::GemImg => gem::decode(bytes),
        Format::AppleHires => apple2::decode(bytes),
    }
}

/// The picture as a PNG, pixel for pixel. The pixel's shape goes in its pHYs chunk, for programs that heed it.
pub fn to_png(picture: &Picture) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, picture.width as u32, picture.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        if (picture.aspect - 1.0).abs() > 0.01 {
            // Pixels per unit along each side: a pixel twice as tall as it is wide has half as many down as across.
            let (across, down) = if picture.aspect < 1.0 { (1000, (1000.0 * picture.aspect).round() as u32) } else { ((1000.0 / picture.aspect).round() as u32, 1000) };
            encoder.set_pixel_dims(Some(png::PixelDimensions { xppu: across, yppu: down, unit: png::Unit::Unspecified }));
        }
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&picture.rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A name to save a picture as PNG under: the file's own, with ".png" in place of its ending.
pub fn png_name(name: &str) -> String {
    let path = std::path::Path::new(name);
    let known = path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_lowercase().as_str(), "iff" | "ilbm" | "lbm" | "bbm" | "pic" | "scr" | "brush" | "pcd" | "koa" | "kla" | "art" | "ocp" | "dd" | "pi1" | "pi2" | "pi3" | "pc1" | "pc2" | "pc3" | "neo" | "pntg" | "pnt" | "mac" | "ff9" | "spr" | "pcx" | "pcc" | "img" | "ximg" | "hgr"));
    let stem = if known { path.file_stem() } else { path.file_name() };
    format!("{}.png", stem.map_or_else(|| "picture".into(), |s| s.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_png_round_trips_and_keeps_the_pixel_shape() {
        let mut picture = Picture::new(2, 1);
        picture.set(0, 0, [255, 0, 0]);
        picture.set(1, 0, [0, 0, 255]);
        picture.aspect = 0.5;
        let png = to_png(&picture).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(&png));
        let mut reader = decoder.read_info().unwrap();
        let dims = reader.info().pixel_dims.unwrap();
        assert_eq!((dims.xppu, dims.yppu), (1000, 500));
        let mut data = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut data).unwrap();
        assert_eq!(data, [255, 0, 0, 255, 0, 0, 255, 255]);
    }

    /// Every picture in the disk images and archives under ..\testmedia, or instead the folders
    /// LIMONITER_PICTURE_SOURCES names (separated by semicolons), is decoded. When LIMONITER_PICTURE_OUT names a folder,
    /// each is written there as it was and as Limoniter's PNG, and a Photo CD's PhotoYCC as a PPM too (Y, C1 and C2
    /// as red, green and blue), to compare with other programs' decoding. Skipped with nothing to read.
    #[test]
    fn real_pictures_decode() {
        use crate::media;
        use std::path::PathBuf;
        use std::sync::Arc;
        fn walk(volume: &dyn media::Volume, dir: Option<&media::Entry>, place: &str, depth: usize, found: &mut Vec<(String, String, Vec<u8>)>) {
            if depth > 6 {
                return;
            }
            let Ok(entries) = volume.list(dir) else {
                return;
            };
            for entry in entries {
                if entry.is_dir {
                    walk(volume, Some(&entry), &format!("{place}/{}", entry.name), depth + 1, found);
                    continue;
                }
                if entry.size > 16 * 1024 * 1024 {
                    continue;
                }
                let Ok(bytes) = volume.read(&entry) else {
                    continue;
                };
                if detect(&entry.open_name, &bytes).is_some() {
                    found.push((format!("{place}/{}", entry.name), entry.open_name.clone(), bytes));
                } else if media::is_nested_image(&entry.open_name, &bytes)
                    && let Ok(inner) = media::open(Arc::new(bytes), &entry.open_name)
                {
                    walk(&*inner, None, &format!("{place}/{}!", entry.name), depth + 1, found);
                }
            }
        }
        let roots: Vec<PathBuf> = match std::env::var_os("LIMONITER_PICTURE_SOURCES") {
            Some(named) => named.to_string_lossy().split(';').filter(|p| !p.is_empty()).map(PathBuf::from).collect(),
            None => vec![PathBuf::from("../testmedia")],
        };
        let mut files = Vec::new();
        let mut folders: Vec<PathBuf> = roots.into_iter().filter(|r| r.is_dir()).collect();
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = item.path();
                if path.is_dir() {
                    if !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("limoniter-")) {
                        folders.push(path);
                    }
                } else {
                    files.push(path);
                }
            }
        }
        files.sort();
        let mut found = Vec::new();
        for path in &files {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(meta) = std::fs::metadata(path) else { continue };
            if meta.len() > 256 * 1024 * 1024 {
                continue;
            }
            let bytes = std::fs::read(path).unwrap();
            if detect(&name, &bytes).is_some() {
                found.push((path.display().to_string(), name, bytes));
            } else if media::is_image(&name, &bytes[..bytes.len().min(0x8800)], bytes.len() as u64)
                && let Ok(volume) = media::open(Arc::new(bytes), &name)
            {
                walk(&*volume, None, &path.display().to_string(), 0, &mut found);
            }
        }
        let out = std::env::var_os("LIMONITER_PICTURE_OUT").map(PathBuf::from);
        let mut report = String::new();
        let mut problems = Vec::new();
        for (number, (place, name, bytes)) in found.iter().enumerate() {
            let format = detect(name, bytes).unwrap();
            match decode_named(format, name, bytes) {
                Ok(picture) => {
                    let summary: Vec<String> = picture.info.iter().map(|(l, v)| format!("{l}={v}")).collect();
                    report.push_str(&format!("{number:04}\t{format:?}\t{place}\t{}\n", summary.join("; ")));
                    if let Some(out) = &out {
                        std::fs::create_dir_all(out).unwrap();
                        let ending = match format {
                            Format::Ilbm => "iff",
                            Format::SpectrumScreen => "scr",
                            Format::PhotoCd => "pcd",
                            Format::C64 => "c64",
                            Format::AtariSt => name.rsplit('.').next().unwrap_or("st"),
                            Format::MacPaint => "pntg",
                            Format::Sprite => "ff9",
                            Format::Pcx => "pcx",
                            Format::GemImg => "img",
                            Format::AppleHires => "hgr",
                        };
                        std::fs::write(out.join(format!("{number:04}.{ending}")), bytes).unwrap();
                        std::fs::write(out.join(format!("{number:04}.png")), to_png(&picture).unwrap()).unwrap();
                        if format == Format::PhotoCd {
                            let ycc = photo_cd::decode_ycc(bytes).unwrap();
                            let mut ppm = format!("P6\n{} {}\n255\n", ycc.width, ycc.height).into_bytes();
                            for i in 0..ycc.width * ycc.height {
                                ppm.extend([ycc.luma[i], ycc.c1[i], ycc.c2[i]]);
                            }
                            std::fs::write(out.join(format!("{number:04}-ycc.ppm")), ppm).unwrap();
                        }
                    }
                }
                Err(problem) => {
                    report.push_str(&format!("{number:04}\t{format:?}\t{place}\tREFUSED: {problem}\n"));
                    problems.push(format!("{place}: {problem}"));
                }
            }
        }
        if let Some(out) = &out {
            std::fs::write(out.join("report.txt"), &report).unwrap();
        }
        eprintln!("{} pictures, {} refused", found.len(), problems.len());
    }

    #[test]
    fn names_for_the_png() {
        assert_eq!(png_name("Fish.iff"), "Fish.png");
        assert_eq!(png_name("Title.scr"), "Title.png");
        assert_eq!(png_name("Picture.1"), "Picture.1.png");
        assert_eq!(png_name("Logo"), "Logo.png");
    }
}
