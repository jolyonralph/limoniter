//! The Amiga hardware window: a guide to the custom chips' and CIAs' registers, searchable and grouped by what
//! they're for, with any value read into its bits; and the copper list under the cursor, decoded line by line with
//! its colours shown and a picture of what it does to a PAL frame, and MOVE, WAIT and END lines put in by register
//! name; and a gradient editor, whose colour stops down the screen become a copper list put into the source.

use super::*;
use crate::amiga_hw::{self, Area, Chipset, Copper, Register};
use egui::{Color32, RichText, Sense, vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Registers,
    Copper,
    Gradient,
}

/// What the window remembers while it's open.
pub(super) struct HwWindow {
    pub tab: Tab,
    search: String,
    /// Only the registers a chipset has.
    chipset: Option<Chipset>,
    /// The register being looked at, by address.
    selected: Option<u32>,
    /// A value to read into the selected register's bits.
    value: String,
    /// Shows the list the cursor is in; otherwise the one chosen stays.
    follow: bool,
    /// The copper list chosen, by its first line.
    list: Option<usize>,
    move_register: String,
    move_value: String,
    wait_line: String,
    wait_x: String,
    /// The picture of the copper list shown, and the list's words it was made from.
    preview: Option<(Vec<(u16, Option<u16>)>, egui::TextureHandle, crate::amiga_copper::Preview)>,
    gradient: crate::amiga_copper::Gradient,
}

impl Default for HwWindow {
    fn default() -> Self {
        HwWindow {
            tab: Tab::Registers,
            search: String::new(),
            chipset: None,
            selected: amiga_hw::named("BPLCON0").map(|r| r.address),
            value: String::new(),
            follow: true,
            list: None,
            move_register: "COLOR00".into(),
            move_value: "$0000".into(),
            wait_line: "$2C".into(),
            wait_x: "$07".into(),
            preview: None,
            gradient: Default::default(),
        }
    }
}

/// A number typed in: $hex, %binary, 0x hex or decimal.
fn number(text: &str) -> Option<i64> {
    crate::amiga_source::evaluate(text.trim(), &|_| None)
}

/// Whether a register goes with what's been searched for: its name, its address or offset, or words of what it does.
fn matches(register: &Register, search: &str) -> bool {
    let search = search.trim().to_ascii_lowercase();
    if search.is_empty() {
        return true;
    }
    if let Some(n) = number(&search).or_else(|| i64::from_str_radix(search.trim_start_matches('$'), 16).ok())
        && (register.address as i64 == n || (!register.is_cia() && register.offset() as i64 == n) || register.address as i64 & 0xFFF == n)
    {
        return true;
    }
    register.name.to_ascii_lowercase().contains(&search)
        || register.about.to_ascii_lowercase().contains(&search)
        || register.fields.iter().any(|f| f.name.to_ascii_lowercase().contains(&search) || f.about.to_ascii_lowercase().contains(&search))
}

