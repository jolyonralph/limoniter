//! The window's side of the trainer (see `trainer`) and of the debug console port (see `debug_console`): the
//! "Trainer" box, opened from Run on, for the machine running in front; freezing and blocking applied each frame
//! to whatever machine a frozen program is running on, box open or not; and the lines programs print to their
//! console port, gathered into the Output panel.

use super::run_host::SessionKind;
use super::*;
use crate::trainer::{self, Finder, Freeze, Search, Width};

/// How many candidates are listed at a time; past this the search needs narrowing.
const MOST_LISTED: usize = 60;

/// What the Trainer box keeps while it's open.
pub(super) struct TrainerBox {
    finder: Finder,
    /// The number typed for Equals, the value a frozen address is first held at, and the address for one added
    /// by hand.
    equals: String,
    hold: String,
    new_address: String,
    /// What to tell the person, such as a typed number that wasn't one.
    said: String,
    /// The program the finder's snapshot is of: a different one is a different search.
    program: String,
}

impl TrainerBox {
    pub(super) fn new() -> TrainerBox {
        TrainerBox { finder: Finder::default(), equals: String::new(), hold: String::new(), new_address: String::new(), said: String::new(), program: String::new() }
    }
}

/// A machine's memory as the trainer sees it.
struct Ram {
    /// The address of its first byte.
    base: u32,
    bytes: Vec<u8>,
    /// Whether it keeps 16-bit values big-endian (the 6809's order), or little-endian.
    big_endian: bool,
    /// Whether the machine can stop writes to an address (a core's memory can only have a value written back).
    can_block: bool,
    name: &'static str,
}

/// The machine's memory, if it's a kind the trainer can reach.
fn ram_of(kind: &mut SessionKind) -> Option<Ram> {
    match kind {
        SessionKind::BuiltIn { machine, .. } => Some(Ram { base: 0x4000, bytes: machine.memory()[0x4000..].to_vec(), big_endian: false, can_block: true, name: "Spectrum" }),
        SessionKind::Atari2600 { machine, .. } => Some(Ram { base: 0x80, bytes: (0x80..=0xFFu16).map(|a| machine.peek(a)).collect(), big_endian: false, can_block: true, name: "Atari 2600" }),
        SessionKind::Vectrex { machine, .. } => match machine.as_ref() {
            super::vectrex_run_host::VectrexRun::Running(vectrex) => Some(Ram { base: 0xC800, bytes: vectrex.memory()[0xC800..0xD000].to_vec(), big_endian: true, can_block: true, name: "Vectrex" }),
            super::vectrex_run_host::VectrexRun::NeedsBios => None,
        },
        SessionKind::Micro { machine, .. } => {
            let name = machine.console_name();
            machine.trainer_ram().map(|bytes| Ram { base: micro_host::TRAINER_BASE as u32, bytes, big_endian: false, can_block: true, name })
        }
        SessionKind::Retro { machine, .. } => machine.trainer_ram().map(|bytes| Ram { base: 0, bytes: bytes.to_vec(), big_endian: false, can_block: false, name: "" }),
        SessionKind::App { .. } | SessionKind::Teaching { .. } => None,
    }
}

/// Whether the trainer can reach this kind of session at all.
fn reachable(kind: &SessionKind) -> bool {
    match kind {
        SessionKind::Vectrex { machine, .. } => matches!(machine.as_ref(), super::vectrex_run_host::VectrexRun::Running(_)),
        SessionKind::BuiltIn { .. } | SessionKind::Atari2600 { .. } | SessionKind::Micro { .. } | SessionKind::Retro { .. } => true,
        SessionKind::App { .. } | SessionKind::Teaching { .. } => false,
    }
}

/// Writes bytes into a machine's memory.
fn poke(kind: &mut SessionKind, address: u32, bytes: &[u8]) {
    let at = address as u16;
    match kind {
        SessionKind::BuiltIn { machine, .. } => machine.poke(at, bytes),
        SessionKind::Atari2600 { machine, .. } => machine.poke(at, bytes),
        SessionKind::Vectrex { machine, .. } => {
            if let super::vectrex_run_host::VectrexRun::Running(vectrex) = machine.as_mut() {
                vectrex.poke(at, bytes);
            }
        }
        SessionKind::Micro { machine, .. } => machine.trainer_poke(at, bytes),
        SessionKind::Retro { machine, .. } => machine.trainer_poke(address, bytes),
        SessionKind::App { .. } | SessionKind::Teaching { .. } => {}
    }
}

