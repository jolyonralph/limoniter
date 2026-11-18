//! Where a program sits in its machine's memory, after it's built: the machine's 64K laid out (ROM, the screen,
//! the system's own areas, I/O, memory that's slower or paged) with the program's bytes over it, what's still
//! free, and a word wherever the program lands somewhere it can't go or shouldn't.

use crate::running::Machine;

/// What a part of memory is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Free for programs.
    Free,
    /// Free, but slower: the Spectrum's contended memory, shared with the screen.
    Slow,
    /// Free, but paged: on the 128, banks switch in here.
    Paged,
    Rom,
    Screen,
    /// The system's own: zero page, the stack, system variables, workspace.
    System,
    Io,
    /// A cartridge's own space, as the 2600's.
    Cartridge,
}

impl Kind {
    pub fn is_free(self) -> bool {
        matches!(self, Kind::Free | Kind::Slow | Kind::Paged | Kind::Cartridge)
    }
}

/// A part of memory: from `start` up to (not including) `end`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Area {
    pub start: u32,
    pub end: u32,
    pub kind: Kind,
    pub name: &'static str,
}

/// A program's place in its machine's memory.
#[derive(Clone, Debug, PartialEq)]
pub struct Map {
    pub machine: Machine,
    pub areas: Vec<Area>,
    /// The program's bytes, as ranges, joined where they touch.
    pub used: Vec<(u32, u32)>,
    /// Bytes still free, in the areas programs can use.
    pub free: u32,
    /// The biggest free stretch, from where to where.
    pub biggest_gap: Option<(u32, u32)>,
    /// What's wrong, or worth knowing, about where it landed.
    pub warnings: Vec<String>,
}

const fn area(start: u32, end: u32, kind: Kind, name: &'static str) -> Area {
    Area { start, end, kind, name }
}

