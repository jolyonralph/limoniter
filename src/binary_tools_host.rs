//! The Binary tools window, on the file in front: its checksums, a ROM header's checksum put right, IPS, BPS and
//! UPS patches made and applied, and its entropy and byte counts.

use super::*;
use crate::binary_tools::{self, PatchKind, Rom};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Checksums,
    Patches,
    Entropy,
}

#[derive(Default)]
pub(super) struct BinaryTools {
    pub tab: Tab,
    /// The file the figures are for (its document's id and how many edits it had), and the figures.
    of: Option<(u64, u64)>,
    name: String,
    length: usize,
    sums: Vec<(&'static str, String)>,
    rom: Option<(Rom, String, String)>,
    counts: Vec<u64>,
    whole: f64,
    along: (usize, Vec<f64>),
    said: String,
}

/// How many points the entropy graph has along the file.
const ENTROPY_POINTS: usize = 400;

impl Limoniter {
    pub(super) fn open_binary_tools(&mut self, tab: Tab) {
        let tools = self.binary_tools.get_or_insert_with(Box::default);
        tools.tab = tab;
        tools.of = None;
    }

    /// The figures, worked out again when the file in front isn't the one they're for.
    fn binary_figures(&self, tools: &mut BinaryTools) {
        let doc = &self.docs[self.active];
        let key = (doc.id, doc.text.version().wrapping_add(doc.hex.version()));
        if tools.of == Some(key) {
            return;
        }
        tools.of = Some(key);
        let bytes = self.document_bytes();
        tools.name = doc.name();
        tools.length = bytes.len();
        tools.sums = binary_tools::checksums(&bytes);
        tools.rom = binary_tools::rom_kind(&bytes).and_then(|rom| binary_tools::rom_checksum(&bytes, rom).map(|(has, right)| (rom, has, right)));
        tools.counts = binary_tools::histogram(&bytes).to_vec();
        tools.whole = binary_tools::entropy(&bytes);
        tools.along = binary_tools::entropy_along(&bytes, ENTROPY_POINTS);
    }

