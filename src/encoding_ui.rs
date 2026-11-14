//! The windows and menus for encodings, line endings and display profiles.
//! Nothing here changes a document; each returns what was asked for and the window carries it out.

use crate::charset::{self, Charset};
use crate::codec::{self, Certainty, Display, Eol, EolCounts, Lossy, NewlinePolicy, PRESETS, Preset, Problems, TextFormat};
use crate::theme;
use eframe::egui::{self, Button, Context, RichText, Ui};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    /// Read the same bytes another way. Nothing is transcoded.
    Reopen,
    /// Keep the text and change how it will be stored.
    Convert,
    /// Write a copy another way, leaving this document as it is.
    SaveCopy,
}

impl Purpose {
    pub fn title(self) -> &'static str {
        match self {
            Purpose::Reopen => "Reopen using encoding",
            Purpose::Convert => "Convert to encoding",
            Purpose::SaveCopy => "Save a copy using encoding",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Purpose::Reopen => "Reads the file's bytes again in another encoding. The file isn't changed or transcoded.",
            Purpose::Convert => "Keeps the text and changes how it will be stored. Every character is checked first, and this can be undone.",
            Purpose::SaveCopy => "Writes a copy in another encoding. This document keeps its own settings.",
        }
    }
}

/// What a menu or window asks the window to do.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuAction {
    Pick(Purpose),
    ChoosePreset,
    SetBom(bool),
    Forget,
    SetInsertEol(Eol),
    ConvertEols(Eol),
    SetPolicy(NewlinePolicy),
    ToggleFinalNewline,
    SetDisplay(Display),
    SetStartState(usize),
    ToggleSourceView,
    AskInsertByte,
}

/// An item in the status bar that opens its own selector when clicked.
pub fn status_menu(ui: &mut Ui, label: String, tip: &str, add: impl FnOnce(&mut Ui) -> Option<MenuAction>) -> Option<MenuAction> {
    let shown = ui.add(egui::Label::new(label.clone()).sense(egui::Sense::click()));
    crate::access::button(&shown, &label);
    crate::access::describe(&shown, tip);
    let shown = shown.on_hover_text(tip);
    let mut action = None;
    egui::Popup::menu(&shown).show(|ui| action = add(ui));
    action
}

fn item(ui: &mut Ui, label: &str, tip: &str) -> bool {
    ui.add(Button::new(label).frame(false)).on_hover_text(tip).clicked()
}

fn radio_item(ui: &mut Ui, on: bool, label: &str) -> bool {
    ui.add(egui::Button::selectable(on, label)).clicked()
}

/// The menu behind the encoding in the status bar.
pub fn encoding_menu(ui: &mut Ui, format: &TextFormat, remembered: bool, can_edit: bool) -> Option<MenuAction> {
    let mut chosen = None;
    ui.set_min_width(250.0);
    ui.label(RichText::new(&format.charset.name).strong());
    ui.label(RichText::new(format.certainty.describe()).size(12.0).color(theme::text_dim()));
    ui.separator();
    if item(ui, "Reopen using encoding...", Purpose::Reopen.explanation()) {
        chosen = Some(MenuAction::Pick(Purpose::Reopen));
    }
    if ui.add_enabled(can_edit, Button::new("Convert to encoding...").frame(false)).on_hover_text(Purpose::Convert.explanation()).clicked() {
        chosen = Some(MenuAction::Pick(Purpose::Convert));
    }
    if item(ui, "Save a copy using encoding...", Purpose::SaveCopy.explanation()) {
        chosen = Some(MenuAction::Pick(Purpose::SaveCopy));
    }
    if item(ui, "System preset...", "Fills in the encoding, line endings and display together") {
        chosen = Some(MenuAction::ChoosePreset);
    }
    if format.charset.bom().is_some() {
        ui.separator();
        ui.label(RichText::new(if format.has_bom { "Has a byte order mark" } else { "No byte order mark" }).size(12.0).color(theme::text_dim()));
        let (label, wanted) = if format.has_bom { ("Remove the byte order mark", false) } else { ("Add a byte order mark", true) };
        if ui.add_enabled(can_edit, Button::new(label).frame(false)).on_hover_text("Takes effect when saved, and can be undone").clicked() {
            chosen = Some(MenuAction::SetBom(wanted));
        }
    }
    if remembered {
        ui.separator();
        if item(ui, "Forget the choice for this file", "Next time, the encoding is worked out from the file again") {
            chosen = Some(MenuAction::Forget);
        }
    }
    chosen
}