/// Stops writes to these addresses on a machine that can (a core can't: its frozen values are only written back).
fn block(kind: &mut SessionKind, addresses: Vec<u16>) {
    match kind {
        SessionKind::BuiltIn { machine, .. } => machine.set_blocked(addresses),
        SessionKind::Atari2600 { machine, .. } => machine.set_blocked(addresses),
        SessionKind::Vectrex { machine, .. } => {
            if let super::vectrex_run_host::VectrexRun::Running(vectrex) = machine.as_mut() {
                vectrex.set_blocked(addresses);
            }
        }
        SessionKind::Micro { machine, .. } => machine.trainer_block(addresses),
        SessionKind::Retro { .. } | SessionKind::App { .. } | SessionKind::Teaching { .. } => {}
    }
}

/// Holds a session's frozen addresses where they are: values written back, writes blocked. Whether it can block.
fn hold(kind: &mut SessionKind, freezes: &[Freeze], big_endian: bool, can_block: bool) {
    let mut blocked = Vec::new();
    for (f, (address, bytes)) in freezes.iter().filter(|f| f.on).zip(trainer::writes_due(freezes, big_endian)) {
        poke(kind, address, &bytes);
        if f.block && can_block {
            blocked.extend((0..bytes.len() as u32).map(|i| address.wrapping_add(i) as u16));
        }
    }
    if can_block {
        block(kind, blocked);
    }
}

/// How 16-bit values are kept on each kind of session: big-endian on the Vectrex's 6809 only.
fn big_endian_of(kind: &SessionKind) -> bool {
    matches!(kind, SessionKind::Vectrex { .. })
}

/// Whether writes can be stopped on a session, as `Ram::can_block` says without reading the memory.
fn can_block_of(kind: &SessionKind) -> bool {
    !matches!(kind, SessionKind::Retro { .. })
}

/// The machine's name for the debug console's lines, and the lines it has printed since last asked.
fn console_of(kind: &mut SessionKind) -> Option<(&'static str, Vec<String>)> {
    match kind {
        SessionKind::BuiltIn { machine, .. } => Some(("Spectrum", machine.take_console())),
        SessionKind::Atari2600 { machine, .. } => Some(("Atari 2600", machine.take_console())),
        SessionKind::Vectrex { machine, .. } => match machine.as_mut() {
            super::vectrex_run_host::VectrexRun::Running(vectrex) => Some(("Vectrex", vectrex.take_console())),
            super::vectrex_run_host::VectrexRun::NeedsBios => None,
        },
        SessionKind::Micro { machine, .. } => Some((machine.console_name(), machine.take_console())),
        // A core has no port of ours; Teaching machines have no such ports at all.
        SessionKind::Retro { .. } | SessionKind::App { .. } | SessionKind::Teaching { .. } => None,
    }
}

impl Limoniter {
    /// Each frame: what programs have printed to their console ports goes to the Output panel, and what's frozen
    /// is held, for every machine the trainer reaches, whether or not its box is open.
    pub(super) fn trainer_tick(&mut self) {
        let mut printed = Vec::new();
        for session in &mut self.running.sessions {
            if let Some((name, lines)) = console_of(&mut session.kind) {
                printed.extend(lines.into_iter().map(|line| crate::debug_console::output_line(name, &line)));
            }
            let freezes = self.trainer_kept.of(&session.file);
            if !freezes.is_empty() || self.trainer_holding.contains(&session.file) {
                if reachable(&session.kind) {
                    let (big, can_block) = (big_endian_of(&session.kind), can_block_of(&session.kind));
                    hold(&mut session.kind, freezes, big, can_block);
                }
                if freezes.is_empty() {
                    self.trainer_holding.remove(&session.file);
                } else {
                    self.trainer_holding.insert(session.file.clone());
                }
            }
        }
        for line in printed {
            self.script_print(line, false);
        }
    }

    /// The session the trainer is for: the one run from the document in front, or else the newest it can reach.
    fn trainer_session(&self) -> Option<usize> {
        let doc_id = self.docs.get(self.active).map(|doc| doc.id);
        let own = |s: &run_host::Session| match &s.kind {
            SessionKind::BuiltIn { doc_id: d, .. } | SessionKind::Atari2600 { doc_id: d, .. } | SessionKind::Vectrex { doc_id: d, .. } | SessionKind::Micro { doc_id: d, .. } | SessionKind::Retro { doc_id: d, .. } => Some(*d) == doc_id,
            _ => false,
        };
        let sessions = &self.running.sessions;
        sessions.iter().position(|s| own(s) && reachable(&s.kind)).or_else(|| sessions.iter().rposition(|s| reachable(&s.kind)))
    }

