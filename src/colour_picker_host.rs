//! The colour picker's window (Tools, Colour picker): a colour chosen by hue, saturation and value, shown and
//! typed in each form the file at the cursor can use, written over the colour at the cursor in the form that
//! colour already has, with the nearest colour a machine can show, a contrast checker, and an eyedropper that
//! picks from a picture taken of the whole screen. What the colours are and how they're written is `colour_picker`.

use super::*;
use crate::colour_picker::{self as pick, Context, Machine, Notation, Rgb, Shot};
use crate::view_extras::Form;
use egui::{Color32, RichText, Sense, vec2};
use std::sync::mpsc::{Receiver, TryRecvError};

/// What the person chose to be shown: what the file wants, plain web colours, or a machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Auto,
    Web,
    Machine(Machine),
}

/// The colour written in the file that the picker will write over.
#[derive(Clone, Debug)]
struct Target {
    line: usize,
    from: usize,
    /// What was there when it was found, to be sure it's still there when the new one is written.
    text: String,
    notation: Notation,
}

/// A row being typed in, which keeps what's typed until the person leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Hex,
    Rgb,
    Hsl,
    Oklch,
    Name,
    Code,
    Second,
}

enum Screen {
    Nothing,
    Waiting(Receiver<Result<Shot, String>>),
    Ready { shot: Shot, texture: egui::TextureHandle },
    Failed(String),
}

pub(super) struct PickerWindow {
    rgb: Rgb,
    alpha: u8,
    /// What the file in front wants, found when the window was opened.
    detected: Context,
    choice: Choice,
    target: Option<Target>,
    /// A form chosen by hand to write in; otherwise the colour's own, or the file's usual.
    write_as: Option<Notation>,
    editing: Option<(Field, String)>,
    second: Rgb,
    screen: Screen,
    /// Seconds to wait before the picture of the screen is taken, to get the Limoniter window out of the way.
    delay: u64,
}

/// What the window asks of the editor, once it's drawn.
enum Asked {
    Write(String, Notation),
    Copy(String),
    Grab,
}

fn rgb32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// The ids the swatch's popup in the text view uses to ask for this window.
pub(super) fn ask_id() -> egui::Id {
    egui::Id::unique("limoniter-open-colour-picker")
}

impl PickerWindow {
    /// For a screenshot: the colours of a machine chosen by the first word of its name.
    pub(super) fn show_machine(&mut self, name: &str) {
        let name = name.to_ascii_lowercase();
        if let Some(m) = Machine::ALL.into_iter().find(|m| !name.is_empty() && m.name().to_ascii_lowercase().starts_with(&name)) {
            self.choice = Choice::Machine(m);
        }
    }

    fn context(&self) -> Context {
        match self.choice {
            Choice::Auto => self.detected,
            Choice::Web => Context::Web,
            Choice::Machine(m) => Context::Machine(m),
        }
    }

    /// The form the colour will be written in.
    fn notation(&self) -> Notation {
        self.write_as.or(self.target.as_ref().map(|t| t.notation)).unwrap_or_else(|| Notation::default_for(self.context()))
    }

    /// The forms offered to write in: the web's, then the machine's if there is one.
    fn forms(&self) -> Vec<Notation> {
        let mut forms: Vec<Notation> = [Form::Hex6, Form::Hex3, Form::Hex8, Form::Rgb, Form::Rgba, Form::Hsl, Form::Oklch, Form::Named].into_iter().map(Notation::Web).collect();
        if let Context::Machine(m) = self.context() {
            let mut own = vec![Notation::default_for(self.context())];
            match m {
                Machine::AmigaAga => own.push(Notation::Machine(m, pick::Style::AgaLow)),
                Machine::Next | Machine::Atari8 | Machine::Atari2600 | Machine::Nes => own.extend([pick::Style::Decimal, pick::Style::ZeroX(2)].map(|s| Notation::Machine(m, s))),
                Machine::Cpc | Machine::Bbc | Machine::C64 | Machine::Msx | Machine::Spectrum => own.extend([pick::Style::Dollar(2), pick::Style::Ampersand(2)].map(|s| Notation::Machine(m, s))),
                _ => {}
            }
            own.extend(forms);
            forms = own;
        }
        forms
    }
}