/// The menu behind the line endings in the status bar.
pub fn eol_menu(ui: &mut Ui, counts: &EolCounts, insert: Eol, format: &TextFormat, can_edit: bool, on_disk: bool) -> Option<MenuAction> {
    let mut chosen = None;
    ui.set_min_width(260.0);
    ui.label(RichText::new(if counts.is_mixed() { "Mixed line endings" } else { "Line endings" }).strong());
    ui.label(RichText::new(counts.describe()).size(12.0).color(theme::text_dim()));
    ui.menu_button(format!("Recognise: {}", format.newline.name()), |ui| {
        for policy in NewlinePolicy::CHOICES {
            if ui.add(egui::Button::selectable(format.newline == policy, policy.name())).on_hover_text("Rereads the file's bytes; the file isn't changed").clicked() {
                chosen = Some(MenuAction::SetPolicy(policy));
            }
        }
    });
    if format.newline.is_rows() {
        return chosen;
    }
    ui.separator();
    ui.label(RichText::new("New lines use").size(12.0).color(theme::text_dim()));
    // NL is only offered where it's a line ending, in EBCDIC.
    let eols: &[Eol] = if format.charset.ebcdic { &[Eol::Nl, Eol::CrLf, Eol::Lf, Eol::Cr] } else { &[Eol::CrLf, Eol::Lf, Eol::Cr, Eol::Atascii] };
    for &eol in eols {
        if radio_item(ui, insert == eol, eol.long_name()) {
            chosen = Some(MenuAction::SetInsertEol(eol));
        }
    }
    ui.separator();
    ui.add_enabled_ui(can_edit && !on_disk, |ui| {
        ui.menu_button("Convert all line endings to", |ui| {
            for &eol in eols {
                if ui.button(eol.long_name()).clicked() {
                    chosen = Some(MenuAction::ConvertEols(eol));
                }
            }
        });
    });
    if on_disk {
        ui.label(RichText::new("Not available in a file this large.").size(11.5).color(theme::text_dim()));
    }
    if ui.add(egui::Button::selectable(format.ensure_final_newline, "Ensure a final line ending when saving")).clicked() {
        chosen = Some(MenuAction::ToggleFinalNewline);
    }
    chosen
}

/// The menu behind the display profile in the status bar.
pub fn display_menu(ui: &mut Ui, format: &TextFormat, source_view: bool) -> Option<MenuAction> {
    let mut chosen = None;
    ui.set_min_width(220.0);
    ui.label(RichText::new("Display profile").strong());
    ui.label(RichText::new("How the text is read on screen. The bytes don't change.").size(12.0).color(theme::text_dim()));
    ui.separator();
    for display in Display::ALL {
        if radio_item(ui, format.display == display, display.name()) {
            chosen = Some(MenuAction::SetDisplay(display));
        }
    }
    if format.charset.tables.len() > 1 && format.display != Display::Plain {
        ui.separator();
        ui.label(RichText::new("Starting character set").size(12.0).color(theme::text_dim()));
        for (index, table) in format.charset.tables.iter().enumerate() {
            if radio_item(ui, format.state == index, &table.title) {
                chosen = Some(MenuAction::SetStartState(index));
            }
        }
    }
    ui.separator();
    if radio_item(ui, source_view, "Show controls as tokens") {
        chosen = Some(MenuAction::ToggleSourceView);
    }
    if item(ui, "Insert byte...", "Type a byte's value in hexadecimal") {
        chosen = Some(MenuAction::AskInsertByte);
    }
    chosen
}

pub struct Picker {
    pub purpose: Purpose,
    pub filter: String,
    pub charset: &'static Charset,
    pub bom: bool,
    pub policy: NewlinePolicy,
    pub display: Display,
    /// The start of the file, to show what this choice would make of it.
    pub sample: Vec<u8>,
}

impl Picker {
    pub fn new(purpose: Purpose, current: &TextFormat, sample: Vec<u8>) -> Self {
        Self { purpose, filter: String::new(), charset: current.charset, bom: current.has_bom, policy: current.newline, display: current.display, sample }
    }

    /// The settings chosen. Encoding alone leaves the line policy as it was.
    pub fn format(&self, current: &TextFormat) -> TextFormat {
        TextFormat {
            charset: self.charset,
            has_bom: self.bom && self.charset.bom().is_some(),
            newline: self.policy,
            display: self.display,
            state: 0,
            certainty: Certainty::Chosen,
            preset: None,
            ..current.clone()
        }
    }
}