    pub(super) fn binary_tools_window(&mut self, ctx: &Context) {
        let Some(mut tools) = self.binary_tools.take() else { return };
        if self.no_file() {
            tools.of = None;
        } else {
            self.binary_figures(&mut tools);
        }
        let (mut close, mut fix, mut make, mut apply, mut copy) = (false, false, None, false, None);
        crate::fit::Dialog::new(crate::i18n::tr("Binary tools")).collapsible(false).resizable(true).default_width(560.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut tools.tab, Tab::Checksums, crate::i18n::tr("Checksums"));
                ui.selectable_value(&mut tools.tab, Tab::Patches, crate::i18n::tr("Patches"));
                ui.selectable_value(&mut tools.tab, Tab::Entropy, crate::i18n::tr("Entropy and byte counts"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(crate::i18n::tr("Close")).clicked();
                });
            });
            ui.separator();
            if tools.of.is_none() {
                ui.label(crate::i18n::tr("Open a file first: these work on the file in front."));
                return;
            }
            ui.label(RichText::new(crate::i18n::tf!("{}: {} bytes, as they'd be saved", tools.name, tools.length)).strong());
            match tools.tab {
                Tab::Checksums => {
                    egui::Grid::new("checksums").num_columns(3).striped(true).show(ui, |ui| {
                        for (name, value) in &tools.sums {
                            ui.label(crate::i18n::tr(name));
                            ui.label(RichText::new(value).monospace());
                            if ui.small_button(crate::i18n::tr("Copy")).clicked() {
                                copy = Some(value.clone());
                            }
                            ui.end_row();
                        }
                    });
                    if let Some((rom, has, right)) = &tools.rom {
                        ui.separator();
                        ui.label(crate::i18n::tf!("A {} ROM. Its header's checksum is {has}; the ROM's is {right}.", crate::i18n::tr(rom.name())));
                        if has == right {
                            ui.label(RichText::new(crate::i18n::tr("They match.")).color(theme::accent()));
                        } else {
                            fix = ui.button(crate::i18n::tr("Put the header's checksum right")).on_hover_text(crate::i18n::tr("In the file in front, as one change that Undo takes back")).clicked();
                        }
                    }
                }
                Tab::Patches => {
                    ui.label(crate::i18n::tr("Make a patch from the file in front (the original) to a changed copy of it:"));
                    ui.horizontal(|ui| {
                        for kind in [PatchKind::Ips, PatchKind::Bps, PatchKind::Ups] {
                            if ui.button(crate::i18n::tf!("{}\u{2026}", kind.ending().to_uppercase())).clicked() {
                                make = Some(kind);
                            }
                        }
                    });
                    ui.label(RichText::new(crate::i18n::tr("IPS is the oldest and reaches 16 MB; BPS and UPS check the file they're applied to and what they make.")).small().color(theme::text_dim()));
                    ui.add_space(6.0);
                    apply = ui.button(crate::i18n::tr("Apply a patch to the file in front\u{2026}")).on_hover_text(crate::i18n::tr("The patched file opens in a new tab, for you to check and save")).clicked();
                }
                Tab::Entropy => {
                    ui.label(crate::i18n::tf!("Entropy: {:.2} bits a byte (8 looks random: packed or encrypted; plain text is about 4 to 5)", tools.whole));
                    let (block, along) = &tools.along;
                    ui.label(RichText::new(crate::i18n::tf!("Along the file, each {block} bytes:")).small().color(theme::text_dim()));
                    entropy_graph(ui, along);
                    ui.add_space(4.0);
                    ui.label(RichText::new(crate::i18n::tr("How many of each byte, $00 to $FF:")).small().color(theme::text_dim()));
                    histogram_graph(ui, &tools.counts);
                }
            }
            if !tools.said.is_empty() {
                ui.separator();
                ui.label(&tools.said);
            }
        });
        if let Some(value) = copy {
            ctx.copy_text(value);
        }
        if fix && let Some((rom, ..)) = tools.rom {
            let mut bytes = self.document_bytes();
            if binary_tools::fix_rom_checksum(&mut bytes, rom) {
                self.replace_document_bytes(bytes);
                tools.of = None;
                tools.said = crate::i18n::tr("The header's checksum has been put right. Save the file to keep it.").to_string();
            }
        }
        if let Some(kind) = make {
            tools.said = self.make_patch_file(kind);
        }
        if apply {
            tools.said = self.apply_patch_file();
        }
        if !close {
            self.binary_tools = Some(tools);
        }
    }

    /// Puts new bytes of the same length in place of the file in front's: in hex, each run that differs typed
    /// over; as text, the lines decoded again, in one change.
    fn replace_document_bytes(&mut self, bytes: Vec<u8>) {
        let old = self.document_bytes();
        let doc = self.doc();
        if doc.hex_mode {
            let (caret, insert) = (doc.hex.caret, doc.hex.insert_mode);
            doc.hex.insert_mode = false;
            let mut at = 0;
            while at < bytes.len().min(old.len()) {
                if bytes[at] == old[at] {
                    at += 1;
                    continue;
                }
                let start = at;
                while at < bytes.len().min(old.len()) && bytes[at] != old[at] {
                    at += 1;
                }
                doc.hex.move_caret(start, false);
                doc.hex.insert_bytes(&bytes[start..at]);
            }
            doc.hex.insert_mode = insert;
            doc.hex.move_caret(caret, false);
        } else {
            let mut format = doc.text.format.clone();
            let decoded = crate::codec::decode(&bytes, &mut format);
            let last = doc.text.line_count().saturating_sub(1);
            doc.text.replace_lines(0, last, decoded.lines.iter().map(|l| l.to_string()).collect());
        }
    }

    /// Bytes in a new tab of their own, read as a file of that name would be.
    pub(super) fn open_bytes_as_new(&mut self, name: &str, bytes: Vec<u8>) {
        if self.no_file() {
            self.docs.clear();
        }
        let mut doc = Document::new(name.to_string());
        doc.load_bytes(bytes, Path::new(name), None, None);
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.pending_focus = true;
    }

    fn make_patch_file(&mut self, kind: PatchKind) -> String {
        let old = self.document_bytes();
        let folder = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let mut picker = rfd::FileDialog::new().set_title(crate::i18n::tr("The changed copy"));
        if let Some(folder) = &folder {
            picker = picker.set_directory(folder);
        }
        let Some(changed) = picker.pick_file() else { return String::new() };
        let new = match std::fs::read(&changed) {
            Ok(bytes) => bytes,
            Err(e) => return e.to_string(),
        };
        let patch = match binary_tools::make_patch(kind, &old, &new) {
            Ok(patch) => patch,
            Err(e) => return e,
        };
        let stem = changed.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "patch".into());
        let mut saver = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the patch")).set_file_name(format!("{stem}.{}", kind.ending()));
        if let Some(folder) = changed.parent() {
            saver = saver.set_directory(folder);
        }
        let Some(path) = saver.save_file() else { return String::new() };
        match std::fs::write(&path, &patch) {
            Ok(()) => crate::i18n::tf!("The patch is saved: {} bytes.", patch.len()),
            Err(e) => e.to_string(),
        }
    }

    fn apply_patch_file(&mut self) -> String {
        let old = self.document_bytes();
        let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("A patch to apply")).add_filter(crate::i18n::tr("Patches"), &["ips", "bps", "ups"]).pick_file() else { return String::new() };
        let patch = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => return e.to_string(),
        };
        let applied = match binary_tools::apply_patch(&old, &patch) {
            Ok(applied) => applied,
            Err(e) => return e,
        };
        let name = self.docs[self.active].path_or_name().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ending = self.docs[self.active].path.as_deref().and_then(Path::extension).map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        self.open_bytes_as_new(&format!("{name} (patched){ending}"), applied.bytes);
        if !applied.problems.is_empty() {
            applied.problems.join(" ")
        } else if applied.kind == PatchKind::Ips {
            crate::i18n::tr("Patched. IPS patches carry no check, so whether it was the right file isn't known.").to_string()
        } else {
            crate::i18n::tr("Patched, and checked: the file was the one the patch was made for, and the result is what it should be.").to_string()
        }
    }
}