impl HwWindow {
    fn registers_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(crate::i18n::tr("Search: name, address or words")).desired_width(240.0));
            egui::ComboBox::from_id_salt("amiga-hw-chipset").selected_text(self.chipset.map_or("Every chipset", |c| c.name())).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.chipset, None, crate::i18n::tr("Every chipset"));
                for chipset in Chipset::ALL {
                    ui.selectable_value(&mut self.chipset, Some(chipset), crate::i18n::tf!("What {} has", chipset.name()));
                }
            });
        });
        ui.separator();
        let searching = !self.search.trim().is_empty();
        ui.horizontal_top(|ui| {
            // In a row, a list laid out by itself would be laid out across; it goes down.
            ui.vertical(|ui| {
                egui::ScrollArea::vertical().id_salt("amiga-hw-list").max_height(440.0).min_scrolled_height(440.0).show(ui, |ui| {
                    ui.set_width(250.0);
                    for area in Area::ALL {
                        let shown: Vec<&Register> = amiga_hw::registers().iter().filter(|r| r.area == area && self.chipset.is_none_or(|c| r.since <= c) && matches(r, &self.search)).collect();
                        if shown.is_empty() {
                            continue;
                        }
                        let header = egui::CollapsingHeader::new(format!("{} ({})", area.name(), shown.len())).id_salt(("amiga-hw-area", area as u8));
                        let header = if searching { header.open(Some(true)) } else { header.default_open(false) };
                        header.show(ui, |ui| {
                            for register in shown {
                                let label = if register.is_cia() { format!("${:06X}  {}", register.address, register.name) } else { format!("${:03X}  {}", register.offset(), register.name) };
                                let chosen = self.selected == Some(register.address);
                                if ui.selectable_label(chosen, RichText::new(label).monospace()).on_hover_text(&register.about).clicked() {
                                    self.selected = Some(register.address);
                                }
                            }
                        });
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(470.0);
                let Some(register) = self.selected.and_then(amiga_hw::at) else {
                    ui.label(crate::i18n::tr("Choose a register."));
                    return;
                };
                ui.label(RichText::new(&register.name).strong().size(16.0));
                let place = if register.is_cia() { format!("${:06X}", register.address) } else { crate::i18n::tf!("${:06X}, ${:03X} from the custom chips' base", register.address, register.offset()) };
                ui.label(RichText::new(place).monospace());
                if register.is_cia() {
                    ui.label(crate::i18n::tf!("{}, a byte", register.access.name()));
                } else {
                    ui.label(crate::i18n::tf!("{}; {}", register.access.name(), register.since.onwards()));
                }
                ui.add_space(4.0);
                ui.label(&register.about);
                ui.add_space(6.0);
                if !register.fields.is_empty() {
                    egui::Grid::new("amiga-hw-bits").striped(true).num_columns(3).show(ui, |ui| {
                        for f in register.fields {
                            ui.label(RichText::new(f.bits()).monospace());
                            ui.label(RichText::new(f.name).monospace().strong());
                            let since = if f.since > register.since { format!(" ({})", f.since.name()) } else { String::new() };
                            ui.add(egui::Label::new(crate::i18n::tf!("{}{since}", f.about)).wrap());
                            ui.end_row();
                        }
                    });
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Value:"));
                    ui.add(egui::TextEdit::singleline(&mut self.value).hint_text(crate::i18n::tr("$5200")).desired_width(90.0).font(egui::TextStyle::Monospace));
                    if ui.button(crate::i18n::tr("Copy name")).clicked() {
                        ui.ctx().copy_text(register.name.replace("CIAA ", "").replace("CIAB ", ""));
                    }
                });
                if let Some(value) = number(&self.value) {
                    let value = value as u16;
                    ui.label(RichText::new(amiga_hw::explain(register, value)).strong());
                    if let Some(n) = register.colour() {
                        let (r, g, b) = amiga_hw::rgb12(value);
                        swatch(ui, Color32::from_rgb(r * 17, g * 17, b * 17), &crate::i18n::tf!("colour {n}"));
                    }
                    for line in amiga_hw::field_lines(register, value) {
                        ui.label(RichText::new(line).monospace().small());
                    }
                } else if !self.value.trim().is_empty() {
                    ui.label(crate::i18n::tr("That isn't a number: write $5200, %0101001000000000 or 20992."));
                }
            });
        });
    }
}

fn swatch(ui: &mut egui::Ui, colour: Color32, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 14.0), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, colour);
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    response.on_hover_text(tip);
}

/// What the copper tab asks of the window.
enum Asked {
    GoTo(usize),
    Insert(String),
}

impl Limoniter {
    pub(super) fn open_amiga_hardware(&mut self, tab: Tab) {
        let window = self.amiga_hw.get_or_insert_with(Box::default);
        window.tab = tab;
    }

