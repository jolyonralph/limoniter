//! Two panels for assembler source: the program's call graph, drawn and clickable, and the instruction
//! reference, which follows the cursor's instruction.

use crate::assemblers::Cpu;
use crate::call_graph::{self, CallGraph};
use crate::diagram::{self, Direction, Scene};
use crate::panels::DocView;
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use std::path::PathBuf;
use std::sync::Arc;

/// What a panel asks the window to do: go to a line, in this file or in another the program includes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeAct {
    GoTo(Option<PathBuf>, usize),
    /// Ask the language server for the call hierarchy of the function at the cursor.
    AskServer,
}

/// The call graph panel's state: the graph as last worked out, and how it's drawn.
#[derive(Default)]
pub struct CallGraphPanel {
    /// The assembly it was made from, to know when to make it again.
    from: Option<Arc<crate::asm::Assembly>>,
    graph: CallGraph,
    files: Vec<PathBuf>,
    across: bool,
    /// How large it's drawn; 0 to fit the panel's width (but never larger than its own size).
    zoom: f32,
    /// The size it was last drawn at, which + and - start from.
    shown_zoom: f32,
    drawn: Option<(bool, Option<usize>, Scene, Vec<call_graph::egui_rect::Rect>)>,
    /// A language server's graph, and the document it was asked for.
    from_server: Option<(u64, CallGraph, Vec<PathBuf>)>,
}

fn say(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::text_faint()).size(12.5));
}

/// The assembled program behind a document, and which of its files the document is.
fn program(doc: &DocView<'_>) -> Option<(Arc<crate::asm::Assembly>, usize, Cpu)> {
    let intel = doc.text.intel.as_ref()?;
    Some((intel.assembly.clone()?, intel.file, intel.cpu?))
}

impl CallGraphPanel {
    /// Shows a language server's call hierarchy for a document.
    pub fn set_from_server(&mut self, doc: u64, graph: CallGraph, files: Vec<PathBuf>) {
        self.from_server = Some((doc, graph, files));
        self.from = None;
        self.drawn = None;
    }