/// The entropy along the file, 0 to 8 bits a byte, with the line at 7.5 above which it's likely packed.
fn entropy_graph(ui: &mut Ui, along: &[f64]) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(200.0), 90.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    if along.is_empty() {
        return;
    }
    let y = |e: f64| rect.max.y - (e / 8.0) as f32 * rect.height();
    painter.hline(rect.x_range(), y(7.5), egui::Stroke::new(1.0, theme::text_faint()));
    let step = rect.width() / along.len() as f32;
    for (index, value) in along.iter().enumerate() {
        let x = rect.min.x + index as f32 * step;
        let colour = if *value >= 7.5 { theme::rainbow_mistake() } else { theme::accent() };
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, y(*value)), egui::pos2(x + step.max(1.0), rect.max.y)), 0.0, colour);
    }
    if let Some(pointer) = response.hover_pos() {
        let index = (((pointer.x - rect.min.x) / step) as usize).min(along.len() - 1);
        response.on_hover_text(crate::i18n::tf!("Part {}: {:.2} bits a byte", index + 1, along[index]));
    }
}

/// How many of each byte, a bar each.
fn histogram_graph(ui: &mut Ui, counts: &[u64]) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(256.0), 90.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
    let most = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
    let step = rect.width() / 256.0;
    for (value, count) in counts.iter().enumerate() {
        let height = *count as f32 / most * rect.height();
        let x = rect.min.x + value as f32 * step;
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.max.y - height), egui::pos2(x + step.max(1.0), rect.max.y)), 0.0, theme::accent());
    }
    if let Some(pointer) = response.hover_pos() {
        let value = (((pointer.x - rect.min.x) / step) as usize).min(255);
        response.on_hover_text(crate::i18n::tf!("${value:02X} ({value}): {}", counts[value]));
    }
}

impl Limoniter {
    /// The regular expression tester, or the schema check, with the text to try it on taken from the selection
    /// or the start of the file in front.
    pub(super) fn open_regex_tester(&mut self, tab: crate::regex_tester::Tab) {
        let sample = if self.no_file() || self.docs[self.active].hex_mode {
            String::new()
        } else {
            let text = &self.docs[self.active].text;
            if text.has_selection() { text.selection_or_all().unwrap_or_default() } else { (0..text.line_count().min(100)).map(|i| text.line(i).to_string()).collect::<Vec<_>>().join("\n") }
        };
        let tester = self.regex_tester.get_or_insert_with(Box::default);
        tester.tab = tab;
        if tester.text.is_empty() {
            tester.text = sample;
        }
    }

    pub(super) fn regex_tester_window(&mut self, ctx: &Context) {
        let Some(mut tester) = self.regex_tester.take() else { return };
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr("Pattern and schema tester")).open(&mut open).collapsible(false).resizable(true).default_width(600.0).show(ctx, |ui| {
            asked = tester.draw(ui);
        });
        match asked {
            Some(crate::regex_tester::Asked::ChooseSchema) => {
                let folder = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
                let mut picker = rfd::FileDialog::new().set_title(crate::i18n::tr("A JSON Schema")).add_filter(crate::i18n::tr("JSON Schema"), &["json"]);
                if let Some(folder) = folder {
                    picker = picker.set_directory(folder);
                }
                if let Some(path) = picker.pick_file() {
                    tester.schema_path = Some(path);
                    tester.schema_result = None;
                }
            }
            Some(crate::regex_tester::Asked::Check) => {
                let schema = tester.schema_path.as_ref().map(std::fs::read_to_string);
                tester.schema_result = Some(match schema {
                    Some(Ok(schema)) => match serde_json::from_str::<serde_json::Value>(&schema) {
                        Ok(_) if self.no_file() || self.docs[self.active].hex_mode => Err(crate::i18n::tr("Put the JSON or YAML to check in front first.").to_string()),
                        Ok(schema) => {
                            let text = self.docs[self.active].text.selection_or_all().unwrap_or_default();
                            crate::regex_tester::read_data(&text).map(|data| crate::regex_tester::check(&schema, &data))
                        }
                        Err(e) => Err(crate::i18n::tf!("The schema isn't JSON that can be read: {e}")),
                    },
                    Some(Err(e)) => Err(e.to_string()),
                    None => Err(crate::i18n::tr("Choose a schema first.").to_string()),
                });
            }
            Some(crate::regex_tester::Asked::ToFind(pattern)) => {
                self.find.find = pattern;
                self.find.regex = true;
                self.find.open = true;
            }
            None => {}
        }
        if open {
            self.regex_tester = Some(tester);
        }
    }
}