    /// Puts lines of source in after the cursor's line (or on it, if it's empty), indented as it is.
    fn put_in_line(&mut self, text: &str) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message("Amiga hardware", crate::i18n::tr("Open the source to put the line in, with the cursor on the line it goes after.").into());
            return;
        }
        let doc = &mut self.docs[self.active];
        let line = doc.text.caret_line;
        let here = doc.text.line(line);
        let indent: String = here.chars().take_while(|c| c.is_whitespace()).collect();
        // A label alone on its line has nothing indented to follow: the line goes in with a tab.
        let indent = if indent.is_empty() { "\t".to_string() } else { indent };
        let text = text.replace('\n', &format!("\n{indent}"));
        let text = text.as_str();
        doc.text.begin_undo_group(false);
        doc.text.move_caret_to_char(line, here.chars().count());
        if here.is_empty() {
            doc.text.type_text(&format!("{indent}{text}"));
        } else if here.trim().is_empty() {
            doc.text.type_text(text);
        } else {
            doc.text.type_text(&format!("\n{indent}{text}"));
        }
        doc.text.end_undo_group();
    }

    pub(super) fn amiga_hw_window(&mut self, ctx: &Context) {
        let Some(mut window) = self.amiga_hw.take() else {
            return;
        };
        let source = if self.no_file() { None } else { self.docs[self.active].text.intel.as_ref().and_then(|i| i.hardware.clone()) };
        let caret = if self.no_file() { 0 } else { self.docs[self.active].text.caret_line };
        let mut open = true;
        let mut asked = None;
        egui::Window::new(crate::i18n::tr("Amiga hardware")).open(&mut open).default_pos([140.0, 90.0]).default_width(760.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut window.tab, Tab::Registers, crate::i18n::tr("Registers"));
                ui.selectable_value(&mut window.tab, Tab::Copper, crate::i18n::tr("Copper list"));
                ui.selectable_value(&mut window.tab, Tab::Gradient, crate::i18n::tr("Gradient"));
            });
            ui.separator();
            match window.tab {
                Tab::Registers => window.registers_tab(ui),
                Tab::Copper => asked = copper_tab(&mut window, ui, source.as_deref(), caret),
                Tab::Gradient => asked = gradient_tab(&mut window.gradient, ui),
            }
        });
        if !open {
            return;
        }
        match asked {
            Some(Asked::GoTo(line)) => {
                let doc = &mut self.docs[self.active];
                doc.text.move_caret_to_char(line, 0);
            }
            Some(Asked::Insert(text)) => self.put_in_line(&text),
            None => {}
        }
        self.amiga_hw = Some(window);
    }
}