    pub fn draw(&mut self, ui: &mut Ui, doc: &DocView<'_>) -> Vec<CodeAct> {
        let mut acts = Vec::new();
        if doc.no_file || doc.hex_mode {
            say(ui, crate::i18n::tr("The call graph is drawn for assembler source, or by a language server for other code."));
            return acts;
        }
        let file = match program(doc) {
            Some((assembly, file, cpu)) => {
                if self.from.as_ref().is_none_or(|from| !Arc::ptr_eq(from, &assembly)) {
                    self.graph = call_graph::build(&assembly, cpu);
                    self.files = assembly.files.clone();
                    self.from = Some(assembly);
                    self.drawn = None;
                }
                file
            }
            None => {
                // Other code: the language server's call hierarchy, from the function at the cursor, when asked.
                if ui.button(crate::i18n::tr("Ask the language server")).on_hover_text(crate::i18n::tr("The calls into and out of the function at the cursor, as the file's language server finds them")).clicked() {
                    acts.push(CodeAct::AskServer);
                }
                match &self.from_server {
                    Some((id, graph, files)) if *id == doc.id => {
                        if self.from.is_some() || self.graph != *graph {
                            self.graph = graph.clone();
                            self.files = files.clone();
                            self.from = None;
                            self.drawn = None;
                        }
                    }
                    _ => {
                        say(ui, crate::i18n::tr("The call graph is drawn for assembler source that Limoniter's own assembler can read (Z80, 6502, 6809, 68000, x86 or ARM) as you type. For other code, put the cursor in a function and ask its language server."));
                        return acts;
                    }
                }
                // The file's own place among the server's files, to mark the function the cursor's in.
                doc.path.and_then(|p| self.files.iter().position(|f| f == p)).unwrap_or(usize::MAX)
            }
        };
        ui.horizontal(|ui| {
            let down = crate::i18n::tr("Down");
            let across = crate::i18n::tr("Across");
            if ui.selectable_label(!self.across, down).on_hover_text(crate::i18n::tr("Callers above the routines they call")).clicked() {
                self.across = false;
            }
            if ui.selectable_label(self.across, across).on_hover_text(crate::i18n::tr("Callers to the left of the routines they call")).clicked() {
                self.across = true;
            }
            ui.separator();
            let shown = if self.shown_zoom > 0.0 { self.shown_zoom } else { 1.0 };
            if ui.small_button("\u{2212}").on_hover_text(crate::i18n::tr("Smaller")).clicked() {
                self.zoom = (shown / 1.25).max(0.2);
            }
            if ui.small_button("+").on_hover_text(crate::i18n::tr("Larger")).clicked() {
                self.zoom = (shown * 1.25).min(3.0);
            }
            if ui.selectable_label(self.zoom == 0.0, crate::i18n::tr("Fit")).on_hover_text(crate::i18n::tr("As wide as the panel")).clicked() {
                self.zoom = 0.0;
            }
        });
        if self.graph.calls.is_empty() {
            say(ui, crate::i18n::tr("No calls found yet. Routines that call others (with CALL, JSR, BSR or BL) to a label are drawn here, each joined to those it calls."));
            return acts;
        }
        let here = call_graph::routine_at(&self.graph, file, doc.text.caret_line);
        if self.drawn.as_ref().is_none_or(|(across, at, ..)| *across != self.across || *at != here) {
            let (scene, boxes) = call_graph::draw(&self.graph, &self.files, if self.across { Direction::Right } else { Direction::Down }, here);
            self.drawn = Some((self.across, here, scene, boxes));
        }
        let count = self.graph.routines.len();
        let calls: usize = self.graph.calls.iter().map(|(_, _, n)| n).sum();
        say(ui, &crate::i18n::tf!("{count} routines and {calls} calls. Click a routine to go to it."));
        if count > call_graph::MOST {
            say(ui, &crate::i18n::tf!("Only the first {} are drawn.", call_graph::MOST));
        }
        let Some((_, _, scene, boxes)) = &self.drawn else { return acts };
        let zoom = if self.zoom == 0.0 { (ui.available_width() / scene.width).clamp(0.2, 1.0) } else { self.zoom };
        self.shown_zoom = zoom;
        egui::ScrollArea::both().id_salt("call-graph-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(egui::vec2(scene.width * zoom, scene.height * zoom), egui::Sense::click());
            diagram::paint(ui.painter(), scene, rect.min, zoom);
            let Some(pointer) = response.hover_pos() else { return };
            let (x, y) = ((pointer.x - rect.min.x) / zoom, (pointer.y - rect.min.y) / zoom);
            let Some(n) = boxes.iter().position(|b| b.contains(x, y)) else { return };
            let routine = &self.graph.routines[n];
            let b = boxes[n];
            let highlight = egui::Rect::from_min_size(rect.min + egui::vec2(b.x * zoom, b.y * zoom), egui::vec2(b.w * zoom, b.h * zoom));
            ui.painter().rect_stroke(highlight, 5.0 * zoom, egui::Stroke::new(2.0, theme::accent()), egui::StrokeKind::Outside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            let callers = self.graph.calls.iter().filter(|(_, to, _)| *to == n).count();
            let callees = self.graph.calls.iter().filter(|(from, _, _)| *from == n).count();
            let place = self.files.get(routine.file).and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let tip = if self.graph.from_server {
                crate::i18n::tf!("{}, line {} of {place}. Called from {callers}; calls {callees}.", routine.name, routine.line + 1)
            } else {
                crate::i18n::tf!("{} at ${:04X}, line {} of {place}. Called from {callers} routines; calls {callees}.", routine.name, routine.address & 0xFFFF_FFFF, routine.line + 1)
            };
            let clicked = response.clicked();
            response.on_hover_text(tip);
            if clicked {
                let other = (routine.file != file || self.graph.from_server).then(|| self.files.get(routine.file).cloned()).flatten();
                acts.push(CodeAct::GoTo(other, routine.line));
            }
        });
        acts
    }
}

/// The instruction reference panel's state: what's been typed to look up.
#[derive(Default)]
pub struct InstructionPanel {
    look_up: String,
}

/// The processor a document's source is for.
fn cpu_of(doc: &DocView<'_>) -> Option<Cpu> {
    doc.text.intel.as_ref().and_then(|i| i.cpu).or_else(|| Cpu::for_language(&doc.language))
}