impl Limoniter {
    /// The box choosing how AVR and PIC chips are flashed: the programmer, the chip and the port. The flashing
    /// tasks themselves are under Tools, Tasks, Run, once a .hex file has been built.
    pub(super) fn flashing_window(&mut self, ctx: &Context) {
        if !self.flashing_open {
            return;
        }
        let mut open = true;
        let before = self.flashing.clone();
        let f = &mut self.flashing;
        crate::fit::Dialog::new(crate::i18n::tr("Microcontroller flashing")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Once a .hex file has been built, Tools, Tasks, Run has a task that flashes it, made with these."));
            ui.add_space(4.0);
            ui.label(RichText::new(crate::i18n::tr("AVR, with avrdude")).strong());
            egui::Grid::new("avr-flash").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Programmer"));
                let shown = crate::tasks::AVR_PROGRAMMERS.iter().find(|(id, _)| *id == f.avr_programmer).map_or(f.avr_programmer.clone(), |(_, name)| crate::i18n::tr(name).to_string());
                egui::ComboBox::from_id_salt("avr-programmer").selected_text(shown).width(280.0).show_ui(ui, |ui| {
                    for (id, name) in crate::tasks::AVR_PROGRAMMERS {
                        ui.selectable_value(&mut f.avr_programmer, id.to_string(), crate::i18n::tr(name));
                    }
                });
                ui.end_row();
                ui.label(crate::i18n::tr("Chip"));
                ui.add(egui::TextEdit::singleline(&mut f.avr_part).hint_text("atmega328p").desired_width(160.0));
                ui.end_row();
                ui.label(crate::i18n::tr("Port"));
                ui.add(egui::TextEdit::singleline(&mut f.avr_port).hint_text(if cfg!(windows) { "COM3" } else { "/dev/ttyUSB0" }).desired_width(160.0)).on_hover_text(crate::i18n::tr("Left empty for a USB programmer that needs none"));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.label(RichText::new(crate::i18n::tr("PIC, with MPLAB IPE's ipecmd")).strong());
            egui::Grid::new("pic-flash").num_columns(2).show(ui, |ui| {
                ui.label(crate::i18n::tr("Programmer"));
                let shown = crate::tasks::PIC_TOOLS.iter().find(|(id, _)| *id == f.pic_tool).map_or(f.pic_tool.clone(), |(_, name)| name.to_string());
                egui::ComboBox::from_id_salt("pic-tool").selected_text(shown).width(280.0).show_ui(ui, |ui| {
                    for (id, name) in crate::tasks::PIC_TOOLS {
                        ui.selectable_value(&mut f.pic_tool, id.to_string(), name);
                    }
                });
                ui.end_row();
                ui.label(crate::i18n::tr("Chip"));
                ui.add(egui::TextEdit::singleline(&mut f.pic_part).hint_text("16F877A").desired_width(160.0));
                ui.end_row();
            });
        });
        self.flashing_open = open;
        if self.flashing != before {
            self.save_settings_if_changed();
        }
    }
}

/// The Code as a picture box: what's chosen, and the picture made.
pub(super) struct CodePicture {
    line_numbers: bool,
    margin: f32,
    scale: f32,
    /// The picture, and how it's shown here; made again when a choice changes.
    made: Option<(crate::pictures::Picture, egui::TextureHandle)>,
    made_with: Option<(bool, u32, u32)>,
    /// The lines, coloured, from the selection or the file when the box opened.
    lines: Vec<crate::code_picture::Line>,
    said: String,
}

/// The most lines made into one picture.
const MOST_PICTURE_LINES: usize = 600;

