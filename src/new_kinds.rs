//! New as: new files of a kind, each with its file ending, encoding, line endings, colouring, the BASIC it's a
//! listing for, and text to start with. Limoniter comes with a list, which can be changed, added to and put back
//! in Settings or from the menu; a changed list is kept in the settings file.

use super::*;

/// What a new file of a kind is a listing for, so that F9 and Send to device tokenise it, and Spectrum BASIC gets
/// GRAPH mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Program {
    #[default]
    None,
    Spectrum,
    Next,
    C64,
    Bbc,
}

impl Program {
    pub const ALL: [Self; 5] = [Self::None, Self::Spectrum, Self::Next, Self::C64, Self::Bbc];

    fn id(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Spectrum => "spectrum",
            Self::Next => "next",
            Self::C64 => "c64",
            Self::Bbc => "bbc",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "Nothing: just text",
            Self::Spectrum => "A Spectrum program (.tap)",
            Self::Next => "A Spectrum Next program (.bas)",
            Self::C64 => "A Commodore 64 program (.prg)",
            Self::Bbc => "A BBC BASIC program",
        }
    }

    fn machine(self) -> Option<basic::Machine> {
        match self {
            Self::None => None,
            Self::Spectrum => Some(basic::Machine::Spectrum),
            Self::Next => Some(basic::Machine::Next),
            Self::C64 => Some(basic::Machine::Commodore(basic::CbmMachine::C64)),
            Self::Bbc => Some(basic::Machine::Bbc),
        }
    }
}

/// The line endings a kind can have.
const EOLS: [Eol; 5] = [Eol::CrLf, Eol::Lf, Eol::Cr, Eol::Atascii, Eol::Nl];

/// A kind of new file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NewKind {
    pub name: String,
    /// Without the dot; empty for none.
    pub ending: String,
    /// A character set's id, as the settings file and Reopen know it.
    pub encoding: String,
    pub eol: Eol,
    /// A colouring's name, as the Language menu has it; empty to go by the ending.
    pub language: String,
    pub program: Program,
    /// The text it starts with.
    pub starter: String,
}

impl NewKind {
    fn new(name: &str, ending: &str, encoding: &str, eol: Eol, language: &str, program: Program, starter: &str) -> Self {
        Self { name: name.into(), ending: ending.into(), encoding: encoding.into(), eol, language: language.into(), program, starter: starter.into() }
    }

    /// The kinds Limoniter comes with.
    pub fn usual() -> Vec<Self> {
        let line = if cfg!(windows) { Eol::CrLf } else { Eol::Lf };
        vec![
            Self::new("Spectrum BASIC", "bas", "utf-8", line, highlight::BASIC_SINCLAIR, Program::Spectrum, "10 PRINT \"Hello from Limoniter\""),
            Self::new("Spectrum Next BASIC", "bas", "utf-8", line, highlight::BASIC_SINCLAIR, Program::Next, "10 PRINT \"Hello from Limoniter\""),
            Self::new("Commodore 64 BASIC", "bas", "utf-8", line, highlight::BASIC_COMMODORE, Program::C64, "10 PRINT \"HELLO FROM LIMONITER\""),
            Self::new("BBC BASIC", "bas", "utf-8", line, "", Program::Bbc, "10 PRINT \"Hello from Limoniter\""),
            Self::new("Z80 assembler", "asm", "utf-8", line, "Assembly Z80", Program::None, "        org 32768\nstart:  ld a,2\n        out (254),a         ; a red border\n        ret"),
            Self::new("6502 assembler", "a65", "utf-8", line, "Assembly 6502", Program::None, "        * = $c000\nstart:  lda #2\n        sta $d020           ; a red border\n        rts"),
            Self::new("6809 assembler", "asm", "utf-8", line, "Assembly 6809", Program::None, "        org $3000\nstart:  lda #1\n        rts\n        end start"),
            Self::new("68000 assembler", "s", "utf-8", line, "Assembly 680x0", Program::None, "start:  clr.w -(sp)\n        trap #1             ; back to the desktop"),
            Self::new("Markdown", "md", "utf-8", line, "Markdown", Program::None, ""),
            Self::new("Commodore text (PETSCII)", "seq", "petscii-c64", Eol::Cr, "", Program::None, ""),
            Self::new("ZX Spectrum text", "txt", "zx-spectrum", Eol::Cr, "", Program::None, ""),
            Self::new("Amiga text", "txt", "amiga-text", Eol::Lf, "", Program::None, ""),
            Self::new("DOS text", "txt", "cp437", Eol::CrLf, "", Program::None, ""),
        ]
    }