/// A text box for one form of the colour. What's typed is kept while the box has the keyboard; a colour that reads
/// is returned at once, so the others change as it's typed.
fn row(ui: &mut egui::Ui, editing: &mut Option<(Field, String)>, field: Field, label: &str, shown: &str, parse: impl Fn(&str) -> bool) -> Option<String> {
    let mut found = None;
    ui.horizontal(|ui| {
        ui.add_sized([96.0, 18.0], egui::Label::new(RichText::new(crate::i18n::tr(label)).color(theme::text_dim()).size(12.5)));
        let own = editing.as_ref().filter(|(f, _)| *f == field).map(|(_, t)| t.clone());
        let mut text = own.clone().unwrap_or_else(|| shown.to_string());
        let bad = own.as_ref().is_some_and(|t| !parse(t));
        let edit = egui::TextEdit::singleline(&mut text).font(egui::TextStyle::Monospace).desired_width(250.0).text_color(if bad { Color32::from_rgb(220, 70, 70) } else { theme::text() });
        let response = ui.add(edit);
        if response.changed() {
            if parse(&text) {
                found = Some(text.clone());
            }
            *editing = Some((field, text));
        } else if response.has_focus() && own.is_none() {
            *editing = Some((field, text));
        }
        if response.lost_focus() {
            *editing = None;
        }
    });
    found
}

/// A square of colour, with a thin edge so a white or black one shows against the window.
fn swatch(ui: &mut egui::Ui, size: egui::Vec2, rgb: Rgb) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    ui.painter().rect_filled(rect, 2.0, rgb32(rgb));
    ui.painter().rect_stroke(rect, 2.0, egui::Stroke::new(1.0, theme::text_dim()), egui::StrokeKind::Inside);
    response
}

impl Limoniter {
    /// Opens the picker on the colour at the cursor, if there is one.
    pub(super) fn open_colour_picker(&mut self) {
        let mut window = PickerWindow { rgb: [0, 122, 204], alpha: 255, detected: Context::Web, choice: Choice::Auto, target: None, write_as: None, editing: None, second: [255, 255, 255], screen: Screen::Nothing, delay: 0 };
        if !self.no_file() && !self.docs[self.active].hex_mode {
            let doc = &self.docs[self.active];
            let sample: String = (0..doc.text.line_count().min(400)).map(|i| doc.text.line(i).to_string()).collect::<Vec<_>>().join("\n");
            window.detected = pick::detect(&doc.language_name(), &doc.name(), &sample);
            let (line, caret) = doc.text.caret_char_position();
            let text = doc.text.line(line).to_string();
            if let Some(found) = pick::literal_at(&text, caret, window.detected) {
                window.rgb = found.rgb;
                window.alpha = found.alpha;
                window.target = Some(Target { line, from: found.from, text: text.chars().skip(found.from).take(found.to - found.from).collect(), notation: found.notation });
            }
        }
        // Reopening keeps the colour to compare against and the machine chosen.
        if let Some(old) = self.colour_picker.take() {
            window.second = old.second;
            if old.choice != Choice::Auto {
                window.choice = old.choice;
            }
        }
        self.colour_picker = Some(Box::new(window));
    }