impl Limoniter {
    /// Code as a picture: the selection's lines, or the file's, coloured as they're shown now.
    pub(super) fn open_code_picture(&mut self) {
        if self.no_file() || self.docs[self.active].hex_mode {
            return self.message("Code as a picture", crate::i18n::tr("Put the code in front first, with the lines selected, or none for the whole file.").into());
        }
        let tab = crate::text_buffer::tab_size();
        let doc = &mut self.docs[self.active];
        let (top, bottom) = if doc.text.has_selection() { doc.text.selected_rows() } else { (0, doc.text.line_count().saturating_sub(1)) };
        let bottom = bottom.min(top + MOST_PICTURE_LINES - 1);
        // Coloured from the top, as the window colours them, however far down they are.
        let mut colours = crate::highlight::LineColours::default();
        let engine = crate::highlight::load_now();
        colours.set_syntax(doc.syntax_index());
        let began = std::time::Instant::now();
        while colours.is_active() && !colours.prepare(engine, &mut doc.text, top, bottom + 1, Duration::from_millis(500)) && began.elapsed() < Duration::from_secs(10) {}
        let text_colour = theme::editor_text();
        let comment_kinds = self.view.comment_kinds;
        let doc = &mut self.docs[self.active];
        let lines = (top..=bottom)
            .map(|r| {
                let raw = doc.text.format.readable_text(&doc.text.line(r));
                let coloured = colours.line(r).filter(|_| colours.is_active());
                let spans = coloured.map(|l| l.spans.clone()).unwrap_or_default();
                // TODO, NOTE and the like in their own colours, as they're shown.
                let kinds = if comment_kinds { coloured.map(|l| crate::view_extras::comment_kinds(&raw, &l.comments)).unwrap_or_default() } else { Vec::new() };
                // Each character into its run, tabs widened to their stops as they go.
                let mut runs: Vec<(String, egui::Color32)> = Vec::new();
                let mut spans = spans.into_iter().flat_map(|(length, colour)| std::iter::repeat_n(colour, length));
                let mut column = 0;
                for (index, c) in raw.chars().enumerate() {
                    let colour = spans.next().unwrap_or(text_colour);
                    let colour = kinds.iter().find(|(a, z, _)| index >= *a && index < *z).map_or(colour, |(_, _, tag)| theme::tag_colour(*tag));
                    let piece = if c == '\t' { " ".repeat(tab - column % tab) } else { c.to_string() };
                    column += piece.chars().count();
                    match runs.last_mut() {
                        Some((text, last)) if *last == colour => text.push_str(&piece),
                        _ => runs.push((piece, colour)),
                    }
                }
                crate::code_picture::Line { number: r + 1, runs }
            })
            .collect();
        self.code_picture = Some(Box::new(CodePicture { line_numbers: true, margin: 32.0, scale: 2.0, made: None, made_with: None, lines, said: String::new() }));
    }

    pub(super) fn code_picture_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.code_picture.take() else { return };
        let wanted = (state.line_numbers, state.margin as u32, (state.scale * 10.0) as u32);
        if state.made_with != Some(wanted) {
            state.made_with = Some(wanted);
            let size = self.text_view_options().font_size;
            let font_id = egui::FontId::monospace(size);
            let (char_width, row) = ctx.fonts_mut(|f| (f.glyph_width(&font_id, 'M'), f.row_height(&font_id)));
            let style = crate::code_picture::Style {
                background: theme::editor_back(),
                numbers_colour: theme::gutter_text(),
                font_size: size,
                char_width,
                line_height: (row * self.view.line_spacing).max(row),
                margin: state.margin,
                line_numbers: state.line_numbers,
                scale: state.scale,
            };
            let svg = crate::code_picture::svg(&state.lines, &style);
            match crate::code_picture::render(&svg, &editor_font_bytes(self.font_file.as_deref())) {
                Ok(picture) => {
                    let texture = ctx.load_texture("code-picture", egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba), egui::TextureOptions::LINEAR);
                    state.made = Some((picture, texture));
                }
                Err(e) => state.said = e,
            }
        }
        let mut open = true;
        let (mut save, mut copy) = (false, false);
        crate::fit::Dialog::new(crate::i18n::tr("Code as a picture")).open(&mut open).collapsible(false).resizable(true).default_width(640.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.line_numbers, crate::i18n::tr("Line numbers"));
                ui.label(crate::i18n::tr("Margin"));
                ui.add(egui::Slider::new(&mut state.margin, 0.0..=96.0).step_by(4.0));
                ui.label(crate::i18n::tr("Sharpness"));
                ui.selectable_value(&mut state.scale, 1.0, "1\u{D7}");
                ui.selectable_value(&mut state.scale, 2.0, "2\u{D7}");
                ui.selectable_value(&mut state.scale, 3.0, "3\u{D7}");
            });
            if let Some((picture, texture)) = &state.made {
                ui.label(RichText::new(crate::i18n::tf!("{} lines, {} \u{D7} {} pixels, in this theme's colours", state.lines.len(), picture.width, picture.height)).small().color(theme::text_dim()));
                egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                    let shown = egui::vec2(picture.width as f32, picture.height as f32) / state.scale;
                    ui.image((texture.id(), shown));
                });
            }
            ui.horizontal(|ui| {
                save = ui.button(crate::i18n::tr("Save as PNG\u{2026}")).clicked();
                copy = ui.button(crate::i18n::tr("Copy the picture")).clicked();
            });
            if !state.said.is_empty() {
                ui.label(&state.said);
            }
        });
        if let Some((picture, _)) = &state.made {
            if save {
                let name = self.docs[self.active].path_or_name().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "code".into());
                if let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the picture")).set_file_name(format!("{name}.png")).add_filter("PNG", &["png"]).save_file() {
                    let image = egui::ColorImage::from_rgba_unmultiplied([picture.width, picture.height], &picture.rgba);
                    state.said = match save_png(&path, &image) {
                        Ok(()) => crate::i18n::tr("Saved.").to_string(),
                        Err(e) => e,
                    };
                }
            }
            if copy {
                let image = arboard::ImageData { width: picture.width, height: picture.height, bytes: std::borrow::Cow::Borrowed(&picture.rgba) };
                state.said = match arboard::Clipboard::new().and_then(|mut c| c.set_image(image)) {
                    Ok(()) => crate::i18n::tr("The picture is on the clipboard.").to_string(),
                    Err(e) => e.to_string(),
                };
            }
        }
        if open {
            self.code_picture = Some(state);
        }
    }
}