pub enum Outcome {
    Open,
    Accept,
    Cancel,
}

fn preview(picker: &Picker, current: &TextFormat) -> Vec<String> {
    let mut format = picker.format(current);
    codec::decode(&picker.sample, &mut format).lines.iter().take(6).map(|line| format.readable_text(line).chars().take(70).collect()).collect()
}

pub fn picker_window(ctx: &Context, picker: &mut Picker, current: &TextFormat) -> Outcome {
    let mut outcome = Outcome::Open;
    egui::Window::new(picker.purpose.title()).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(640.0);
        ui.label(RichText::new(picker.purpose.explanation()).size(12.5).color(theme::text_dim()));
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(270.0);
                ui.add(egui::TextEdit::singleline(&mut picker.filter).hint_text("Search encodings").desired_width(262.0));
                let needle = picker.filter.to_lowercase();
                egui::ScrollArea::vertical().id_salt("picker-list").max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
                    for group in charset::groups() {
                        let sets: Vec<&'static Charset> = charset::all().iter().copied().filter(|set| set.group == group && (needle.is_empty() || set.name.to_lowercase().contains(&needle) || set.id.contains(&needle))).collect();
                        if sets.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(group).size(11.5).color(theme::text_dim()));
                        for set in sets {
                            if ui.add(egui::Button::selectable(picker.charset.id == set.id, &set.name).truncate()).clicked() {
                                picker.charset = set;
                            }
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.set_width(350.0);
                ui.label(RichText::new(&picker.charset.name).strong());
                ui.label(RichText::new(&picker.charset.scope).size(12.0));
                ui.label(RichText::new(format!("Mapping: {} ({})", picker.charset.source, picker.charset.version)).size(11.0).color(theme::text_faint()));
                ui.add_space(6.0);
                if picker.charset.bom().is_some() {
                    ui.add_enabled_ui(picker.purpose != Purpose::Reopen, |ui| {
                        ui.checkbox(&mut picker.bom, "Byte order mark");
                    });
                }
                if picker.purpose == Purpose::Reopen {
                    egui::ComboBox::from_label("Line endings").selected_text(picker.policy.name()).width(200.0).show_ui(ui, |ui| {
                        for policy in NewlinePolicy::CHOICES {
                            ui.selectable_value(&mut picker.policy, policy, policy.name());
                        }
                    });
                    egui::ComboBox::from_label("Display").selected_text(picker.display.name()).width(200.0).show_ui(ui, |ui| {
                        for display in Display::ALL {
                            ui.selectable_value(&mut picker.display, display, display.name());
                        }
                    });
                }
                if !picker.sample.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("The start of this file would read:").size(12.0).color(theme::text_dim()));
                    egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                        ui.set_width(340.0);
                        for line in preview(picker, current) {
                            ui.add(egui::Label::new(RichText::new(line).monospace().size(12.0)).truncate());
                        }
                    });
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                outcome = Outcome::Accept;
            }
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    outcome
}

pub struct PresetPicker {
    pub selected: usize,
    pub for_new_files: bool,
}