    pub(super) fn open_trainer(&mut self) {
        if self.trainer.is_none() {
            self.trainer = Some(Box::new(TrainerBox::new()));
        }
    }

    /// The Trainer box.
    pub(super) fn trainer_window(&mut self, ctx: &Context) {
        let Some(mut tb) = self.trainer.take() else { return };
        let mut open = true;
        let index = self.trainer_session();
        let (program, ram) = match index {
            Some(i) => {
                let session = &mut self.running.sessions[i];
                (Some(session.file.clone()), ram_of(&mut session.kind))
            }
            None => (None, None),
        };
        let mut freezes = program.as_deref().map(|p| self.trainer_kept.of(p).to_vec()).unwrap_or_default();
        let before = freezes.clone();
        let mut go_to: Option<u32> = None;
        crate::fit::Dialog::new(crate::i18n::tr("Trainer")).open(&mut open).collapsible(false).resizable(true).default_width(560.0).show(ctx, |ui| {
            match (&program, &ram) {
                (Some(program), Some(ram)) => draw(ui, &mut tb, program, ram, &mut freezes, &mut go_to),
                _ => {
                    ui.label(crate::i18n::tr("Run a program on a built-in machine, or in a core (File, Run on), and this finds and holds the numbers it keeps in memory: lives, energy, a score."));
                }
            }
        });
        if let Some(program) = &program {
            if freezes != before {
                self.trainer_kept.set(program, freezes);
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if let (Some(address), Some(ram)) = (go_to, &ram) {
            self.open_memory_at(ram.name, ram.base, &ram.bytes, address);
        }
        if open {
            self.trainer = Some(tb);
        }
    }

    /// For a screenshot: the box opened over the machine in front, a snapshot taken, `frames` frames run, and the
    /// addresses that changed kept, with the first frozen and named.
    pub(super) fn trainer_for_screenshot(&mut self, frames: usize) {
        self.open_trainer();
        let Some(i) = self.trainer_session() else { return };
        let (program, ram) = (self.running.sessions[i].file.clone(), ram_of(&mut self.running.sessions[i].kind));
        let Some(ram) = ram else { return };
        let mut tb = self.trainer.take().unwrap_or_else(|| Box::new(TrainerBox::new()));
        tb.program = program.clone();
        tb.finder.big_endian = ram.big_endian;
        tb.finder.start(ram.base, &ram.bytes);
        self.run_frames_for_screenshot(frames);
        if let Some(now) = ram_of(&mut self.running.sessions[i].kind) {
            tb.finder.narrow(Search::Changed, &now.bytes);
            if let Some(&address) = tb.finder.candidates().first() {
                self.trainer_kept.set(&program, vec![Freeze { address, value: 3, width: Width::Byte, name: crate::i18n::tr("Lives").into(), on: true, block: false }]);
            }
        }
        self.trainer = Some(tb);
    }

    /// A new hex document of the machine's memory as it is now, with the cursor at an address.
    fn open_memory_at(&mut self, name: &str, base: u32, bytes: &[u8], address: u32) {
        let mut memory = vec![0; base as usize];
        memory.extend_from_slice(bytes);
        let title = if name.is_empty() { crate::i18n::tr("Machine memory").to_string() } else { crate::i18n::tf!("{name} memory") };
        let mut doc = Document::from_image_file(title, Vec::new(), None);
        doc.hex.load(memory);
        doc.hex_mode = true;
        doc.hex.go_to(&format!("0x{address:X}"));
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }
}

/// What a value is shown as: decimal, with hex beside it.
fn shown(value: Option<u32>, width: Width) -> String {
    match (value, width) {
        (None, _) => "-".into(),
        (Some(v), Width::Byte) => format!("{v} (${v:02X})"),
        (Some(v), Width::Word) => format!("{v} (${v:04X})"),
    }
}

fn draw(ui: &mut Ui, tb: &mut TrainerBox, program: &str, ram: &Ram, freezes: &mut Vec<Freeze>, go_to: &mut Option<u32>) {
    if tb.program != program {
        tb.program = program.to_string();
        tb.finder = Finder::default();
        tb.said.clear();
    }
    tb.finder.big_endian = ram.big_endian;
    let machine = if ram.name.is_empty() { program.to_string() } else { format!("{} ({program})", ram.name) };
    ui.label(RichText::new(crate::i18n::tf!("Trainer for {machine}")).strong());
    ui.label(RichText::new(crate::i18n::tr("Press Start, do something in the game that changes the number (lose a life), then narrow the addresses down by what happened. Repeat until a few are left, then freeze one.")).color(theme::text_dim()).size(12.5));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Looking for"));
        let mut width = tb.finder.width;
        ui.radio_value(&mut width, Width::Byte, crate::i18n::tr("an 8-bit number"));
        let word = if ram.big_endian { crate::i18n::tr("a 16-bit number (big-endian)") } else { crate::i18n::tr("a 16-bit number (little-endian)") };
        ui.radio_value(&mut width, Width::Word, word);
        tb.finder.set_width(width);
    });
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Start")).on_hover_text(crate::i18n::tr("Takes a snapshot of the machine's memory, with every address a candidate")).clicked() {
            tb.finder.start(ram.base, &ram.bytes);
            tb.said.clear();
        }
        let started = tb.finder.started();
        ui.add_enabled_ui(started, |ui| {
            for (label, hover, search) in [
                (crate::i18n::tr("Changed"), crate::i18n::tr("Keeps the addresses whose value is different from the last snapshot"), Search::Changed),
                (crate::i18n::tr("Not changed"), crate::i18n::tr("Keeps the addresses whose value is the same as in the last snapshot"), Search::NotChanged),
                (crate::i18n::tr("Went up"), crate::i18n::tr("Keeps the addresses whose value is bigger than in the last snapshot"), Search::WentUp),
                (crate::i18n::tr("Went down"), crate::i18n::tr("Keeps the addresses whose value is smaller than in the last snapshot"), Search::WentDown),
            ] {
                if ui.button(label).on_hover_text(hover).clicked() {
                    tb.finder.narrow(search, &ram.bytes);
                    tb.said.clear();
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_enabled_ui(tb.finder.started(), |ui| {
            ui.add(egui::TextEdit::singleline(&mut tb.equals).desired_width(90.0).hint_text(crate::i18n::tr("a number")));
            let equals = ui.button(crate::i18n::tr("Equals")).on_hover_text(crate::i18n::tr("Keeps the addresses that hold this number now (decimal, or hex with $ before it)"));
            if equals.clicked() {
                match trainer::parse_number(&tb.equals) {
                    Some(n) => {
                        tb.finder.narrow(Search::Equals(n), &ram.bytes);
                        tb.said.clear();
                    }
                    None => tb.said = crate::i18n::tr("Type a number, such as 3 or $FF.").into(),
                }
            }
        });
    });
    if !tb.said.is_empty() {
        ui.label(RichText::new(tb.said.clone()).color(theme::accent()));
    }
    if tb.finder.started() {
        let count = tb.finder.count();
        ui.label(crate::i18n::tf!("Addresses left: {count}"));
        if count > MOST_LISTED {
            ui.label(RichText::new(crate::i18n::tf!("The first {MOST_LISTED} are listed: narrow them down to see more.")).color(theme::text_dim()).size(12.5));
        }
        candidates(ui, tb, ram, freezes, go_to);
    }
    ui.separator();
    frozen(ui, tb, ram, freezes, go_to);
}

fn candidates(ui: &mut Ui, tb: &mut TrainerBox, ram: &Ram, freezes: &mut Vec<Freeze>, go_to: &mut Option<u32>) {
    let width = tb.finder.width;
    let shown_addresses: Vec<u32> = tb.finder.candidates().iter().copied().take(MOST_LISTED).collect();
    if shown_addresses.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Freeze at"));
        ui.add(egui::TextEdit::singleline(&mut tb.hold).desired_width(100.0).hint_text(crate::i18n::tr("its value now"))).on_hover_text(crate::i18n::tr("The value a frozen address is held at; left empty, it's held at the value it has now"));
    });
    egui::ScrollArea::vertical().max_height(crate::fit::height_left(ui).clamp(80.0, 220.0)).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("trainer-candidates").striped(true).show(ui, |ui| {
            for label in [crate::i18n::tr("Address"), crate::i18n::tr("Now"), crate::i18n::tr("Before"), crate::i18n::tr("16-bit now")] {
                ui.label(RichText::new(label).color(theme::text_dim()).size(12.0));
            }
            ui.end_row();
            for &address in &shown_addresses {
                let now = trainer::read(&ram.bytes, ram.base, address, width, ram.big_endian);
                let before = tb.finder.before(address);
                let word = trainer::read(&ram.bytes, ram.base, address, Width::Word, ram.big_endian);
                ui.label(RichText::new(format!("${address:04X}")).monospace());
                ui.label(shown(now, width));
                ui.label(shown(before, width));
                ui.label(shown(word, Width::Word));
                if ui.small_button(crate::i18n::tr("Freeze")).on_hover_text(crate::i18n::tr("Holds this address at the value, written back every frame")).clicked() {
                    let value = trainer::parse_number(&tb.hold).or(now).unwrap_or(0);
                    if !freezes.iter().any(|f| f.address == address) {
                        freezes.push(Freeze { address, value, width, name: String::new(), on: true, block: false });
                    }
                }
                if ui.small_button(crate::i18n::tr("Go to in memory")).on_hover_text(crate::i18n::tr("Opens the machine's memory, as it is now, at this address")).clicked() {
                    *go_to = Some(address);
                }
                ui.end_row();
            }
        });
    });
}