    /// Writes text over the colour the picker found (if it's still there), or at the cursor.
    fn write_colour(&mut self, window: &mut PickerWindow, text: &str, notation: Notation) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message("Colour picker", crate::i18n::tr("Open a file that can be changed to write the colour into it.").into());
            return;
        }
        let buffer = &mut self.docs[self.active].text;
        let found = window.target.clone().filter(|t| t.line < buffer.line_count() && buffer.line(t.line).chars().skip(t.from).take(t.text.chars().count()).collect::<String>() == t.text);
        match found {
            Some(target) => {
                let line = buffer.line(target.line).to_string();
                let byte = |chars: usize| line.char_indices().nth(chars).map_or(line.len(), |(b, _)| b);
                buffer.select_match(target.line, byte(target.from), byte(target.from + target.text.chars().count()));
                buffer.type_text(text);
                window.target = Some(Target { line: target.line, from: target.from, text: text.to_string(), notation });
            }
            None => {
                buffer.type_text(text);
                let (line, caret) = buffer.caret_char_position();
                let n = text.chars().count();
                window.target = Some(Target { line, from: caret.saturating_sub(n), text: text.to_string(), notation });
            }
        }
        self.pending_focus = true;
    }

    /// Starts taking a picture of the screen, off to one side.
    fn grab_in_background(window: &mut PickerWindow, ctx: &egui::Context) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (delay, ctx) = (window.delay, ctx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(delay));
            let _ = sender.send(pick::grab_screen());
            ctx.request_repaint();
        });
        window.screen = Screen::Waiting(receiver);
    }

    pub(super) fn colour_picker_window(&mut self, ctx: &egui::Context) {
        // The swatch beside a colour in the text asks for this window with its More button.
        if ctx.data_mut(|d| d.remove_temp::<bool>(ask_id())).unwrap_or(false) {
            self.open_colour_picker();
        }
        let Some(mut window) = self.colour_picker.take() else {
            return;
        };
        if let Screen::Waiting(receiver) = &window.screen {
            match receiver.try_recv() {
                Ok(Ok(shot)) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([shot.width, shot.height], &shot.rgba);
                    let name = "colour-picker-screen";
                    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                    window.screen = Screen::Ready { shot, texture };
                }
                Ok(Err(why)) => window.screen = Screen::Failed(why),
                Err(TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
                Err(TryRecvError::Disconnected) => window.screen = Screen::Failed(crate::i18n::tr("The screen couldn't be copied.").into()),
            }
        }
        let mut open = true;
        let mut asked = None;
        crate::fit::Dialog::new(crate::i18n::tr("Colour picker")).open(&mut open).resizable(false).default_pos([120.0, 70.0]).show(ctx, |ui| {
            // Scrolls on a short screen, rather than running off the bottom of it.
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 110.0).max(200.0)).auto_shrink([true, true]).show(ui, |ui| draw(ui, &mut window, &mut asked, self.no_file()));
        });
        // The picture of the screen, picked from.
        let mut picked = None;
        let mut leave = false;
        if matches!(window.screen, Screen::Ready { .. } | Screen::Failed(_) | Screen::Waiting(_)) {
            let mut shown = true;
            crate::fit::Dialog::new(crate::i18n::tr("Pick a colour from the screen")).id(egui::Id::unique("colour-picker-screen-window")).open(&mut shown).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                picked = screen_view(ui, &window.screen, window.context());
            });
            leave = !shown;
        }
        if let Some(rgb) = picked {
            window.rgb = rgb;
            window.alpha = 255;
            window.editing = None;
            leave = true;
        }
        if leave {
            window.screen = Screen::Nothing;
        }
        match asked {
            Some(Asked::Write(text, notation)) => self.write_colour(&mut window, &text, notation),
            Some(Asked::Copy(text)) => ctx.copy_text(text),
            Some(Asked::Grab) => Self::grab_in_background(&mut window, ctx),
            None => {}
        }
        if open {
            self.colour_picker = Some(window);
        }
    }
}

