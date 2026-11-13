//! The window's side of Spectrum BASIC's graphics: GRAPH mode, which types block graphics and UDGs as the
//! Spectrum's GRAPHICS key does, and the pictures UDGs are drawn with, from the tape a listing came from, its
//! DATA lines, or a file chosen for them.

use super::*;
use crate::zx_graphics::{self, Notation, ZxText};

impl Limoniter {
    /// Whether a document is Spectrum or Next BASIC: listed from a program, or coloured as that BASIC.
    pub(super) fn is_spectrum_basic(&self, index: usize) -> bool {
        let Some(doc) = self.docs.get(index) else {
            return false;
        };
        if doc.hex_mode || doc.browser.is_some() || doc.picture.is_some() {
            return false;
        }
        doc.text.zx.is_some() || matches!(doc.listing_of, Some((basic::Machine::Spectrum | basic::Machine::Next, _))) || doc.language_name() == highlight::BASIC_SINCLAIR
    }

    /// The notation a document's graphics are typed in: zmakebas's if it's already written that way, otherwise
    /// the one chosen in Settings.
    fn notation_for(&self, index: usize) -> Notation {
        let lines = self.docs[index].text.all_lines();
        if zx_graphics::uses_zmakebas(&lines) { Notation::Zmakebas } else { self.prefs.zx_notation }
    }

    /// Switches GRAPH mode on or off, as Caps Shift and 9 do on the Spectrum.
    pub(super) fn toggle_graph_mode(&mut self) {
        if !self.is_spectrum_basic(self.active) {
            return self.message(
                "GRAPH mode",
                "GRAPH mode types the Spectrum's block graphics and UDGs, so it's for Spectrum BASIC. Open a program's listing as text, or choose BASIC (Sinclair and Next) as this file's language.".into(),
            );
        }
        let notation = self.notation_for(self.active);
        let zx = self.docs[self.active].text.zx.get_or_insert_with(Box::default);
        zx.graph = !zx.graph;
        zx.notation = notation;
    }

    /// Asks for a file to take the UDGs' pictures from, and draws the document's UDGs with them.
    pub(super) fn choose_udg_pictures(&mut self) {
        if !self.is_spectrum_basic(self.active) {
            return self.message("UDG pictures", "UDG pictures are for Spectrum BASIC listings.".into());
        }
        let mut picker = rfd::FileDialog::new()
            .set_title("Take the UDGs' pictures from")
            .add_filter("Tapes, snapshots and UDG data", &["tap", "tzx", "sna", "z80", "udg", "bin", "chr"])
            .add_filter("Any file", &["*"]);
        if let Some(folder) = self.docs[self.active].path.as_deref().and_then(Path::parent) {
            picker = picker.set_directory(folder);
        }
        let Some(path) = picker.pick_file() else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => return self.message("UDG pictures", format!("Couldn't read {}.\n\n{e}", path.display())),
        };
        let ending = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match udgs_in_file(&bytes, &ending) {
            Some(udgs) => self.give_udg_pictures(self.active, udgs, name),
            None => self.message(
                "UDG pictures",
                format!("{name} has no UDGs this can find. A tape needs a CODE block loaded where the UDGs go (LOAD \"\" CODE USR \"a\"), a snapshot has them where its UDG system variable says, and any other file is taken as up to 168 bytes of pictures, eight to a UDG, from A."),
            ),
        }
    }

    pub(super) fn give_udg_pictures(&mut self, index: usize, udgs: zx_graphics::Udgs, from: String) {
        let notation = self.notation_for(index);
        let zx = self.docs[index].text.zx.get_or_insert_with(Box::default);
        zx.udgs = udgs;
        zx.udgs_from = Some(from);
        zx.notation = notation;
    }

    /// A Spectrum listing just opened: its graphics in the notation chosen, and its UDGs' pictures if the file
    /// it came from has them or its DATA lines set them up. What it gives back marks the document as Spectrum BASIC.
    pub(super) fn prepare_spectrum_listing(&self, lines: &mut [String], file: Option<(&[u8], &str)>) -> Option<Box<ZxText>> {
        for line in lines.iter_mut() {
            *line = zx_graphics::rewrite(line, self.prefs.zx_notation);
        }
        let on_tape = file.filter(|(_, ending)| matches!(*ending, "tap" | "tzx")).and_then(|(bytes, _)| zx_graphics::udgs_on_tape(bytes)).map(|udgs| (udgs, "the tape"));
        let found = on_tape.or_else(|| zx_graphics::udgs_from_data(lines).map(|udgs| (udgs, "its DATA lines")));
        let (udgs, udgs_from) = found.map_or_else(Default::default, |(udgs, from)| (udgs, Some(from.to_string())));
        Some(Box::new(ZxText { graph: false, udgs, udgs_from, notation: self.prefs.zx_notation }))
    }
}

/// The UDGs in a file chosen for them.
pub(super) fn udgs_in_file(bytes: &[u8], ending: &str) -> Option<zx_graphics::Udgs> {
    match ending {
        "tap" | "tzx" => zx_graphics::udgs_on_tape(bytes),
        "sna" | "z80" => {
            let snapshot = crate::media::spectrum::Snapshot::open(std::sync::Arc::new(bytes.to_vec()), ending == "sna").ok()?;
            Some(zx_graphics::udgs_from_bytes(&snapshot.udgs()?, 0))
        }
        _ if bytes.starts_with(b"ZXTape!\x1A") => zx_graphics::udgs_on_tape(bytes),
        _ if !bytes.is_empty() && bytes.len() <= 168 && bytes.len().is_multiple_of(8) => Some(zx_graphics::udgs_from_bytes(bytes, 0)),
        _ => None,
    }
}