fn frozen(ui: &mut Ui, tb: &mut TrainerBox, ram: &Ram, freezes: &mut Vec<Freeze>, go_to: &mut Option<u32>) {
    ui.label(RichText::new(crate::i18n::tr("Frozen addresses")).strong());
    ui.label(RichText::new(crate::i18n::tr("Kept for this program, and held whenever it runs, with this box open or not.")).color(theme::text_dim()).size(12.5));
    let mut remove = None;
    egui::Grid::new("trainer-frozen").num_columns(6).striped(true).show(ui, |ui| {
        for (i, f) in freezes.iter_mut().enumerate() {
            ui.checkbox(&mut f.on, "").on_hover_text(crate::i18n::tr("Holds this address while it's ticked"));
            ui.add(egui::TextEdit::singleline(&mut f.name).desired_width(120.0).hint_text(crate::i18n::tr("a name, such as Lives")));
            ui.label(RichText::new(format!("${:04X}", f.address)).monospace());
            let mut text = f.value.to_string();
            if ui.add(egui::TextEdit::singleline(&mut text).desired_width(60.0)).on_hover_text(crate::i18n::tr("The value it's held at")).changed()
                && let Some(value) = trainer::parse_number(&text)
            {
                f.value = value.min(if f.width == Width::Byte { 0xFF } else { 0xFFFF });
            }
            let block = ui.add_enabled(ram.can_block, egui::Checkbox::new(&mut f.block, crate::i18n::tr("Block writes")));
            block.on_hover_text(crate::i18n::tr("Stops the machine's own writes to this address, rather than putting the value back each frame")).on_disabled_hover_text(crate::i18n::tr("A core's memory can only have the value written back each frame; its writes can't be stopped"));
            ui.horizontal(|ui| {
                if ui.small_button(crate::i18n::tr("Go to in memory")).clicked() {
                    *go_to = Some(f.address);
                }
                if ui.small_button(crate::i18n::tr("Remove")).clicked() {
                    remove = Some(i);
                }
            });
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        freezes.remove(i);
    }
    if freezes.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("Nothing is frozen.")).color(theme::text_dim()));
    }
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Add by hand"));
        ui.add(egui::TextEdit::singleline(&mut tb.new_address).desired_width(120.0).hint_text(crate::i18n::tr("an address, in hex")));
        ui.add(egui::TextEdit::singleline(&mut tb.hold).desired_width(80.0).hint_text(crate::i18n::tr("a value")));
        if ui.button(crate::i18n::tr("Freeze")).clicked() {
            let text = tb.new_address.trim().trim_start_matches('$').trim_start_matches("0x");
            match (u32::from_str_radix(text, 16), trainer::parse_number(&tb.hold)) {
                (Ok(address), Some(value)) => {
                    if !freezes.iter().any(|f| f.address == address) {
                        freezes.push(Freeze { address, value, width: tb.finder.width, name: String::new(), on: true, block: false });
                    }
                    tb.said.clear();
                }
                _ => tb.said = crate::i18n::tr("Type an address in hex and a value, such as 5C00 and 3.").into(),
            }
        }
    });
    let note = if ram.can_block { crate::i18n::tr("Freezing writes the value back every frame. Block writes stops the machine writing to the address at all.") } else { crate::i18n::tr("This machine runs in a core, which can only have the value written back each frame: its writes can't be blocked.") };
    ui.label(RichText::new(note).color(theme::text_dim()).size(12.5));
}