    /// One line of the settings file. Fields are kept apart by \u{1}, and the starting text's lines by \u{3}.
    fn to_setting(&self) -> String {
        let fields = [self.name.as_str(), &self.ending, &self.encoding, self.eol.name(), &self.language, self.program.id(), &self.starter.replace('\n', "\u{3}")];
        format!("new_kind={}\n", fields.join("\u{1}"))
    }

    fn from_setting(value: &str) -> Option<Self> {
        let fields: Vec<&str> = value.split('\u{1}').collect();
        let [name, ending, encoding, eol, language, program, starter] = fields[..] else {
            return None;
        };
        let eol = EOLS.into_iter().find(|e| e.name() == eol).unwrap_or(Eol::CrLf);
        let program = Program::ALL.into_iter().find(|p| p.id() == program).unwrap_or_default();
        Some(Self::new(name, ending, encoding, eol, language, program, &starter.replace('\u{3}', "\n")))
    }
}

/// The list as the settings file keeps it: only when it's been changed, so the usual list can grow in later versions.
pub(super) fn kinds_to_settings(kinds: &[NewKind]) -> String {
    if kinds == NewKind::usual() {
        return String::new();
    }
    let mut text = "new_kinds=changed\n".to_string();
    text.extend(kinds.iter().map(NewKind::to_setting));
    text
}

pub(super) fn kinds_from_settings(settings: &str) -> Vec<NewKind> {
    if !list_setting(settings, "new_kinds").iter().any(|v| v == "changed") {
        return NewKind::usual();
    }
    list_setting(settings, "new_kind").iter().filter_map(|value| NewKind::from_setting(value)).collect()
}

/// The box changing the list.
pub(super) struct NewKindsBox {
    pub kinds: Vec<NewKind>,
    pub selected: usize,
}

impl Limoniter {
    /// A new file of a kind from the list.
    pub(super) fn new_of_kind(&mut self, index: usize) {
        let Some(kind) = self.new_kinds.get(index).cloned() else {
            return;
        };
        if self.no_file() {
            self.docs.clear();
        }
        self.untitled_count += 1;
        let stem = format!("Edit{}", self.untitled_count);
        let name = if kind.ending.is_empty() { stem.clone() } else { format!("{stem}.{}", kind.ending) };
        // A preset for the encoding brings how it's shown (PETSCII's colours, say) and its line rules.
        let preset = PRESETS.iter().find(|p| p.charset == kind.encoding && p.eol == kind.eol).or_else(|| PRESETS.iter().find(|p| p.charset == kind.encoding));
        let mut doc = Document::untitled(name, preset.unwrap_or(&PRESETS[0]));
        if preset.is_none() {
            doc.text.format.charset = charset::known(&kind.encoding);
            doc.text.format.preset = None;
            charset_widths(&doc.text.format);
        }
        doc.text.set_insert_eol(kind.eol);
        if !kind.starter.is_empty() {
            doc.text.set_content(kind.starter.lines().map(str::to_string).collect());
        }
        if !kind.language.is_empty() {
            highlight::load_now();
            if let Some((_, syntax)) = highlight::engine().and_then(|engine| engine.names().iter().find(|(name, _)| *name == kind.language)) {
                doc.set_language(Language::Named(*syntax));
            }
            doc.update_syntax();
        }
        doc.listing_of = kind.program.machine().map(|machine| (machine, stem));
        if matches!(kind.program, Program::Spectrum | Program::Next) {
            doc.text.zx = Some(Box::new(crate::zx_graphics::ZxText { notation: self.prefs.zx_notation, ..Default::default() }));
        }
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
    }

    pub(super) fn open_new_kinds_box(&mut self) {
        self.dialog = Dialog::NewKinds(Box::new(NewKindsBox { kinds: self.new_kinds.clone(), selected: 0 }));
    }