fn copper_tab(window: &mut HwWindow, ui: &mut egui::Ui, source: Option<&crate::amiga_source::Source>, caret: usize) -> Option<Asked> {
    let mut asked = None;
    let lists = source.map(|s| s.lists.as_slice()).unwrap_or_default();
    if lists.is_empty() {
        ui.label(crate::i18n::tr("No copper list here. Open 68000 source with one: dc.w pairs such as\n    dc.w $0180,$0FFF   ; MOVE to COLOR00\n    dc.w $2C07,$FFFE   ; WAIT for line $2C\n    dc.w $FFFF,$FFFE   ; END\nor CMOVE, CWAIT and CEND lines. It's read a moment after typing stops."));
    } else {
        if window.follow
            && let Some(list) = lists.iter().find(|l| l.first_line <= caret && caret <= l.last_line)
        {
            window.list = Some(list.first_line);
        }
        let chosen = lists.iter().find(|l| Some(l.first_line) == window.list).unwrap_or(&lists[0]);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("amiga-hw-lists").selected_text(chosen.name()).show_ui(ui, |ui| {
                for list in lists {
                    if ui.selectable_label(list.first_line == chosen.first_line, list.name()).clicked() {
                        window.list = Some(list.first_line);
                        window.follow = false;
                    }
                }
            });
            ui.checkbox(&mut window.follow, crate::i18n::tr("Follow the cursor"));
        });
        ui.separator();
        let entries: Vec<(u16, Option<u16>)> = chosen.entries.iter().map(|e| (e.first, e.second)).collect();
        if window.preview.as_ref().is_none_or(|(made_from, _, _)| *made_from != entries) {
            let ops: Vec<Option<Copper>> = chosen.entries.iter().map(|e| e.second.map(|_| e.op())).collect();
            let preview = crate::amiga_copper::simulate(&ops);
            let texture = ui.ctx().load_texture("amiga-copper-preview", preview_image(&preview), egui::TextureOptions::NEAREST);
            window.preview = Some((entries, texture, preview));
        }
        ui.horizontal_top(|ui| {
        if let Some((_, texture, preview)) = &window.preview {
            ui.vertical(|ui| {
                ui.set_width(texture.size()[0] as f32 * 1.2);
                let size = vec2(texture.size()[0] as f32 * 1.2, texture.size()[1] as f32);
                let response = ui.add(egui::Image::new((texture.id(), size)).sense(Sense::hover()));
                // The usual display window: lines $2C to $12C, colour clocks $40 to $E0.
                let rect = response.rect;
                let sx = size.x / texture.size()[0] as f32;
                let window_rect = egui::Rect::from_min_max(rect.min + vec2(0x40 as f32 * sx, 0x2C as f32), rect.min + vec2(0xE0 as f32 * sx, 0x12C as f32));
                ui.painter().rect_stroke(window_rect, 0.0, egui::Stroke::new(1.0, Color32::from_white_alpha(70)), egui::StrokeKind::Outside);
                if let Some(p) = response.hover_pos() {
                    let line = ((p.y - rect.min.y) as usize).min(crate::amiga_copper::LINES - 1);
                    let clock = ((p.x - rect.min.x) / sx) as usize;
                    if clock < crate::amiga_copper::CLOCKS {
                        let [r, g, b] = preview.at(line, clock);
                        response.on_hover_text(crate::i18n::tf!("Line ${line:03X} ({line}), x ${clock:02X}: COLOR00 #{r:02X}{g:02X}{b:02X}"));
                    } else if clock >= crate::amiga_copper::CLOCKS + 2
                        && let Some(n) = preview.touched.get((clock - crate::amiga_copper::CLOCKS - 2) / 6)
                    {
                        let [r, g, b] = preview.palettes.get(line).and_then(|p| p.get((clock - crate::amiga_copper::CLOCKS - 2) / 6)).copied().unwrap_or_default();
                        response.on_hover_text(crate::i18n::tf!("Line ${line:03X} ({line}): colour {n} is #{r:02X}{g:02X}{b:02X}"));
                    }
                }
                let note = match preview.stopped {
                    Some((line, _)) => crate::i18n::tf!("The copper stops on line ${line:03X}."),
                    None => crate::i18n::tr("The list doesn't end, so the copper would run on into what follows it.").into(),
                };
                ui.label(RichText::new(crate::i18n::tf!("A PAL frame with no bitplanes: COLOR00 where the beam was, and beside it each other colour set, line by line. {note}")).small());
            });
        }
        egui::ScrollArea::vertical().id_salt("amiga-hw-copper").max_height(380.0).show(ui, |ui| {
            egui::Grid::new("amiga-hw-copper-rows").striped(true).num_columns(4).show(ui, |ui| {
                for row in chosen.rows() {
                    let here = row.line == caret;
                    let number = RichText::new(crate::i18n::tf!("{:>5}", row.line + 1)).monospace();
                    if ui.selectable_label(here, number).on_hover_text(crate::i18n::tr("Go to this line")).clicked() {
                        asked = Some(Asked::GoTo(row.line));
                    }
                    ui.label(RichText::new(&row.words).monospace());
                    match row.colour {
                        Some((n, [r, g, b])) => swatch(ui, Color32::from_rgb(r, g, b), &crate::i18n::tf!("colour {n}")),
                        None => {
                            ui.label(crate::i18n::tr(""));
                        }
                    }
                    ui.add(egui::Label::new(&row.said).wrap());
                    ui.end_row();
                }
            });
        });
        });
    }
    ui.separator();
    ui.label(RichText::new(crate::i18n::tr("Put a line in after the cursor's")).strong());
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("MOVE"));
        ui.add(egui::TextEdit::singleline(&mut window.move_value).desired_width(70.0).font(egui::TextStyle::Monospace));
        ui.label(crate::i18n::tr("to"));
        ui.add(egui::TextEdit::singleline(&mut window.move_register).hint_text(crate::i18n::tr("COLOR00")).desired_width(110.0).font(egui::TextStyle::Monospace));
        let register = amiga_hw::named(&window.move_register).or_else(|| number(&window.move_register).and_then(|n| amiga_hw::custom(n as u32 & 0x1FF))).filter(|r| !r.is_cia());
        let value = number(&window.move_value);
        match (register, value) {
            (Some(register), Some(value)) => {
                let tip = format!("{}: {}", register.name, amiga_hw::explain(register, value as u16));
                if ui.button(crate::i18n::tr("Put in")).on_hover_text(crate::i18n::tr(&tip)).clicked() {
                    asked = Some(Asked::Insert(amiga_hw::copper_line(Copper::Move { register: register.offset() as u16, value: value as u16 })));
                }
            }
            (None, _) => {
                ui.label(crate::i18n::tr("(no such register)"));
            }
            (_, None) => {
                ui.label(crate::i18n::tr("(the value isn't a number)"));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("WAIT for line"));
        ui.add(egui::TextEdit::singleline(&mut window.wait_line).desired_width(50.0).font(egui::TextStyle::Monospace));
        ui.label(crate::i18n::tr("x"));
        ui.add(egui::TextEdit::singleline(&mut window.wait_x).desired_width(50.0).font(egui::TextStyle::Monospace));
        if let (Some(line), Some(x)) = (number(&window.wait_line), number(&window.wait_x)) {
            let wait = Copper::Wait { line: line as u8, x: (x as u8) & 0xFE, line_mask: 0xFF, x_mask: 0xFE, blitter: false, skip: false };
            if ui.button(crate::i18n::tr("Put in")).on_hover_text(crate::i18n::tr("Lines past 255 need a WAIT for $FF,$DE first, then the line less 256")).clicked() {
                asked = Some(Asked::Insert(amiga_hw::copper_line(wait)));
            }
        }
        ui.separator();
        if ui.button(crate::i18n::tr("END")).on_hover_text(crate::i18n::tr("dc.w $FFFF,$FFFE")).clicked() {
            asked = Some(Asked::Insert(amiga_hw::copper_line(Copper::End)));
        }
    });
    asked
}