/// The window's contents.
fn draw(ui: &mut egui::Ui, w: &mut PickerWindow, asked: &mut Option<Asked>, no_file: bool) {
    let context = w.context();
    // The picker proper: hue, saturation and value, and opacity where the form has it.
    let mut colour = Color32::from_rgba_unmultiplied(w.rgb[0], w.rgb[1], w.rgb[2], w.alpha);
    let with_alpha = matches!(w.notation(), Notation::Web(Form::Hex8 | Form::Rgba));
    let before = colour;
    egui::color_picker::color_picker_color32(ui, &mut colour, if with_alpha { egui::color_picker::Alpha::OnlyBlend } else { egui::color_picker::Alpha::Opaque });
    if colour != before {
        let [r, g, b, a] = colour.to_srgba_unmultiplied();
        (w.rgb, w.alpha) = ([r, g, b], if with_alpha { a } else { w.alpha });
        w.editing = None;
    }
    ui.add_space(4.0);
    // Which colours: what the file wants, or one chosen by hand.
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Colours of")).color(theme::text_dim()).size(12.5));
        let current = match w.choice {
            Choice::Auto => match w.detected {
                Context::Web => crate::i18n::tr("The file (web colours)").to_string(),
                Context::Machine(m) => crate::i18n::tf!("The file ({})", crate::i18n::tr(m.name())),
            },
            Choice::Web => crate::i18n::tr("Web colours only").to_string(),
            Choice::Machine(m) => crate::i18n::tr(m.name()).to_string(),
        };
        egui::ComboBox::from_id_salt("colour-picker-machine").selected_text(current).width(300.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut w.choice, Choice::Auto, crate::i18n::tr("From the file"));
            ui.selectable_value(&mut w.choice, Choice::Web, crate::i18n::tr("Web colours only"));
            for m in Machine::ALL {
                ui.selectable_value(&mut w.choice, Choice::Machine(m), crate::i18n::tr(m.name()));
            }
        });
    });
    ui.separator();
    // The colour in each web form, each one editable.
    let mut set = None;
    let hex = pick::write_web(w.rgb, w.alpha, if w.alpha == 255 { Form::Hex6 } else { Form::Hex8 });
    let rgb = pick::write_web(w.rgb, w.alpha, if w.alpha == 255 { Form::Rgb } else { Form::Rgba });
    let (name, exact) = pick::nearest_name(w.rgb);
    let name_shown = if exact { name.to_string() } else { crate::i18n::tf!("{name} (nearest)") };
    for (field, label, shown) in [(Field::Hex, "Hex", hex), (Field::Rgb, "RGB", rgb), (Field::Hsl, "HSL", pick::write_web(w.rgb, w.alpha, Form::Hsl)), (Field::Oklch, "OKLCH", pick::oklch_text(w.rgb)), (Field::Name, "Name", name_shown)] {
        let accept = |t: &str| pick::parse_web(t).is_some();
        if let Some(text) = row(ui, &mut w.editing, field, label, &shown, accept) {
            set = pick::parse_web(&text);
        }
    }
    if let Some((rgb, alpha)) = set {
        (w.rgb, w.alpha) = (rgb, alpha);
    }
    // The machine's colours: the nearest it can show, and how it's asked for.
    if let Context::Machine(m) = context {
        ui.separator();
        let snapped = pick::snap(m, w.rgb);
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tf!("Nearest on the {}", crate::i18n::tr(m.name()))).strong());
            swatch(ui, vec2(34.0, 18.0), w.rgb).on_hover_text(crate::i18n::tr("The colour picked"));
            ui.label("\u{2192}");
            swatch(ui, vec2(34.0, 18.0), snapped.rgb).on_hover_text(crate::i18n::tr("The nearest the machine can show"));
            if snapped.rgb != w.rgb && ui.button(crate::i18n::tr("Use it")).on_hover_text(crate::i18n::tr("Change the colour picked to the one the machine shows")).clicked() {
                w.rgb = snapped.rgb;
                w.editing = None;
            }
        });
        for (label, text) in pick::code_lines(m, snapped.value) {
            ui.horizontal(|ui| {
                ui.add_sized([150.0, 18.0], egui::Label::new(RichText::new(crate::i18n::tr(label)).color(theme::text_dim()).size(12.5)));
                ui.label(RichText::new(&text).monospace());
            });
        }
        // The machine's own number typed in.
        let typed = pick::write_number(m, snapped.value, Notation::default_for(context).machine_style());
        let accept = move |t: &str| code_value(m, t).is_some();
        if let Some(text) = row(ui, &mut w.editing, Field::Code, "Number", &typed, accept)
            && let Some(rgb) = code_value(m, &text).and_then(|v| pick::decode(m, v))
        {
            w.rgb = rgb;
        }
        let grid = m.grid();
        if !grid.is_empty() {
            let columns = match grid.len() {
                0..=16 => grid.len(),
                17..=27 => 9,
                28..=128 => 16,
                _ => 32,
            };
            let size = if grid.len() > 128 { 11.0 } else { 18.0 };
            ui.add_space(4.0);
            ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
            let near = grid.iter().position(|c| *c == snapped.rgb);
            for (r, cells) in grid.chunks(columns).enumerate() {
                ui.horizontal(|ui| {
                    for (c, rgb) in cells.iter().enumerate() {
                        let response = swatch(ui, vec2(size, size), *rgb);
                        let index = r * columns + c;
                        if near == Some(index) {
                            ui.painter().rect_stroke(response.rect.expand(1.0), 2.0, egui::Stroke::new(2.0, theme::text()), egui::StrokeKind::Outside);
                        }
                        if response.on_hover_text(crate::i18n::tf!("Colour {index}")).clicked() {
                            w.rgb = *rgb;
                            w.editing = None;
                        }
                    }
                });
            }
            ui.spacing_mut().item_spacing = vec2(8.0, 3.0);
        }
    }
    ui.separator();
    contrast_part(ui, w);
    ui.separator();
    // Writing it: in the form of the colour under the cursor, or one chosen.
    let notation = w.notation();
    let text = pick::write(w.rgb, w.alpha, notation);
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Write as")).color(theme::text_dim()).size(12.5));
        let own = w.write_as.is_none() && w.target.is_some();
        let shown = if own { crate::i18n::tf!("{} (as it is)", crate::i18n::tr(notation.name())) } else { crate::i18n::tr(notation.name()).to_string() };
        egui::ComboBox::from_id_salt("colour-picker-write").selected_text(shown).width(210.0).show_ui(ui, |ui| {
            if w.target.is_some() && ui.selectable_label(w.write_as.is_none(), crate::i18n::tr("As it is in the file")).clicked() {
                w.write_as = None;
            }
            for form in w.forms() {
                if ui.selectable_label(w.write_as == Some(form), crate::i18n::tr(form.name())).clicked() {
                    w.write_as = Some(form);
                }
            }
        });
        ui.label(RichText::new(&text).monospace().strong());
    });
    ui.horizontal(|ui| {
        let replace = w.target.is_some();
        let label = if replace { crate::i18n::tr("Write over the colour") } else { crate::i18n::tr("Write at the cursor") };
        if ui.add_enabled(!no_file, egui::Button::new(label)).clicked() {
            *asked = Some(Asked::Write(text.clone(), notation));
        }
        if ui.button(crate::i18n::tr("Copy")).clicked() {
            *asked = Some(Asked::Copy(text.clone()));
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Pick from the screen\u{2026}")).on_hover_text(crate::i18n::tr("Takes a picture of the whole screen to pick a colour from, wherever it is")).clicked() {
            *asked = Some(Asked::Grab);
        }
        egui::ComboBox::from_id_salt("colour-picker-delay").selected_text(if w.delay == 0 { crate::i18n::tr("At once").to_string() } else { crate::i18n::tf!("After {} seconds", w.delay) }).width(110.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut w.delay, 0, crate::i18n::tr("At once"));
            for seconds in [3, 5] {
                ui.selectable_value(&mut w.delay, seconds, crate::i18n::tf!("After {} seconds", seconds));
            }
        });
    });
}

