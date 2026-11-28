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
    OpticOs,
    Zx80,
    Zx81,
    JupiterAce,
}

impl Program {
    pub const ALL: [Self; 9] = [Self::None, Self::Spectrum, Self::Next, Self::C64, Self::Bbc, Self::OpticOs, Self::Zx80, Self::Zx81, Self::JupiterAce];

    fn id(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Spectrum => "spectrum",
            Self::Next => "next",
            Self::C64 => "c64",
            Self::Bbc => "bbc",
            Self::OpticOs => "opticos",
            Self::Zx80 => "zx80",
            Self::Zx81 => "zx81",
            Self::JupiterAce => "jupiterace",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "Nothing: just text",
            Self::Spectrum => "A Spectrum program (.tap)",
            Self::Next => "A Spectrum Next program (.bas)",
            Self::C64 => "A Commodore 64 program (.prg)",
            Self::Bbc => "A BBC BASIC program",
            Self::OpticOs => "An OpticOS program (.tap)",
            Self::Zx80 => "A ZX80 program (.o)",
            Self::Zx81 => "A ZX81 program (.p)",
            Self::JupiterAce => "Jupiter Ace Forth, typed in",
        }
    }

    fn machine(self) -> Option<basic::Machine> {
        match self {
            Self::None => None,
            Self::Spectrum => Some(basic::Machine::Spectrum),
            Self::Next => Some(basic::Machine::Next),
            Self::C64 => Some(basic::Machine::Commodore(basic::CbmMachine::C64)),
            Self::Bbc => Some(basic::Machine::Bbc),
            Self::OpticOs => Some(basic::Machine::OpticOs),
            Self::Zx80 => Some(basic::Machine::Zx80),
            Self::Zx81 => Some(basic::Machine::Zx81),
            Self::JupiterAce => Some(basic::Machine::JupiterAce),
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
            Self::new("Boriel ZX Basic (Spectrum)", "bas", "utf-8", line, highlight::BASIC_BORIEL, Program::None, crate::boriel::STARTER),
            Self::new("RGC BASIC", "bas", "utf-8", line, highlight::BASIC_RGC, Program::None, crate::rgc_basic::STARTER),
            Self::new("Turbo Rascal (TRSE)", "ras", "utf-8", line, highlight::TURBO_RASCAL, Program::None, crate::trse::STARTER),
            Self::new("OpticBASIC (OpticOS)", "bas", "utf-8", line, "", Program::OpticOs, "DEF PROC hello\n  PRINT \"Hello from Limoniter\"\nENDPROC\nhello"),
            Self::new("ZX80 BASIC", "bas", "utf-8", line, "", Program::Zx80, "10 FOR I=1 TO 10\n20 PRINT \"HELLO FROM LIMONITER \";I\n30 NEXT I"),
            Self::new("ZX81 BASIC", "bas", "utf-8", line, "", Program::Zx81, "10 PRINT \"HELLO FROM LIMONITER\""),
            Self::new("ZX81 machine code (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::zx81_program::STARTER),
            Self::new("Jupiter Ace Forth", "fth", "utf-8", line, "Forth", Program::JupiterAce, ": HELLO\n  .\" HELLO FROM LIMONITER\" CR ;\nHELLO"),
            Self::new("ZX Spectrum demo (Z80, How To Code)", "asm", "utf-8", line, "Assembly Z80", Program::None, &crate::howtocode_spectrum::single_file_demo()),
            Self::new("Z80 assembler", "asm", "utf-8", line, "Assembly Z80", Program::None, "        org 32768\nstart:  ld a,2\n        out (254),a         ; a red border\n        ret"),
            Self::new("6502 assembler", "a65", "utf-8", line, "Assembly 6502", Program::None, "        * = $c000\nstart:  lda #2\n        sta $d020           ; a red border\n        rts"),
            Self::new("6809 assembler", "asm", "utf-8", line, "Assembly 6809", Program::None, "        org $3000\nstart:  lda #1\n        rts\n        end start"),
            Self::new("68000 assembler", "s", "utf-8", line, "Assembly 680x0", Program::None, "start:  clr.w -(sp)\n        trap #1             ; back to the desktop"),
            Self::new("Amiga demo (68000, How To Code v8)", "s", "utf-8", line, "Assembly 680x0", Program::None, &crate::howtocode::single_file_demo()),
            Self::new("Amiga copper only (68000)", "s", "utf-8", line, "Assembly 680x0", Program::None, crate::amiga_starters::COPPER),
            Self::new("Atari ST demo (68000)", "s", "utf-8", line, "Assembly 680x0", Program::None, crate::st_starters::ST_DEMO),
            Self::new("Atari STE demo (68000)", "s", "utf-8", line, "Assembly 680x0", Program::None, crate::st_starters::STE_DEMO),
            Self::new("C64 demo (6502)", "a65", "utf-8", line, "Assembly 6502", Program::None, crate::c64_starters::DEMO),
            Self::new("C64 intro with music (6502)", "a65", "utf-8", line, "Assembly 6502", Program::None, crate::c64_starters::INTRO),
            Self::new("Atari 2600 (6502)", "asm", "utf-8", line, "Assembly 6502", Program::None, crate::atari2600::STARTER),
            Self::new("DOS program (x86, .COM)", "asm", "utf-8", line, "Assembly 80x86", Program::None, crate::pc_starters::COM),
            Self::new("PC boot sector (x86)", "asm", "utf-8", line, "Assembly 80x86", Program::None, crate::pc_starters::BOOT),
            Self::new("Vectrex cartridge (6809)", "asm", "utf-8", line, "Assembly 6809", Program::None, crate::vectrex::STARTER),
            Self::new("Sega Mega Drive / Genesis (68000)", "s", "utf-8", line, "Assembly 680x0", Program::None, crate::sega::MEGA_DRIVE_STARTER),
            Self::new("Sega Master System (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::sega::MASTER_SYSTEM_STARTER),
            Self::new("Sega Game Gear (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::sega::GAME_GEAR_STARTER),
            Self::new("Sega SG-1000 (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::sega::SG1000_STARTER),
            Self::new("ColecoVision (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::sega::COLECO_STARTER),
            Self::new("Game Boy program (SM83)", "asm", "utf-8", line, crate::highlight::ASM_SM83, Program::None, crate::gameboy::STARTER),
            Self::new("NES program (6502)", "asm", "utf-8", line, "Assembly 6502", Program::None, crate::nes::STARTER),
            Self::new("Atari 8-bit program (6502, .xex)", "asm", "utf-8", line, "Assembly 6502", Program::None, crate::atari8_run::STARTER),
            Self::new("Atari 7800 cartridge (6502)", "asm", "utf-8", line, "Assembly 6502", Program::None, crate::atari7800::STARTER),
            Self::new("Apple II program (6502)", "asm", "utf-8", line, "Assembly 6502", Program::None, crate::apple2_run::STARTER),
            Self::new("Amstrad CPC program (Z80)", "asm", "utf-8", line, "Assembly Z80", Program::None, crate::cpc_run::STARTER),
            Self::new("Dragon 32 program (6809)", "asm", "utf-8", line, "Assembly 6809", Program::None, crate::vectrex::DRAGON_STARTER),
            Self::new("QL program (68008)", "asm", "utf-8", line, "Assembly 680x0", Program::None, crate::ql::STARTER),
            Self::new("Manchester Baby (SSEM, 1948): Kilburn's first program", "ssem", "utf-8", line, highlight::SSEM, Program::None, crate::ssem::KILBURN),
            Self::new("Manchester Baby (SSEM, 1948): a first program", "ssem", "utf-8", line, highlight::SSEM, Program::None, crate::ssem::COUNTDOWN),
            Self::new("EDSAC (1949): Wilkes's table of squares", "edsac", "utf-8", line, highlight::EDSAC, Program::None, crate::edsac::SQUARES),
            Self::new("EDSAC (1949): a first program (Initial Orders 2)", "edsac", "utf-8", line, highlight::EDSAC, Program::None, crate::edsac::HELLO),
            Self::new("PDP-8 (1965): hello on the Teletype (PAL)", "pal", "utf-8", line, highlight::PAL8, Program::None, crate::pal8::HELLO),
            Self::new("PDP-8 (1965): echo the keyboard (PAL)", "pal", "utf-8", line, highlight::PAL8, Program::None, crate::pal8::ECHO),
            Self::new("C", "c", "utf-8", line, "", Program::None, "#include <stdio.h>\n\nint main(void)\n{\n    printf(\"Hello\\n\");\n    return 0;\n}\n"),
            Self::new("C for the Commodore 64 (cc65 or llvm-mos)", "c", "utf-8", line, "C", Program::None, crate::c_starters::C64),
            Self::new("C for the ZX Spectrum (z88dk)", "c", "utf-8", line, "C", Program::None, crate::c_starters::SPECTRUM),
            Self::new("C for the ZX Spectrum (SDCC)", "c", "utf-8", line, "C", Program::None, crate::c_starters::SPECTRUM_SDCC),
            Self::new("C for the Sega Master System (z88dk)", "c", "utf-8", line, "C", Program::None, crate::c_starters::MASTER_SYSTEM),
            Self::new("C for the Sega Mega Drive (SGDK)", "c", "utf-8", line, "C", Program::None, crate::c_starters::MEGA_DRIVE),
            Self::new("C for the Amiga (amiga-gcc or vbcc)", "c", "utf-8", line, "C", Program::None, crate::c_starters::AMIGA),
            Self::new("Markdown", "md", "utf-8", line, "Markdown", Program::None, ""),
            Self::new("HTML page", "html", "utf-8", line, "", Program::None, "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title></title>\n  <link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n\n  <script src=\"script.js\"></script>\n</body>\n</html>\n"),
            Self::new("CSS", "css", "utf-8", line, "", Program::None, ""),
            Self::new("JavaScript", "js", "utf-8", line, "", Program::None, ""),
            Self::new("TypeScript", "ts", "utf-8", line, "", Program::None, ""),
            Self::new("JSON", "json", "utf-8", line, "", Program::None, "{\n}\n"),
            Self::new("Python notebook (Jupyter)", "ipynb", "utf-8", Eol::Lf, "", Program::None, &crate::notebook::Notebook::new("python3", "Python 3 (ipykernel)", "python").to_json()),
            Self::new("R notebook (Jupyter)", "ipynb", "utf-8", Eol::Lf, "", Program::None, &crate::notebook::Notebook::new("ir", "R", "R").to_json()),
            Self::new("Python notebook (Jupyter): an example", "ipynb", "utf-8", Eol::Lf, "", Program::None, &crate::notebook::Notebook::python_example().to_json()),
            Self::new("R notebook (Jupyter): an example", "ipynb", "utf-8", Eol::Lf, "", Program::None, &crate::notebook::Notebook::r_example().to_json()),
            Self::new("Python", "py", "utf-8", line, "", Program::None, "def main():\n    print(\"Hello\")\n\n\nif __name__ == \"__main__\":\n    main()\n"),
            Self::new("C++", "cpp", "utf-8", line, "", Program::None, "#include <iostream>\n\nint main()\n{\n    std::cout << \"Hello\\n\";\n}\n"),
            Self::new("Rust", "rs", "utf-8", line, "", Program::None, "fn main() {\n    println!(\"Hello\");\n}\n"),
            Self::new("Go", "go", "utf-8", line, "", Program::None, "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"Hello\")\n}\n"),
            Self::new("Java", "java", "utf-8", line, "", Program::None, "public class Main {\n    public static void main(String[] args) {\n        System.out.println(\"Hello\");\n    }\n}\n"),
            Self::new("C#", "cs", "utf-8", line, "", Program::None, "Console.WriteLine(\"Hello\");\n"),
            Self::new("PHP", "php", "utf-8", line, "", Program::None, "<?php\n\n"),
            Self::new("Ruby", "rb", "utf-8", line, "", Program::None, ""),
            Self::new("Lua", "lua", "utf-8", line, "", Program::None, ""),
            Self::new("Shell script (sh)", "sh", "utf-8", line, "", Program::None, "#!/bin/sh\n\n"),
            Self::new("PowerShell script", "ps1", "utf-8", line, "", Program::None, ""),
            Self::new("Windows batch file", "bat", "utf-8", line, "", Program::None, "@echo off\n"),
            Self::new("SQL", "sql", "utf-8", line, "", Program::None, ""),
            Self::new("YAML", "yaml", "utf-8", line, "", Program::None, ""),
            Self::new("TOML", "toml", "utf-8", line, "", Program::None, ""),
            Self::new("XML", "xml", "utf-8", line, "", Program::None, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"),
            Self::new("Song lyrics (Suno and others)", "lyrics", "utf-8", line, highlight::SONG_LYRICS, Program::None, crate::lyrics::STARTER),
            Self::new("Plain text", "txt", "utf-8", line, "", Program::None, ""),
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

/// The sub-menus of File, New as, in their order; a kind of one's own goes in the last.
/// How many of the kinds last chosen New as offers at its top.
pub(super) const MOST_RECENT_KINDS: usize = 3;

pub(super) const GROUPS: [&str; 18] = [
    "Modern languages",
    "Text",
    "ZX Spectrum, ZX80, ZX81 and Jupiter Ace",
    "Sinclair QL",
    "Amstrad CPC",
    "Commodore",
    "Amiga",
    "Atari",
    "Acorn",
    "Apple II",
    "Nintendo",
    "Sega and ColecoVision",
    "Dragon and Vectrex",
    "PC",
    "Early computing",
    "Any machine",
    "C",
    "Your own",
];

impl NewKind {
    /// The sub-menu of File, New as this kind goes in: the usual kinds by name, any other in "Your own".
    pub(super) fn group(&self) -> &'static str {
        let name = self.name.as_str();
        let starts = |p: &str| name.starts_with(p);
        if starts("C for ") || name == "C" {
            "C"
        } else if starts("Manchester Baby") || starts("EDSAC") || starts("PDP-8") {
            "Early computing"
        } else if ["Z80 assembler", "6502 assembler", "6809 assembler", "68000 assembler", "RGC BASIC"].contains(&name) || starts("Turbo Rascal") {
            // Assemblers, and the languages that make programs for many machines.
            "Any machine"
        } else if starts("Spectrum") || starts("ZX Spectrum") || starts("ZX80") || starts("ZX81") || starts("Jupiter Ace") || starts("Boriel") || starts("OpticBASIC") {
            "ZX Spectrum, ZX80, ZX81 and Jupiter Ace"
        } else if starts("QL program") {
            "Sinclair QL"
        } else if starts("Amstrad CPC") {
            "Amstrad CPC"
        } else if starts("Apple II") {
            "Apple II"
        } else if starts("Commodore 64") || starts("C64") {
            "Commodore"
        } else if starts("Amiga demo") || starts("Amiga copper") {
            "Amiga"
        } else if starts("Atari") {
            "Atari"
        } else if starts("BBC BASIC") {
            "Acorn"
        } else if starts("Game Boy") || starts("NES ") {
            "Nintendo"
        } else if starts("Sega") || starts("ColecoVision") {
            "Sega and ColecoVision"
        } else if starts("Dragon") || starts("Vectrex") {
            "Dragon and Vectrex"
        } else if starts("DOS program") || starts("PC boot") {
            "PC"
        } else if ["Markdown", "Song lyrics (Suno and others)", "Plain text", "Commodore text (PETSCII)", "ZX Spectrum text", "Amiga text", "DOS text"].contains(&name) {
            "Text"
        } else if Self::usual().iter().any(|kind| kind.name == name) {
            "Modern languages"
        } else {
            "Your own"
        }
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
        if let Some(kind) = self.new_kinds.get(index).cloned() {
            // Remembered, to be offered first next time.
            self.recent_new_kinds.retain(|name| *name != kind.name);
            self.recent_new_kinds.insert(0, kind.name.clone());
            self.recent_new_kinds.truncate(MOST_RECENT_KINDS);
            self.save_settings_if_changed();
            self.new_from_kind(kind);
        }
    }

    /// A new file from one of the usual kinds, by its name, as a menu other than New as offers it (Tools, Early
    /// computing), whether or not the person's own New as list still has it.
    pub(super) fn new_of_usual_kind(&mut self, name: &str) {
        if let Some(kind) = NewKind::usual().into_iter().find(|kind| kind.name == name) {
            self.new_from_kind(kind);
        }
    }

    fn new_from_kind(&mut self, kind: NewKind) {
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
            doc.text.set_content(crate::i18n::comments(&kind.starter).lines().map(str::to_string).collect());
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
        crate::fit::Dialog::new(crate::i18n::tr("Kinds of new file")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.label(crate::i18n::tr("File, New as lists these. Each is a file ending, an encoding, line endings, colouring, and text to start with."));
                ui.add_space(6.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                            for (index, kind) in state.kinds.iter().enumerate() {
                                let name = if kind.name.is_empty() { "(no name)" } else { kind.name.as_str() };
                                if ui.selectable_label(state.selected == index, crate::i18n::tr(name)).clicked() {
                                    state.selected = index;
                                }
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(crate::i18n::tr("Add")).clicked() {
                                let at = (state.selected + 1).min(state.kinds.len());
                                let mut kind = state.kinds.get(state.selected).cloned().unwrap_or_else(|| NewKind::new("", "txt", "utf-8", Eol::CrLf, "", Program::None, ""));
                                kind.name = crate::i18n::tr("New kind").into();
                                state.kinds.insert(at, kind);
                                state.selected = at;
                            }
                            if ui.add_enabled(!state.kinds.is_empty(), egui::Button::new(crate::i18n::tr("Remove"))).clicked() {
                                state.kinds.remove(state.selected);
                                state.selected = state.selected.min(state.kinds.len().saturating_sub(1));
                            }
                            if ui.add_enabled(state.selected > 0, egui::Button::new(crate::i18n::tr("Up"))).clicked() {
                                state.kinds.swap(state.selected, state.selected - 1);
                                state.selected -= 1;
                            }
                            if ui.add_enabled(state.selected + 1 < state.kinds.len(), egui::Button::new(crate::i18n::tr("Down"))).clicked() {
                                state.kinds.swap(state.selected, state.selected + 1);
                                state.selected += 1;
                            }
                            if ui.button(crate::i18n::tr("Put back the usual list")).clicked() {
                                state.kinds = NewKind::usual();
                                state.selected = 0;
                            }
                        });
                    });
                    ui.add_space(16.0);
                    let Some(kind) = state.kinds.get_mut(state.selected) else {
                        ui.label(RichText::new(crate::i18n::tr("The list is empty. Add a kind, or put back the usual list.")).color(theme::text_dim()));
                        return;
                    };
                    ui.vertical(|ui| {
                        ui.set_width(380.0);
                        egui::Grid::new("new-kind").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                            ui.label(crate::i18n::tr("Name"));
                            ui.text_edit_singleline(&mut kind.name);
                            ui.end_row();
                            ui.label(crate::i18n::tr("File ending"));
                            ui.add(egui::TextEdit::singleline(&mut kind.ending).hint_text(crate::i18n::tr("bas, asm, txt...")));
                            ui.end_row();
                            ui.label(crate::i18n::tr("Encoding"));
                            let shown = charset::by_id(&kind.encoding).map_or_else(|| kind.encoding.clone(), |set| set.name.clone());
                            egui::ComboBox::from_id_salt("new-kind-encoding").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                                for set in charset::all() {
                                    ui.selectable_value(&mut kind.encoding, set.id.clone(), &set.name);
                                }
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("Line endings"));
                            egui::ComboBox::from_id_salt("new-kind-eol").width(260.0).selected_text(kind.eol.long_name()).show_ui(ui, |ui| {
                                for eol in EOLS {
                                    ui.selectable_value(&mut kind.eol, eol, eol.long_name());
                                }
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("Colouring"));
                            let shown = if kind.language.is_empty() { crate::i18n::tr("From the file ending").to_string() } else { kind.language.clone() };
                            egui::ComboBox::from_id_salt("new-kind-language").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                                ui.selectable_value(&mut kind.language, String::new(), "From the file ending");
                                for name in &languages {
                                    ui.selectable_value(&mut kind.language, name.clone(), name);
                                }
                            });
                            ui.end_row();
                            ui.label(crate::i18n::tr("A listing for"));
                            egui::ComboBox::from_id_salt("new-kind-program").width(260.0).selected_text(crate::i18n::tr(kind.program.label())).show_ui(ui, |ui| {
                                for program in Program::ALL {
                                    ui.selectable_value(&mut kind.program, program, crate::i18n::tr(program.label()));
                                }
                            });
                            ui.end_row();
                        });
                        ui.add_space(4.0);
                        ui.label(crate::i18n::tr("Starts with"));
                        ui.add(egui::TextEdit::multiline(&mut kind.starter).font(egui::TextStyle::Monospace).desired_rows(6).desired_width(f32::INFINITY));
                    });
                });
            }
            crate::fit::Part::Buttons => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    keep = ui.button(crate::i18n::tr("OK")).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
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