/// The editor's own font, as the text is drawn in: the one chosen, else Consolas where Windows has it, else egui's
/// own monospace font.
fn editor_font_bytes(chosen: Option<&Path>) -> Vec<u8> {
    let chosen = chosen.map_or_else(|| PathBuf::from(r"C:\Windows\Fonts\consola.ttf"), Path::to_path_buf);
    std::fs::read(chosen).unwrap_or_else(|_| egui::FontDefinitions::default().font_data.get("Hack").map(|f| (*f.font).as_ref().to_vec()).unwrap_or_default())
}

#[cfg(test)]
pub(super) fn state_lines(state: &CodePicture) -> usize {
    state.lines.len()
}

#[cfg(test)]
pub(super) fn state_made(state: &CodePicture) -> bool {
    state.made.is_some()
}

impl Limoniter {
    /// A doc comment for the function at the cursor (on its line, or the one below or above), each parameter and
    /// the return listed, the cursor left on its summary.
    pub(super) fn write_doc_comment(&mut self) {
        let Some(style) = crate::doc_comments::style_for(&self.docs[self.active].language_name()) else { return };
        let text = &self.docs[self.active].text;
        let caret = text.caret_line;
        let count = text.line_count();
        // A signature may run over a few lines: read on until its brackets close.
        let read = |start: usize| -> Option<(crate::doc_comments::Signature, usize)> {
            let mut joined = String::new();
            for (offset, line) in (start..count.min(start + 8)).enumerate() {
                joined.push_str(&text.line(line));
                joined.push(' ');
                let opened = joined.matches('(').count();
                if opened > 0 && opened == joined.matches(')').count() {
                    return crate::doc_comments::signature(&joined, style).map(|s| (s, start + offset));
                }
            }
            None
        };
        let Some((start, (sig, end))) = [caret, caret + 1, caret.saturating_sub(1)].into_iter().filter(|l| *l < count).find_map(|l| read(l).map(|found| (l, found))) else {
            return self.message("Doc comment", crate::i18n::tr("Put the cursor on a function's first line, or just above it, to write its doc comment.").into());
        };
        let first = text.line(start).to_string();
        let indent: String = first.chars().take_while(|c| c.is_whitespace()).collect();
        let doc = self.doc();
        if style == crate::doc_comments::Style::Python {
            // Under the def, one level in.
            let inner = format!("{indent}{}", if indent.contains('\t') || (indent.is_empty() && !crate::text_buffer::edit_prefs().spaces_for_tab && first.starts_with('\t')) { "\t" } else { "    " });
            let (lines, summary) = crate::doc_comments::comment(&sig, style, &inner);
            let def_line = doc.text.line(end).to_string();
            let mut rows = vec![def_line];
            rows.extend(lines);
            doc.text.replace_lines(end, end, rows);
            let at = end + 1 + summary;
            let length = doc.text.line(at).chars().count();
            doc.text.move_caret_to_char(at, length);
        } else {
            let (mut lines, summary) = crate::doc_comments::comment(&sig, style, &indent);
            lines.push(first);
            doc.text.replace_lines(start, start, lines);
            let at = start + summary;
            let length = doc.text.line(at).chars().count();
            doc.text.move_caret_to_char(at, length);
        }
    }
}

impl Limoniter {
    /// A table of contents of the Markdown file's headings: put in at the cursor between markers, or, where
    /// there are markers already, made afresh between them. Saving keeps it up to date.
    pub(super) fn markdown_toc(&mut self) {
        let doc = self.doc();
        let lines = doc.text.all_lines();
        if let Some((start, end, rows)) = crate::markdown_tools::refresh_toc(&lines) {
            doc.text.replace_lines(start, end, rows);
            return;
        }
        let at = doc.text.caret_line;
        let mut rows = vec![crate::markdown_tools::TOC_START.to_string()];
        rows.extend(crate::markdown_tools::table_of_contents(&lines));
        rows.push(crate::markdown_tools::TOC_END.to_string());
        rows.push(lines[at].to_string());
        doc.text.replace_lines(at, at, rows);
    }