/// A machine's number typed, in any way a number is written.
fn code_value(m: Machine, text: &str) -> Option<u32> {
    let t = text.trim();
    let value = if let Some(h) = t.strip_prefix('$').or_else(|| t.strip_prefix('&')).or_else(|| t.strip_prefix("0x")) {
        u32::from_str_radix(h, 16).ok()?
    } else {
        t.parse::<u32>().ok()?
    };
    // Eight-bit bytes for the Next, as in a file.
    let value = if m == Machine::Next && t.trim_start_matches(['$', '&']).len() <= 2 && (t.starts_with('$') || t.starts_with('&')) { pick::next_nine(value) } else { value };
    pick::decode(m, value).map(|_| value)
}

/// The contrast checker: the colour as text on a second colour, with WCAG 2's ratio and grades.
fn contrast_part(ui: &mut egui::Ui, w: &mut PickerWindow) {
    ui.label(RichText::new(crate::i18n::tr("Contrast (WCAG 2)")).strong());
    let hex = pick::write_web(w.second, 255, Form::Hex6);
    if let Some(text) = row(ui, &mut w.editing, Field::Second, "Against", &hex, |t| pick::parse_web(t).is_some())
        && let Some((rgb, _)) = pick::parse_web(&text)
    {
        w.second = rgb;
    }
    let ratio = pick::contrast(w.rgb, w.second);
    let grades = pick::grades(ratio);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(150.0, 44.0), Sense::hover());
        ui.painter().rect_filled(rect, 3.0, rgb32(w.second));
        ui.painter().text(rect.center() - vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, crate::i18n::tr("Sample text"), egui::FontId::proportional(13.0), rgb32(w.rgb));
        ui.painter().text(rect.center() + vec2(0.0, 12.0), egui::Align2::CENTER_CENTER, crate::i18n::tr("Large sample"), egui::FontId::proportional(20.0), rgb32(w.rgb));
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("{ratio:.2} : 1")).monospace().strong().size(15.0));
            ui.horizontal(|ui| {
                for (label, pass) in [("AA", grades.aa), ("AAA", grades.aaa)] {
                    ui.label(RichText::new(format!("{} {label}", if pass { "\u{2714}" } else { "\u{2718}" })).color(if pass { Color32::from_rgb(70, 170, 90) } else { Color32::from_rgb(220, 90, 80) }));
                }
                ui.label(RichText::new(crate::i18n::tr("normal text")).color(theme::text_dim()).size(12.0));
            });
            ui.horizontal(|ui| {
                for (label, pass) in [("AA", grades.aa_large), ("AAA", grades.aaa_large)] {
                    ui.label(RichText::new(format!("{} {label}", if pass { "\u{2714}" } else { "\u{2718}" })).color(if pass { Color32::from_rgb(70, 170, 90) } else { Color32::from_rgb(220, 90, 80) }));
                }
                ui.label(RichText::new(crate::i18n::tr("large text")).color(theme::text_dim()).size(12.0));
            });
        });
    });
}