/// A machine's 64K, if Limoniter knows it.
pub fn layout(machine: Machine) -> Option<Vec<Area>> {
    let spectrum = |paged: bool| {
        let mut areas = vec![
            area(0x0000, 0x4000, Kind::Rom, "ROM"),
            area(0x4000, 0x5800, Kind::Screen, "the screen's pixels"),
            area(0x5800, 0x5B00, Kind::Screen, "the screen's colours"),
            area(0x5B00, 0x5C00, Kind::System, "the printer buffer"),
            area(0x5C00, 0x5CCB, Kind::System, "the system variables"),
            area(0x5CCB, crate::asm::output::LOWEST_FOR_TAPE as u32, Kind::System, "BASIC, with the loader and its variables"),
            area(crate::asm::output::LOWEST_FOR_TAPE as u32, 0x8000, Kind::Slow, "contended memory"),
        ];
        if paged {
            areas.push(area(0x8000, 0xC000, Kind::Free, "bank 2"));
            areas.push(area(0xC000, 0x10000, Kind::Paged, "the paged top 16K (bank 0 to begin with)"));
        } else {
            areas.push(area(0x8000, 0xFF58, Kind::Free, "free memory"));
            areas.push(area(0xFF58, 0x10000, Kind::System, "the UDGs"));
        }
        areas
    };
    Some(match machine {
        Machine::Spectrum48 | Machine::OpticOs => spectrum(false),
        Machine::Spectrum128 | Machine::SpectrumPlus3 | Machine::Pentagon => spectrum(true),
        Machine::C64 => vec![
            area(0x0000, 0x0100, Kind::System, "zero page"),
            area(0x0100, 0x0200, Kind::System, "the stack"),
            area(0x0200, 0x0400, Kind::System, "the KERNAL's and BASIC's workspace"),
            area(0x0400, 0x0800, Kind::Screen, "the screen (and the sprite pointers)"),
            area(0x0800, 0xA000, Kind::Free, "BASIC's memory, free"),
            area(0xA000, 0xC000, Kind::Rom, "the BASIC ROM"),
            area(0xC000, 0xD000, Kind::Free, "free memory"),
            area(0xD000, 0xE000, Kind::Io, "the I/O: the VIC-II, SID, colour memory and CIAs"),
            area(0xE000, 0x10000, Kind::Rom, "the KERNAL ROM"),
        ],
        Machine::Bbc => vec![
            area(0x0000, 0x0100, Kind::System, "zero page"),
            area(0x0100, 0x0200, Kind::System, "the stack"),
            area(0x0200, 0x0E00, Kind::System, "the MOS's workspace"),
            area(0x0E00, 0x1900, Kind::System, "the disc system's workspace"),
            area(0x1900, 0x3000, Kind::Free, "free memory (from PAGE)"),
            area(0x3000, 0x7C00, Kind::Slow, "screen memory in MODEs 0 to 6 (free in MODE 7)"),
            area(0x7C00, 0x8000, Kind::Screen, "MODE 7's screen"),
            area(0x8000, 0xC000, Kind::Rom, "the sideways ROM (BASIC)"),
            area(0xC000, 0xFC00, Kind::Rom, "the MOS ROM"),
            area(0xFC00, 0xFF00, Kind::Io, "the I/O: FRED, JIM and SHEILA"),
            area(0xFF00, 0x10000, Kind::Rom, "the MOS ROM"),
        ],
        Machine::CoCo => vec![
            area(0x0000, 0x0400, Kind::System, "the system's workspace"),
            area(0x0400, 0x0600, Kind::Screen, "the text screen"),
            area(0x0600, 0x1E00, Kind::Slow, "the graphics pages"),
            area(0x1E00, 0x8000, Kind::Free, "free memory"),
            area(0x8000, 0xA000, Kind::Rom, "Extended Color BASIC"),
            area(0xA000, 0xC000, Kind::Rom, "Color BASIC"),
            area(0xC000, 0xFF00, Kind::Rom, "the cartridge's space"),
            area(0xFF00, 0x10000, Kind::Io, "the I/O"),
        ],
        Machine::Atari2600 => vec![
            area(0x0000, 0x0080, Kind::Io, "the TIA"),
            area(0x0080, 0x0100, Kind::System, "RAM (and the stack)"),
            area(0x0100, 0x0280, Kind::Io, "the TIA's mirrors"),
            area(0x0280, 0x0300, Kind::Io, "the RIOT"),
            area(0x0300, 0xF000, Kind::Io, "not there"),
            area(0xF000, 0xFFFA, Kind::Cartridge, "the cartridge"),
            area(0xFFFA, 0x10000, Kind::System, "the vectors"),
        ],
        _ => return None,
    })
}

/// What landing in an area means, or nothing if it's fine.
fn warning(machine: Machine, a: &Area, from: u32, to: u32) -> Option<String> {
    let at = if to - from == 1 { format!("${from:04X}") } else { format!("${from:04X}-${:04X}", to - 1) };
    let spectrum = matches!(machine, Machine::Spectrum48 | Machine::Spectrum128 | Machine::SpectrumPlus3 | Machine::Pentagon | Machine::OpticOs);
    Some(match (a.kind, machine) {
        (Kind::Free | Kind::Cartridge, _) => return None,
        (Kind::Rom, Machine::C64) => crate::i18n::tf!("{at} is under {}: it loads into the RAM beneath, but only runs there with the ROM switched out ($01).", a.name),
        (Kind::Rom, _) => crate::i18n::tf!("{at} is in {}, where nothing can be written: it won't be there.", a.name),
        (Kind::Io, Machine::C64) => crate::i18n::tf!("{at} is under {}: it can't be loaded or run there as it is.", a.name),
        (Kind::Io, _) => crate::i18n::tf!("{at} is in {}, where there's no memory for it.", a.name),
        (Kind::Screen, _) => crate::i18n::tf!("{at} is over {}: it shows there, and anything printed overwrites it.", a.name),
        (Kind::System, Machine::Atari2600) if a.name == "the vectors" => return None,
        (Kind::System, _) if spectrum && a.name == "the UDGs" => crate::i18n::tf!("{at} is over the UDGs: fine unless the program uses them."),
        (Kind::System, _) => crate::i18n::tf!("{at} is over {}, which the system uses.", a.name),
        (Kind::Slow, _) if spectrum => crate::i18n::tf!("{at} is in contended memory, shared with the screen: code there runs slower while the screen's drawn."),
        (Kind::Slow, Machine::Bbc) => crate::i18n::tf!("{at} is screen memory in MODEs 0 to 6: only MODE 7 leaves it free."),
        (Kind::Slow, _) => format!("{at} is over {}.", a.name),
        (Kind::Paged, _) => crate::i18n::tf!("{at} is in the 128's paged top 16K: bank 0 is there when the program starts, and paging another in hides it."),
    })
}