    /// The Markdown table the cursor's in, its columns lined up.
    pub(super) fn markdown_table(&mut self) {
        let doc = self.doc();
        let lines = doc.text.all_lines();
        let Some((top, bottom)) = crate::markdown_tools::table_around(&lines, doc.text.caret_line) else {
            return self.message("Markdown", crate::i18n::tr("Put the cursor in a table first: rows of cells between | marks, with a line of dashes under the first.").into());
        };
        match crate::markdown_tools::line_up_table(&lines[top..=bottom]) {
            Some(rows) => {
                self.doc().text.replace_lines(top, bottom, rows);
            }
            None => self.message("Markdown", crate::i18n::tr("That table has no line of dashes under its first row, so its columns can't be told apart.").into()),
        }
    }
}

impl Limoniter {
    /// Formats a document with the external formatter for its language. Returns whether there was one to use;
    /// what goes wrong is said only when asked, not on a save.
    pub(super) fn external_format(&mut self, index: usize, quiet: bool) -> bool {
        let doc = &self.docs[index];
        if doc.hex_mode || doc.text.is_disk_backed() {
            return false;
        }
        let Some(formatter) = crate::ext_formatters::chosen(&doc.language_name(), &self.formatter_choices) else { return false };
        let path = doc.path_or_name();
        let text = doc.text.all_lines().iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n");
        match crate::ext_formatters::run(formatter, &text, &path) {
            Ok(formatted) => {
                let rows: Vec<String> = formatted.strip_suffix('\n').unwrap_or(&formatted).split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
                let doc = &mut self.docs[index];
                if rows.iter().map(String::as_str).ne(doc.text.all_lines().iter().map(|l| &**l)) {
                    let (line, column) = doc.text.caret_char_position();
                    let last = doc.text.line_count().saturating_sub(1);
                    doc.text.replace_lines(0, last, rows);
                    let line = line.min(doc.text.line_count().saturating_sub(1));
                    doc.text.move_caret_to_char(line, column);
                }
            }
            Err(why) if !quiet => self.message("Format document", why),
            Err(_) => {}
        }
        true
    }