/// The picture of the screen shown for picking: the pixel under the pointer, and the one clicked is returned.
fn screen_view(ui: &mut egui::Ui, screen: &Screen, context: Context) -> Option<Rgb> {
    match screen {
        Screen::Nothing => None,
        Screen::Waiting(_) => {
            ui.label(crate::i18n::tr("Taking a picture of the screen\u{2026}"));
            None
        }
        Screen::Failed(why) => {
            ui.set_max_width(420.0);
            ui.label(why);
            None
        }
        Screen::Ready { shot, texture } => {
            let room = ui.ctx().content_rect().size() * 0.8;
            let scale = (room.x / shot.width as f32).min((room.y / shot.height as f32).max(0.1)).min(1.0);
            let size = vec2(shot.width as f32, shot.height as f32) * scale;
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            let under = response.hover_pos().and_then(|p| {
                let at = (p - rect.min) / scale;
                shot.pixel(at.x as usize, at.y as usize)
            });
            ui.horizontal(|ui| match under {
                Some(rgb) => {
                    swatch(ui, vec2(28.0, 16.0), rgb);
                    let mut text = pick::write_web(rgb, 255, Form::Hex6);
                    if let Context::Machine(m) = context {
                        text = format!("{text}  \u{2192}  {}", pick::write_number(m, pick::snap(m, rgb).value, Notation::default_for(context).machine_style()));
                    }
                    ui.label(RichText::new(text).monospace());
                }
                None => {
                    ui.label(crate::i18n::tr("Click the colour wanted."));
                }
            });
            if response.on_hover_cursor(egui::CursorIcon::Crosshair).clicked() { under } else { None }
        }
    }
}

impl Notation {
    /// The way a machine's number is written when the style is its usual; plain decimal for a web form.
    fn machine_style(self) -> pick::Style {
        match self {
            Notation::Machine(_, style) => style,
            Notation::Web(_) => pick::Style::Decimal,
        }
    }
}