/// Where a program's bytes are in its machine, if Limoniter knows the machine.
pub fn map(machine: Machine, chunks: &[(i64, usize)]) -> Option<Map> {
    let areas = layout(machine)?;
    let mut used: Vec<(u32, u32)> = chunks
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|&(at, n)| {
            // The 2600's cartridge is as often written at $1000 as $F000: the same 4K.
            let at = if machine == Machine::Atari2600 && (0x1000..0x2000).contains(&at) { at + 0xE000 } else { at };
            (at.clamp(0, 0x10000) as u32, (at + n as i64).clamp(0, 0x10000) as u32)
        })
        .filter(|(a, b)| b > a)
        .collect();
    used.sort();
    let mut joined: Vec<(u32, u32)> = Vec::new();
    for (a, b) in used {
        match joined.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => joined.push((a, b)),
        }
    }
    let mut warnings = Vec::new();
    for area in &areas {
        for &(a, b) in &joined {
            let (from, to) = (a.max(area.start), b.min(area.end));
            if from < to
                && let Some(w) = warning(machine, area, from, to)
            {
                warnings.push(w);
            }
        }
    }
    // What's free: the areas programs can use, less what the program uses; and the biggest stretch of it.
    let mut free = 0;
    let mut biggest: Option<(u32, u32)> = None;
    let mut consider = |from: u32, to: u32| {
        if to > from {
            free += to - from;
            if biggest.is_none_or(|(a, b)| to - from > b - a) {
                biggest = Some((from, to));
            }
        }
    };
    for area in areas.iter().filter(|a| a.kind.is_free()) {
        let mut at = area.start;
        for &(a, b) in joined.iter().filter(|(a, b)| *b > area.start && *a < area.end) {
            consider(at, a.max(at).min(area.end));
            at = at.max(b.min(area.end));
        }
        consider(at, area.end);
    }
    Some(Map { machine, areas, used: joined, free, biggest_gap: biggest, warnings })
}

impl Map {
    pub fn bytes_used(&self) -> u32 {
        self.used.iter().map(|(a, b)| b - a).sum()
    }

    /// A line for the Output panel.
    pub fn summary(&self) -> String {
        let gap = self.biggest_gap.map_or(String::new(), |(a, b)| crate::i18n::tf!("; the biggest stretch is ${a:04X}-${:04X}, {} bytes", b - 1, b - a));
        crate::i18n::tf!("Memory: {} bytes of the program, {} bytes still free{gap}.", self.bytes_used(), self.free)
    }

    /// What's at an address: its area, and whether the program's there.
    pub fn at(&self, address: u32) -> (Option<&Area>, bool) {
        (self.areas.iter().find(|a| (a.start..a.end).contains(&address)), self.used.iter().any(|&(a, b)| (a..b).contains(&address)))
    }
}