    /// The box choosing each language's formatter, among those installed.
    pub(super) fn formatters_window(&mut self, ctx: &Context) {
        if !self.formatters_open {
            return;
        }
        let mut open = true;
        let before = self.formatter_choices.clone();
        let mut languages: Vec<&str> = crate::ext_formatters::ALL.iter().flat_map(|f| f.languages.iter().copied()).collect();
        languages.sort_unstable();
        languages.dedup();
        let choices = &mut self.formatter_choices;
        crate::fit::Dialog::new(crate::i18n::tr("Formatters")).open(&mut open).collapsible(false).resizable(true).default_height(420.0).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Format document uses these where no language server formats the file. Only those installed can be chosen."));
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("formatters").num_columns(2).striped(true).show(ui, |ui| {
                    for language in languages {
                        ui.label(crate::i18n::tr(language));
                        let found = crate::ext_formatters::chosen(language, choices).map(|f| f.name);
                        let shown = if choices.get(language).is_some_and(|c| c == "none") { crate::i18n::tr("None").to_string() } else { found.map_or_else(|| crate::i18n::tr("None installed").to_string(), str::to_string) };
                        egui::ComboBox::from_id_salt(("formatter", language)).selected_text(shown).width(200.0).show_ui(ui, |ui| {
                            for formatter in crate::ext_formatters::for_language(language) {
                                let installed = crate::ext_formatters::installed(formatter);
                                if ui.add_enabled(installed, egui::Button::selectable(found == Some(formatter.name), formatter.name)).on_disabled_hover_text(crate::i18n::tf!("{} isn't installed", formatter.program)).clicked() {
                                    choices.insert(language.to_string(), formatter.name.to_string());
                                }
                            }
                            if ui.selectable_label(false, crate::i18n::tr("None")).clicked() {
                                choices.insert(language.to_string(), "none".to_string());
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        });
        self.formatters_open = open;
        if self.formatter_choices != before {
            self.save_settings_if_changed();
        }
    }
}

impl Limoniter {
    /// Serves the project's folder (or the file's own) for a browser, opening the page in front; or stops it.
    pub(super) fn toggle_live_server(&mut self) {
        if let Some(server) = self.live_server.take() {
            let folder = server.folder.display().to_string();
            drop(server);
            return self.message("Live server", crate::i18n::tf!("{folder} isn't being served any more."));
        }
        let front = (!self.no_file()).then(|| self.docs[self.active].path.clone()).flatten();
        let folder = self.project.as_ref().map(|p| p.folder()).filter(|f| front.as_deref().is_none_or(|file| file.starts_with(f))).or_else(|| front.as_deref().and_then(Path::parent).map(Path::to_path_buf));
        let Some(folder) = folder else {
            return self.message("Live server", crate::i18n::tr("Open a folder, or save the page, first: the folder it's in is what's served.").into());
        };
        match crate::live_server::Server::start(&folder) {
            Ok(server) => {
                let address = server.address_of(front.as_deref().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))));
                let _ = crate::platform::open_web_page(&address);
                self.live_server = Some(server);
                self.message("Live server", crate::i18n::tf!("{} is served at {address}, on this computer only. Its pages reload each time a file's saved. Choose Run, Serve this folder again to stop.", folder.display()));
            }
            Err(e) => self.message("Live server", e),
        }
    }
}

impl Limoniter {
    /// Time spent: the switch, and the totals for the days chosen by project, language and file, and by day.
    pub(super) fn time_spent_window(&mut self, ctx: &Context) {
        let Some(mut days) = self.time_spent_open else { return };
        let mut open = true;
        let mut tracking = self.view.track_time;
        let tracker = &self.time_spent;
        crate::fit::Dialog::new(crate::i18n::tr("Time spent")).open(&mut open).collapsible(false).resizable(true).default_width(520.0).show(ctx, |ui| {
            ui.checkbox(&mut tracking, crate::i18n::tr("Keep count of the time spent working")).on_hover_text(crate::i18n::tr("Kept in time-spent.tsv beside the settings, on this computer only, and sent nowhere. A pause of over two minutes isn't counted."));
            ui.horizontal(|ui| {
                ui.selectable_value(&mut days, 1, crate::i18n::tr("Today"));
                ui.selectable_value(&mut days, 7, crate::i18n::tr("This week"));
                ui.selectable_value(&mut days, 30, crate::i18n::tr("30 days"));
                ui.selectable_value(&mut days, 3650, crate::i18n::tr("All"));
            });
            let from = crate::time_spent::days_ago(days - 1);
            let total: u64 = tracker.summary(&from, |_| String::new()).iter().map(|(_, s)| s).sum();
            ui.label(RichText::new(crate::i18n::tf!("In all: {}", crate::time_spent::shown(total))).strong());
            if total == 0 {
                ui.label(RichText::new(crate::i18n::tr("Nothing counted for these days yet.")).color(theme::text_faint()));
                return;
            }
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (heading, rows) in [
                    ("By project", tracker.summary(&from, |k| k.project.clone())),
                    ("By language", tracker.summary(&from, |k| k.language.clone())),
                    ("By file", tracker.summary(&from, |k| format!("{} \u{203A} {}", k.project, k.file))),
                    ("By day", { let mut by_day = tracker.summary(&from, |k| k.day.clone()); by_day.sort_by(|a, b| b.0.cmp(&a.0)); by_day }),
                ] {
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::i18n::tr(heading)).strong());
                    let most = rows.iter().map(|r| r.1).max().unwrap_or(1).max(1);
                    egui::Grid::new(heading).num_columns(3).show(ui, |ui| {
                        for (name, seconds) in rows.iter().take(15) {
                            ui.label(name);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 10.0), egui::Sense::hover());
                            ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(140.0 * *seconds as f32 / most as f32, 10.0)), 2.0, theme::accent());
                            ui.label(crate::time_spent::shown(*seconds));
                            ui.end_row();
                        }
                    });
                }
            });
        });
        if tracking != self.view.track_time {
            self.view.track_time = tracking;
            self.save_settings_if_changed();
        }
        self.time_spent_open = open.then_some(days);
    }
}

impl Limoniter {
    /// How To Code: the guide's contents page, shown as it reads.
    pub(super) fn open_how_to_code(&mut self) {
        match crate::howtocode::unpacked() {
            Ok(folder) => {
                let path = folder.join("README.md");
                if self.open_path(&path) {
                    self.docs[self.active].preview_view = settings::PreviewView::Full;
                }
            }
            Err(problem) => self.message("How To Code", problem),
        }
    }

    /// How To Code's examples, and what they include, copied into a folder chosen, which then opens as a project.
    pub(super) fn copy_how_to_code_examples(&mut self) {
        let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("A folder for the examples")).pick_folder() else { return };
        match crate::howtocode::copy_examples(&folder) {
            Ok(copied) => {
                self.open_folder(&folder);
                let demo = folder.join("startup_demo.s");
                if demo.is_file() {
                    self.open_path(&demo);
                }
                self.message("How To Code", crate::i18n::tf!("{copied} files copied. startup_demo.s is the smallest whole demo: F9 builds it and runs it. Files that were there already were left as they were."));
            }
            Err(problem) => self.message("How To Code", problem),
        }
    }
}