/// Fills in several settings at once; each can still be changed afterwards.
pub fn preset_window(ctx: &Context, picker: &mut PresetPicker) -> Outcome {
    let mut outcome = Outcome::Open;
    egui::Window::new("System preset").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new("A starting point for new files, or a way to reread this one. Not a promise about any existing file.").size(12.5).color(theme::text_dim()));
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                egui::ScrollArea::vertical().id_salt("preset-list").max_height(340.0).auto_shrink([false, false]).show(ui, |ui| {
                    for (index, preset) in PRESETS.iter().enumerate() {
                        if ui.add(egui::Button::selectable(picker.selected == index, preset.name)).clicked() {
                            picker.selected = index;
                        }
                    }
                });
            });
            ui.vertical(|ui| {
                ui.set_width(290.0);
                let preset: &'static Preset = &PRESETS[picker.selected];
                let set = charset::known(preset.charset);
                ui.label(RichText::new(preset.name).strong());
                egui::Grid::new("preset-grid").num_columns(2).show(ui, |ui| {
                    for (label, value) in [("Encoding", set.name.clone()), ("New lines", preset.eol.long_name().to_string()), ("Recognises", preset.newline.name()), ("Display", preset.display.name().to_string())] {
                        ui.label(RichText::new(label).size(12.0).color(theme::text_dim()));
                        ui.label(RichText::new(value).size(12.0));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new(preset.notes).size(12.0));
                ui.add_space(2.0);
                ui.label(RichText::new(&set.scope).size(11.5).color(theme::text_dim()));
                ui.add_space(6.0);
                ui.checkbox(&mut picker.for_new_files, "Use for new files, and leave this document alone");
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                outcome = Outcome::Accept;
            }
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        outcome = Outcome::Cancel;
    }
    outcome
}

/// What the person chose in the report of characters that won't fit. Cancelling is the default.
pub enum LossyChoice {
    Waiting,
    Cancel,
    GoTo(usize, usize),
    Export(Lossy),
    SaveCopyInstead,
}

pub struct LossyReport {
    pub purpose: LossyPurpose,
    pub target_name: String,
    pub problems: Problems,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LossyPurpose {
    Save,
    Convert,
    Export,
}

fn describe_character(c: char) -> String {
    match charset::raw_byte(c) {
        Some(byte) => format!("byte {byte:02X}, which has no character to carry over"),
        None => format!("\"{c}\" U+{:04X} {}", c as u32, charset::char_name(c)),
    }
}

pub fn lossy_window(ctx: &Context, report: &LossyReport) -> LossyChoice {
    let mut choice = LossyChoice::Waiting;
    let heading = match report.purpose {
        LossyPurpose::Save => format!("Can't save: {} character(s) won't fit in {}", report.problems.total, report.target_name),
        LossyPurpose::Convert => format!("Can't convert: {} character(s) won't fit in {}", report.problems.total, report.target_name),
        LossyPurpose::Export => format!("{} character(s) won't fit in {}", report.problems.total, report.target_name),
    };
    egui::Window::new("Characters that won't fit").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new(heading).strong());
        ui.label(RichText::new("Nothing has been changed.").size(12.5).color(theme::text_dim()));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt("lossy-list").max_height(280.0).auto_shrink([false, true]).show(ui, |ui| {
            for problem in &report.problems.listed {
                let label = format!("Line {}, column {}: {}", problem.line + 1, problem.column + 1, describe_character(problem.ch));
                if ui.add(Button::new(RichText::new(label).size(12.5)).frame(false)).on_hover_text("Go there").clicked() {
                    choice = LossyChoice::GoTo(problem.line, problem.column);
                }
            }
            if report.problems.total > report.problems.listed.len() {
                ui.label(RichText::new(format!("And {} more.", report.problems.total - report.problems.listed.len())).color(theme::text_dim()));
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                choice = LossyChoice::Cancel;
            }
            match report.purpose {
                LossyPurpose::Save => {
                    if ui.button("Save a copy another way...").on_hover_text("Choose an encoding that can hold everything").clicked() {
                        choice = LossyChoice::SaveCopyInstead;
                    }
                }
                LossyPurpose::Convert => {}
                LossyPurpose::Export => {
                    ui.separator();
                    if ui.button("Replace with ?").clicked() {
                        choice = LossyChoice::Export(Lossy::Replace);
                    }
                    if ui.button("Transliterate").on_hover_text("Nearest plain letters where there are any, otherwise ?").clicked() {
                        choice = LossyChoice::Export(Lossy::Transliterate);
                    }
                    if ui.button("Omit").clicked() {
                        choice = LossyChoice::Export(Lossy::Omit);
                    }
                }
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = LossyChoice::Cancel;
    }
    choice
}

/// Reads bytes typed as hexadecimal, such as `1B 5B` or `9b`.
pub fn parse_bytes(text: &str) -> Option<Vec<u8>> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
    let digits = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")).unwrap_or(&digits);
    if digits.is_empty() || !digits.len().is_multiple_of(2) {
        return None;
    }
    (0..digits.len()).step_by(2).map(|i| digits.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the status bar's encoding item with no window, feeding it exact mouse events.
    struct MenuBench {
        ctx: Context,
        time: f64,
        format: TextFormat,
        opened: bool,
        chosen: Option<MenuAction>,
        item: egui::Rect,
    }

    impl MenuBench {
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))), time: Some(self.time), predicted_dt: 1.0 / 60.0, events, ..Default::default() };
            let format = self.format.clone();
            let (mut opened, mut chosen, mut item) = (false, None, self.item);
            let mut output = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let start = ui.next_widget_position();
                    chosen = status_menu(ui, "UTF-8".to_string(), "tip", |ui| {
                        opened = true;
                        encoding_menu(ui, &format, false, true)
                    });
                    item = egui::Rect::from_min_size(start, egui::vec2(20.0, 12.0));
                });
            });
            output.textures_delta.clear();
            self.opened = opened;
            self.item = item;
            self.chosen = self.chosen.or(chosen);
        }

        fn click(&mut self, at: egui::Pos2) {
            let button = egui::PointerButton::Primary;
            self.frame(vec![egui::Event::PointerMoved(at)]);
            self.frame(vec![egui::Event::PointerButton { pos: at, button, pressed: true, modifiers: egui::Modifiers::NONE }]);
            self.frame(vec![egui::Event::PointerButton { pos: at, button, pressed: false, modifiers: egui::Modifiers::NONE }]);
            self.frame(Vec::new());
            self.frame(Vec::new());
        }
    }

    #[test]
    fn clicking_a_status_item_opens_its_selector_and_escape_closes_it() {
        let mut bench = MenuBench { ctx: Context::default(), time: 0.0, format: TextFormat::default(), opened: false, chosen: None, item: egui::Rect::NOTHING };
        bench.frame(Vec::new());
        bench.frame(Vec::new());
        assert!(!bench.opened, "closed until clicked");
        bench.click(bench.item.center());
        assert!(bench.opened, "the click opens it");
        let key = |pressed| egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        bench.frame(vec![key(true)]);
        bench.frame(vec![key(false)]);
        bench.frame(Vec::new());
        assert!(!bench.opened, "escape closes it");
        assert!(bench.chosen.is_none());
    }

    fn in_a_frame(ctx: &Context, escape: bool, mut body: impl FnMut(&Context)) {
        let events = if escape { vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }] } else { Vec::new() };
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), events, ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| body(ui.ctx()));
        output.textures_delta.clear();
    }

    #[test]
    fn the_windows_draw_and_cancelling_is_the_way_out() {
        let ctx = Context::default();
        let current = TextFormat::default();
        let mut picker = Picker::new(Purpose::Reopen, &current, b"caf\xE9\r\nline".to_vec());
        for (escape, open) in [(false, true), (true, false)] {
            in_a_frame(&ctx, escape, |ctx| {
                let outcome = picker_window(ctx, &mut picker, &current);
                assert_eq!(matches!(outcome, Outcome::Open), open);
                if !open {
                    assert!(matches!(outcome, Outcome::Cancel));
                }
            });
        }
        let mut presets = PresetPicker { selected: 9, for_new_files: false };
        in_a_frame(&ctx, false, |ctx| assert!(matches!(preset_window(ctx, &mut presets), Outcome::Open)));
        in_a_frame(&ctx, true, |ctx| assert!(matches!(preset_window(ctx, &mut presets), Outcome::Cancel)));
        for purpose in [LossyPurpose::Save, LossyPurpose::Convert, LossyPurpose::Export] {
            let mut problems = Problems::default();
            problems.add(codec::Problem { line: 3, column: 1, ch: '\u{20AC}' });
            let report = LossyReport { purpose, target_name: "CP437".into(), problems };
            in_a_frame(&ctx, false, |ctx| assert!(matches!(lossy_window(ctx, &report), LossyChoice::Waiting), "nothing is chosen for the person"));
            in_a_frame(&ctx, true, |ctx| assert!(matches!(lossy_window(ctx, &report), LossyChoice::Cancel)));
        }
    }

    #[test]
    fn typed_bytes_are_read_or_refused() {
        assert_eq!(parse_bytes("9B"), Some(vec![0x9B]));
        assert_eq!(parse_bytes("1b 5b"), Some(vec![0x1B, 0x5B]));
        assert_eq!(parse_bytes("0xFF"), Some(vec![0xFF]));
        assert_eq!(parse_bytes("F"), None);
        assert_eq!(parse_bytes("zz"), None);
        assert_eq!(parse_bytes(""), None);
    }

    #[test]
    fn choosing_an_encoding_alone_leaves_the_line_policy() {
        let current = TextFormat { newline: NewlinePolicy::CrOnly, ..TextFormat::default() };
        let mut picker = Picker::new(Purpose::Reopen, &current, Vec::new());
        picker.charset = charset::known("cp437");
        let format = picker.format(&current);
        assert_eq!(format.newline, NewlinePolicy::CrOnly);
        assert_eq!(format.charset.id, "cp437");
        assert!(!format.has_bom);
    }

    #[test]
    fn problems_describe_their_character() {
        assert!(describe_character('\u{20AC}').contains("EURO SIGN"));
        assert!(describe_character(charset::raw_char(0x81)).contains("81"));
    }
}