impl InstructionPanel {
    pub fn draw(&mut self, ui: &mut Ui, doc: &DocView<'_>) {
        let Some(cpu) = (!doc.no_file && !doc.hex_mode).then(|| cpu_of(doc)).flatten() else {
            say(ui, crate::i18n::tr("The instruction reference is for assembler source: Z80, 6502, 6809, 68000, x86 or ARM. It follows the instruction the cursor is on."));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Look up")).color(theme::text_dim()).size(12.5));
            ui.add(egui::TextEdit::singleline(&mut self.look_up).desired_width(ui.available_width() - 6.0).hint_text(crate::i18n::tr("the cursor's instruction")));
        });
        let typed = self.look_up.trim().to_ascii_lowercase();
        let mnemonic = if typed.is_empty() { crate::instruction_ref::instruction_at(cpu, &doc.text.line(doc.text.caret_line)) } else { Some(typed.clone()) };
        egui::ScrollArea::vertical().id_salt("instruction-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let found = mnemonic.as_deref().and_then(|m| crate::instruction_ref::reference(cpu, m));
            let Some(reference) = found else {
                if !typed.is_empty() {
                    // What's been typed so far, as a list of the instructions it could be.
                    let matches: Vec<String> = crate::instruction_ref::mnemonics(cpu).into_iter().filter(|m| m.starts_with(&typed)).collect();
                    if matches.is_empty() {
                        say(ui, &crate::i18n::tf!("The {} has no instruction {}.", cpu.name(), typed.to_uppercase()));
                    }
                    ui.horizontal_wrapped(|ui| {
                        for m in matches {
                            if ui.selectable_label(false, m.to_uppercase()).clicked() {
                                self.look_up = m;
                            }
                        }
                    });
                } else {
                    say(ui, crate::i18n::tr("Put the cursor on an instruction, or type one above."));
                }
                return;
            };
            show_reference(ui, &reference);
        });
    }
}

fn show_reference(ui: &mut Ui, reference: &crate::instruction_ref::Reference) {
    let cpu = reference.cpu;
    ui.horizontal(|ui| {
        ui.label(RichText::new(reference.mnemonic.to_uppercase()).size(18.0).strong().color(theme::strong_text()));
        ui.label(RichText::new(cpu.name()).color(theme::text_dim()).size(12.5));
    });
    match &reference.about {
        Some(about) => {
            ui.label(RichText::new(about.text).size(13.0));
            ui.add_space(4.0);
            let names = crate::instruction_ref::flag_names(cpu);
            egui::Grid::new("instruction-flags").spacing([10.0, 2.0]).show(ui, |ui| {
                ui.label(RichText::new(crate::i18n::tr("Flags")).color(theme::text_dim()).size(12.0));
                for name in names {
                    ui.label(RichText::new(*name).monospace().size(12.0).color(theme::text_dim()));
                }
                ui.end_row();
                ui.label("");
                for mark in about.flags.chars() {
                    let colour = if mark == '-' { theme::text_faint() } else { theme::accent() };
                    ui.label(RichText::new(mark.to_string()).monospace().size(12.5).color(colour)).on_hover_text(crate::instruction_ref::flag_meaning(mark));
                }
                ui.end_row();
            });
            // What each mark that's used means.
            let mut marks: Vec<char> = about.flags.chars().collect();
            marks.sort_unstable();
            marks.dedup();
            let legend: Vec<String> = marks.iter().map(|m| format!("{m} {}", crate::instruction_ref::flag_meaning(*m))).collect();
            ui.label(RichText::new(legend.join(", ")).color(theme::text_faint()).size(11.5));
            if !about.cycles.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(crate::i18n::tf!("Cycles: {}", about.cycles)).size(12.5));
            }
        }
        None => {
            ui.label(RichText::new(crate::i18n::tr("There's no note for this one yet; its forms are below.")).color(theme::text_faint()).size(12.5));
        }
    }
    ui.add_space(6.0);
    if reference.forms.is_empty() {
        say(ui, crate::i18n::tr("No form of it could be assembled here on its own."));
        return;
    }
    let with_cycles = reference.forms.iter().any(|f| f.cycles.is_some());
    egui::Grid::new("instruction-forms").striped(true).spacing([12.0, 3.0]).show(ui, |ui| {
        ui.label(RichText::new(crate::i18n::tr("Form")).color(theme::text_dim()).size(12.0));
        ui.label(RichText::new(crate::i18n::tr("Bytes")).color(theme::text_dim()).size(12.0));
        if with_cycles {
            ui.label(RichText::new(crate::i18n::tr("Cycles")).color(theme::text_dim()).size(12.0));
        }
        ui.end_row();
        for form in &reference.forms {
            ui.label(RichText::new(&form.form).monospace().size(12.5));
            ui.label(RichText::new(&form.encoding).monospace().size(12.5).color(theme::text_dim()));
            if with_cycles {
                ui.label(RichText::new(form.cycles.clone().unwrap_or_default()).monospace().size(12.5));
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(RichText::new(crate::i18n::tr("nn is a byte of the operand: n a number, d a displacement, label a place to branch to. Each form is shown with one register where others would do.")).color(theme::text_faint()).size(11.5));
}