/// A copper preview as a picture: the frame a colour clock to a pixel, then a narrow column for each other colour
/// the list set (up to 24 of them).
fn preview_image(preview: &crate::amiga_copper::Preview) -> egui::ColorImage {
    use crate::amiga_copper::{CLOCKS, LINES};
    let extra = preview.touched.len().min(24) * 6 + if preview.touched.is_empty() { 0 } else { 2 };
    let width = CLOCKS + extra;
    let mut rgb = Vec::with_capacity(width * LINES * 3);
    for line in 0..LINES {
        for clock in 0..CLOCKS {
            rgb.extend_from_slice(&preview.at(line, clock));
        }
        for x in 0..extra {
            let colour = if x < 2 || (x - 2) % 6 == 5 { [24, 24, 24] } else { preview.palettes.get(line).and_then(|p| p.get((x - 2) / 6)).copied().unwrap_or_default() };
            rgb.extend_from_slice(&colour);
        }
    }
    egui::ColorImage::from_rgb([width, LINES], &rgb)
}

/// The gradient tab: its stops, their colours and lines, a bar showing what the copper will make of them, and the
/// source it becomes.
fn gradient_tab(gradient: &mut crate::amiga_copper::Gradient, ui: &mut egui::Ui) -> Option<Asked> {
    use crate::amiga_copper::{LINES, Stop, colour_at, simulate};
    let mut asked = None;
    let ops = gradient.ops();
    let preview = simulate(&ops.iter().map(|o| Some(*o)).collect::<Vec<_>>());
    let register = 0x180 + gradient.register * 2;
    let place = preview.touched.iter().position(|n| *n == gradient.register);
    let colours: Vec<[u8; 3]> = (0..LINES)
        .map(|line| if gradient.register == 0 { preview.at(line, 120) } else { place.and_then(|i| preview.palettes.get(line).and_then(|p| p.get(i))).copied().unwrap_or_default() })
        .collect();
    ui.horizontal_top(|ui| {
        // The bar: a pixel a line, the stops marked; a click adds a stop there.
        let (rect, response) = ui.allocate_exact_size(vec2(60.0, LINES as f32), Sense::click());
        for (line, [r, g, b]) in colours.iter().enumerate() {
            let y = rect.min.y + line as f32;
            ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y + 1.0)), 0.0, Color32::from_rgb(*r, *g, *b));
        }
        for stop in &gradient.stops {
            let y = rect.min.y + stop.line as f32 + 0.5;
            ui.painter().line_segment([egui::pos2(rect.max.x, y), egui::pos2(rect.max.x + 8.0, y)], egui::Stroke::new(2.0, ui.visuals().strong_text_color()));
        }
        if let Some(p) = response.hover_pos() {
            let line = ((p.y - rect.min.y) as usize).min(LINES - 1);
            let [r, g, b] = colours[line];
            if response.clicked() {
                let rgb = colour_at(&gradient.stops, line as u16);
                gradient.stops.push(Stop { line: line as u16, rgb });
                gradient.stops.sort_by_key(|s| s.line);
            }
            response.on_hover_text(crate::i18n::tf!("Line ${line:03X} ({line}): #{r:02X}{g:02X}{b:02X}. Click to add a stop here"));
        }
        ui.add_space(14.0);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Colour register COLOR"));
                ui.add(egui::DragValue::new(&mut gradient.register).range(0..=31).custom_formatter(|n, _| format!("{:02}", n as u16)));
                ui.checkbox(&mut gradient.aga, crate::i18n::tr("AGA 24-bit colour")).on_hover_text(crate::i18n::tr("Each line sets the high bits, then the low bits with BPLCON3's LOCT"));
            });
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Wait at x"));
                ui.add(egui::DragValue::new(&mut gradient.x).range(0..=0xE2).hexadecimal(2, false, true).prefix("$"));
                ui.label(crate::i18n::tr("a change every"));
                ui.add(egui::DragValue::new(&mut gradient.every).range(1..=32));
                ui.label(crate::i18n::tr("lines"));
                if gradient.aga {
                    ui.label(crate::i18n::tr("BPLCON3"));
                    ui.add(egui::DragValue::new(&mut gradient.bplcon3).hexadecimal(4, false, true).prefix("$"));
                }
            });
            ui.add_space(4.0);
            let mut remove = None;
            egui::Grid::new("amiga-gradient-stops").num_columns(4).show(ui, |ui| {
                for (i, stop) in gradient.stops.iter_mut().enumerate() {
                    ui.label(crate::i18n::tr("Line"));
                    ui.add(egui::DragValue::new(&mut stop.line).range(0..=LINES as u16 - 1).hexadecimal(3, false, true).prefix("$"));
                    ui.color_edit_button_srgb(&mut stop.rgb);
                    if ui.small_button(crate::i18n::tr("Remove")).clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = remove.filter(|_| gradient.stops.len() > 2) {
                gradient.stops.remove(i);
            }
            if ui.button(crate::i18n::tr("Add a stop")).clicked() {
                let last = gradient.stops.iter().map(|s| s.line).max().unwrap_or(0x2C);
                let line = (last + 16).min(LINES as u16 - 1);
                gradient.stops.push(Stop { line, rgb: colour_at(&gradient.stops, line) });
            }
            ui.add_space(6.0);
            let source = gradient.source();
            let changes = ops.iter().filter(|o| matches!(o, Copper::Move { register: r, .. } if *r == register)).count();
            ui.label(crate::i18n::tf!("{} copper instructions ({} bytes), {changes} colour writes.", ops.len(), ops.len() * 4));
            egui::ScrollArea::vertical().id_salt("amiga-gradient-source").max_height(180.0).show(ui, |ui| {
                ui.label(RichText::new(&source).monospace().small());
            });
            if ui.button(crate::i18n::tr("Put it in after the cursor's line")).on_hover_text(crate::i18n::tr("As dc.w lines, inside a copper list before its END")).clicked() && !source.is_empty() {
                asked = Some(Asked::Insert(source));
            }
        });
    });
    asked
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_are_found_by_name_address_or_words() {
        let bplcon0 = amiga_hw::named("BPLCON0").unwrap();
        assert!(matches(bplcon0, "bplcon"));
        assert!(matches(bplcon0, "$100"));
        assert!(matches(bplcon0, "dff100"));
        assert!(matches(bplcon0, "interlace"));
        assert!(!matches(bplcon0, "sprite"));
    }

    #[test]
    fn the_window_draws_both_tabs_and_puts_lines_in() {
        let (mut app, ctx) = super::super::tests::app_with("");
        let text = "copper:\n\tdc.w\t$0180,$0000\n\tdc.w\t$ffff,$fffe\n";
        app.docs[0].text.type_text(text);
        let source = crate::amiga_source::Source::scan(text, None);
        app.docs[0].text.intel = Some(Box::new(crate::code_intel::CodeIntel { hardware: Some(std::sync::Arc::new(source)), ..Default::default() }));
        for tab in [Tab::Registers, Tab::Copper, Tab::Gradient] {
            app.open_amiga_hardware(tab);
            for _ in 0..2 {
                ctx.run_ui(Default::default(), |ui| app.amiga_hw_window(ui.ctx())).textures_delta.clear();
            }
            assert!(app.amiga_hw.is_some());
        }
        app.docs[0].text.move_caret_to_char(1, 0);
        app.put_in_line(&amiga_hw::copper_line(Copper::Move { register: 0x182, value: 0x0F00 }));
        assert_eq!(&*app.docs[0].text.line(2), "\tdc.w $0182,$0F00\t; COLOR01");
        assert_eq!(&*app.docs[0].text.line(3), "\tdc.w\t$ffff,$fffe");
    }
}