/// The map as a bar across the panel: the machine's areas in their colours, the program over them, and what's
/// where on hovering.
pub fn bar(ui: &mut eframe::egui::Ui, map: &Map) {
    use eframe::egui::{Color32, Pos2, Rect, Sense, Stroke, vec2};
    let width = ui.available_width().max(100.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 18.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let x_of = |address: u32| rect.left() + address as f32 / 65536.0 * rect.width();
    let colour = |kind: Kind| match kind {
        Kind::Free | Kind::Cartridge => Color32::from_rgb(70, 78, 70),
        Kind::Slow => Color32::from_rgb(110, 100, 60),
        Kind::Paged => Color32::from_rgb(60, 80, 110),
        Kind::Rom => Color32::from_rgb(110, 60, 60),
        Kind::Screen => Color32::from_rgb(60, 110, 80),
        Kind::System => Color32::from_rgb(95, 80, 110),
        Kind::Io => Color32::from_rgb(130, 90, 50),
    };
    for a in &map.areas {
        painter.rect_filled(Rect::from_min_max(Pos2::new(x_of(a.start), rect.top() + 3.0), Pos2::new(x_of(a.end), rect.bottom() - 3.0)), 0.0, colour(a.kind));
    }
    let used = crate::theme::accent();
    for &(a, b) in &map.used {
        let (left, right) = (x_of(a), x_of(b).max(x_of(a) + 2.0));
        painter.rect_filled(Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom())), 1.0, used);
    }
    for k in 1..16 {
        let x = x_of(k * 0x1000);
        painter.vline(x, rect.top() + 3.0..=rect.top() + 6.0, Stroke::new(1.0, Color32::from_gray(160)));
    }
    if let Some(pos) = response.hover_pos() {
        let address = (((pos.x - rect.left()) / rect.width()).clamp(0.0, 0.99999) * 65536.0) as u32;
        let (area, program) = map.at(address);
        let what = area.map_or(String::new(), |a| format!("{} (${:04X}-${:04X})", a.name, a.start, a.end - 1));
        response.on_hover_text(crate::i18n::tf!("${address:04X}: {what}{}", if program { ", the program's" } else { "" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layout_covers_the_64k_in_order() {
        for machine in [Machine::Spectrum48, Machine::Spectrum128, Machine::C64, Machine::Bbc, Machine::CoCo, Machine::Atari2600] {
            let areas = layout(machine).unwrap();
            assert_eq!(areas[0].start, 0, "{machine:?}");
            assert_eq!(areas.last().unwrap().end, 0x10000, "{machine:?}");
            assert!(areas.windows(2).all(|w| w[0].end == w[1].start && w[0].start < w[0].end), "{machine:?}");
        }
        assert!(layout(Machine::Amiga).is_none());
    }

    #[test]
    fn a_spectrum_program_in_free_memory_says_nothing_and_counts_what_s_left() {
        let program = map(Machine::Spectrum48, &[(0x8000, 0x100), (0x8100, 0x50)]).unwrap();
        let map = program;
        assert_eq!(map.used, [(0x8000, 0x8150)]);
        assert!(map.warnings.is_empty(), "{:?}", map.warnings);
        assert_eq!(map.bytes_used(), 0x150);
        let contended = 0x8000 - crate::asm::output::LOWEST_FOR_TAPE as u32;
        assert_eq!(map.free, contended + (0xFF58 - 0x8150));
        assert_eq!(map.biggest_gap, Some((0x8150, 0xFF58)));
        assert!(map.summary().contains("336 bytes of the program"));
    }

    #[test]
    fn landing_somewhere_it_can_t_go_is_said() {
        let spectrum = map(Machine::Spectrum48, &[(0x5000, 0x1000), (0x3FF0, 0x20)]).unwrap();
        let said = spectrum.warnings.join("\n");
        assert!(said.contains("$3FF0-$3FFF is in ROM"), "{said}");
        assert!(said.contains("over the screen's pixels"), "{said}");
        assert!(said.contains("over the screen's colours"), "{said}");
        assert!(said.contains("contended memory"), "{said}");
        let c64 = map(Machine::C64, &[(0xC000, 0x1100)]).unwrap();
        assert_eq!(c64.warnings, ["$D000-$D0FF is under the I/O: the VIC-II, SID, colour memory and CIAs: it can't be loaded or run there as it is."]);
        let paged = map(Machine::Spectrum128, &[(0xBF00, 0x200)]).unwrap();
        assert!(paged.warnings[0].contains("$C000-$C0FF is in the 128's paged top 16K"), "{:?}", paged.warnings);
        // A 2600 cartridge written at $1000 is the same 4K as at $F000, its vectors and all.
        let cart = map(Machine::Atari2600, &[(0x1000, 0x1000)]).unwrap();
        assert!(cart.warnings.is_empty(), "{:?}", cart.warnings);
        assert_eq!(cart.free, 0);
    }
}