    pub(super) fn new_kinds_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::NewKinds(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        highlight::load_now();
        let languages: Vec<String> = highlight::engine().map(|engine| engine.names().iter().map(|(name, _)| name.clone()).collect()).unwrap_or_default();
        egui::Window::new("Kinds of new file").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("File, New as lists these. Each is a file ending, an encoding, line endings, colouring, and text to start with.");
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(220.0);
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (index, kind) in state.kinds.iter().enumerate() {
                            let name = if kind.name.is_empty() { "(no name)" } else { kind.name.as_str() };
                            if ui.selectable_label(state.selected == index, name).clicked() {
                                state.selected = index;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Add").clicked() {
                            let at = (state.selected + 1).min(state.kinds.len());
                            let mut kind = state.kinds.get(state.selected).cloned().unwrap_or_else(|| NewKind::new("", "txt", "utf-8", Eol::CrLf, "", Program::None, ""));
                            kind.name = "New kind".into();
                            state.kinds.insert(at, kind);
                            state.selected = at;
                        }
                        if ui.add_enabled(!state.kinds.is_empty(), egui::Button::new("Remove")).clicked() {
                            state.kinds.remove(state.selected);
                            state.selected = state.selected.min(state.kinds.len().saturating_sub(1));
                        }
                        if ui.add_enabled(state.selected > 0, egui::Button::new("Up")).clicked() {
                            state.kinds.swap(state.selected, state.selected - 1);
                            state.selected -= 1;
                        }
                        if ui.add_enabled(state.selected + 1 < state.kinds.len(), egui::Button::new("Down")).clicked() {
                            state.kinds.swap(state.selected, state.selected + 1);
                            state.selected += 1;
                        }
                        if ui.button("Put back the usual list").clicked() {
                            state.kinds = NewKind::usual();
                            state.selected = 0;
                        }
                    });
                });
                ui.add_space(16.0);
                let Some(kind) = state.kinds.get_mut(state.selected) else {
                    ui.label(RichText::new("The list is empty. Add a kind, or put back the usual list.").color(theme::text_dim()));
                    return;
                };
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    egui::Grid::new("new-kind").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut kind.name);
                        ui.end_row();
                        ui.label("File ending");
                        ui.add(egui::TextEdit::singleline(&mut kind.ending).hint_text("bas, asm, txt..."));
                        ui.end_row();
                        ui.label("Encoding");
                        let shown = charset::by_id(&kind.encoding).map_or_else(|| kind.encoding.clone(), |set| set.name.clone());
                        egui::ComboBox::from_id_salt("new-kind-encoding").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                            for set in charset::all() {
                                ui.selectable_value(&mut kind.encoding, set.id.clone(), &set.name);
                            }
                        });
                        ui.end_row();
                        ui.label("Line endings");
                        egui::ComboBox::from_id_salt("new-kind-eol").width(260.0).selected_text(kind.eol.long_name()).show_ui(ui, |ui| {
                            for eol in EOLS {
                                ui.selectable_value(&mut kind.eol, eol, eol.long_name());
                            }
                        });
                        ui.end_row();
                        ui.label("Colouring");
                        let shown = if kind.language.is_empty() { "From the file ending".to_string() } else { kind.language.clone() };
                        egui::ComboBox::from_id_salt("new-kind-language").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                            ui.selectable_value(&mut kind.language, String::new(), "From the file ending");
                            for name in &languages {
                                ui.selectable_value(&mut kind.language, name.clone(), name);
                            }
                        });
                        ui.end_row();
                        ui.label("A listing for");
                        egui::ComboBox::from_id_salt("new-kind-program").width(260.0).selected_text(kind.program.label()).show_ui(ui, |ui| {
                            for program in Program::ALL {
                                ui.selectable_value(&mut kind.program, program, program.label());
                            }
                        });
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    ui.label("Starts with");
                    ui.add(egui::TextEdit::multiline(&mut kind.starter).font(egui::TextStyle::Monospace).desired_rows(6).desired_width(f32::INFINITY));
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                keep = ui.button("OK").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if keep {
            if let Dialog::NewKinds(state) = std::mem::replace(&mut self.dialog, Dialog::None) {
                self.new_kinds = state.kinds;
            }
            self.pending_focus = true;
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }
}
