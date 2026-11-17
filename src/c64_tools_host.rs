//! The Commodore 64 windows for timing raster code: the raster line calculator and the raster interrupt helper,
//! each kept open beside the text and putting what it makes at the cursor; and, for 6502 source, the cycles a
//! selected block of lines takes, for the status bar.

use super::*;

/// The C64 windows, while they're open.
#[derive(Default)]
pub(super) struct C64Tools {
    pub raster: Option<Box<crate::c64_raster::Calculator>>,
    pub irqs: Option<Box<crate::c64_irq::Helper>>,
}

impl Limoniter {
    pub(super) fn open_c64_raster(&mut self) {
        self.c64_tools.raster.get_or_insert_with(Box::default);
    }

    pub(super) fn open_c64_irqs(&mut self) {
        self.c64_tools.irqs.get_or_insert_with(Box::default);
    }

    /// Text from one of the windows, at the cursor in the file in front, as one undoable change.
    fn insert_c64_text(&mut self, title: &str, text: &str) {
        if self.no_file() || self.docs[self.active].hex_mode || self.docs[self.active].text.read_only {
            self.message(title, "Open the source to put it in, with the cursor where it goes.".into());
            return;
        }
        let doc = &mut self.docs[self.active];
        doc.text.begin_undo_group(false);
        doc.text.type_text(text);
        doc.text.end_undo_group();
    }

    pub(super) fn c64_tools_windows(&mut self, ctx: &Context) {
        if let Some(mut calculator) = self.c64_tools.raster.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new(crate::i18n::tr("C64 raster lines")).open(&mut open).resizable(false).default_pos([240.0, 100.0]).show(ctx, |ui| {
                asked = calculator.draw(ui);
            });
            if let Some(crate::c64_raster::Asked::Insert(text)) = asked {
                self.insert_c64_text("C64 raster lines", &text);
            }
            if open {
                self.c64_tools.raster = Some(calculator);
            }
        }
        if let Some(mut helper) = self.c64_tools.irqs.take() {
            let mut open = true;
            let mut asked = None;
            egui::Window::new(crate::i18n::tr("C64 raster interrupts")).open(&mut open).resizable(false).default_pos([280.0, 120.0]).show(ctx, |ui| {
                asked = helper.draw(ui);
            });
            if let Some(crate::c64_irq::Asked::Insert(text)) = asked {
                self.insert_c64_text("C64 raster interrupts", &text);
            }
            if open {
                self.c64_tools.irqs = Some(helper);
            }
        }
    }

    /// For pictures: the C64 demo starter with its bar loop selected, and the calculator (with a bad line and two
    /// sprites) or the interrupt helper open beside it.
    pub(super) fn c64_screenshot_step(&mut self, step: &str) {
        if let Some(index) = self.new_kinds.iter().position(|k| k.name == "C64 demo (6502)") {
            self.new_of_kind(index);
        }
        self.assembly_tick();
        self.refresh_intel();
        let first = crate::c64_starters::DEMO.lines().position(|l| l.starts_with("bar_line")).unwrap_or(0);
        let doc = &mut self.docs[self.active];
        doc.text.move_caret(first.saturating_sub(12), 0, false);
        doc.text.move_caret(first + 11, 0, true);
        doc.text.reveal = true;
        if step == "c64-raster" {
            let mut calculator = crate::c64_raster::Calculator::default();
            calculator.setup.sprites = 0b11;
            self.c64_tools.raster = Some(Box::new(calculator));
        } else {
            self.open_c64_irqs();
        }
    }

    /// For 6502 source with lines selected: how many instructions there are and the cycles they take, least and
    /// most, from the program as last assembled, for the status bar.
    pub(super) fn selection_cycles(&self) -> Option<String> {
        if self.no_file() {
            return None;
        }
        let doc = &self.docs[self.active];
        if !doc.text.has_selection() || doc.text.anchor_line == doc.text.caret_line {
            return None;
        }
        let intel = doc.text.intel.as_ref().filter(|i| i.cpu == Some(crate::assemblers::Cpu::M6502) && i.version == doc.text.version())?;
        let assembly = intel.assembly.as_ref()?;
        let (first, mut last) = (doc.text.anchor_line.min(doc.text.caret_line), doc.text.anchor_line.max(doc.text.caret_line));
        // A selection ending at the start of a line doesn't take that line in.
        if (doc.text.caret_line == last && doc.text.caret_col == 0) || (doc.text.anchor_line == last && doc.text.anchor_col == 0) {
            last = last.saturating_sub(1);
        }
        let (sum, count) = crate::m6502_timing::block(assembly, intel.file, first, last);
        (count > 0).then(|| format!("Selected: {count} instruction{}, {} cycles", if count == 1 { "" } else { "s" }, sum.range()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;

    #[test]
    fn the_windows_draw_and_put_text_in() {
        let (mut app, ctx) = app_with("");
        app.open_c64_raster();
        app.open_c64_irqs();
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.c64_tools_windows(ui.ctx())).textures_delta.clear();
        }
        assert!(app.c64_tools.raster.is_some() && app.c64_tools.irqs.is_some());
        let note = app.c64_tools.raster.as_ref().unwrap().setup.note();
        app.insert_c64_text("C64 raster lines", &note);
        assert!(app.doc().text.line(0).starts_with("; Line $033 (51) on PAL"));
    }

    #[test]
    fn the_c64_starters_are_on_new_as() {
        let (mut app, _ctx) = app_with("");
        for (name, text) in [("C64 demo (6502)", crate::c64_starters::DEMO), ("C64 intro with music (6502)", crate::c64_starters::INTRO)] {
            let index = app.new_kinds.iter().position(|k| k.name == name).unwrap();
            app.new_of_kind(index);
            assert_eq!(app.docs[app.active].text.all_lines().len(), text.lines().count());
            assert_eq!(app.doc_cpu(app.active), Some(crate::assemblers::Cpu::M6502));
        }
    }

    #[test]
    fn a_selection_says_its_cycles() {
        let (mut app, _ctx) = app_with("");
        let index = app.new_kinds.iter().position(|k| k.name == "6502 assembler").unwrap();
        app.new_of_kind(index);
        let doc = &mut app.docs[app.active];
        doc.text.set_content(vec!["        * = $c000".into(), "start   lda $d012".into(), "        sta $d020,x".into(), "        bne start".into(), "        rts".into()]);
        let version = doc.text.version();
        let text = doc.text.all_lines().join("\n");
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, std::path::Path::new("t.a65"), &text).assembly;
        doc.text.intel = Some(Box::new(crate::code_intel::CodeIntel { assembly: Some(std::sync::Arc::new(assembly)), version, cpu: Some(crate::assemblers::Cpu::M6502), ..Default::default() }));
        assert_eq!(app.selection_cycles(), None);
        let doc = &mut app.docs[app.active];
        doc.text.anchor_line = 1;
        doc.text.anchor_col = 0;
        doc.text.caret_line = 4;
        doc.text.caret_col = 0;
        // LDA 4, STA abs,X 5, BNE 2 or 3 (back into the same page).
        assert_eq!(app.selection_cycles().as_deref(), Some("Selected: 3 instructions, 11\u{2013}12 cycles"));
    }
}
