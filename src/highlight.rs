//! Language highlighting, using syntect with the grammars from two-face. Only the lines being shown
//! are coloured. A grammar's state runs from line to line, so it's saved every so often, and a jump far
//! into a big file starts a little above the view instead of reading everything before it.

use crate::rainbow::scan_chars;
use crate::text_buffer::TextBuffer;
use eframe::egui::{self, Color32};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use syntect::easy::ScopeRegionIterator;
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Theme};
use syntect::parsing::{ParseState, Scope, SyntaxDefinition, SyntaxSet};
use two_face::theme::EmbeddedThemeName;

/// Longer lines stay plain: colouring them could hold up the window for a long time.
const MAX_LINE_BYTES: usize = 16 * 1024;
const CHECKPOINT_LINES: usize = 128;
/// How far to read forward from saved state. Beyond this, start nearer the view.
const MAX_WALK_LINES: usize = 5000;
/// Where colouring starts above the view after a jump, when it can't read from the top.
const LOOK_BEHIND_LINES: usize = 300;
/// Coloured lines this far from the view are forgotten.
const KEEP_AROUND_LINES: usize = 500;

/// Which language a document is shown as.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Language {
    #[default]
    Auto,
    Off,
    Named(usize),
}

pub struct Engine {
    syntaxes: SyntaxSet,
    /// The bundled syntax colour schemes, and those already read, by name.
    schemes: two_face::theme::EmbeddedLazyThemeSet,
    read: std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<Theme>>>,
    string_scope: Scope,
    comment_scope: Scope,
    /// Language names in menu order, each with its grammar's number.
    names: Vec<(String, usize)>,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

/// The scheme that colours nothing: every word in the theme's own text colour, as the older machines showed code.
pub const NO_SYNTAX_COLOURS: &str = "None";

/// The syntax colour schemes a document theme can use, as they're named in a theme and shown in the theme editor.
pub const SCHEMES: [EmbeddedThemeName; 29] = [
    EmbeddedThemeName::OneHalfDark,
    EmbeddedThemeName::OneHalfLight,
    EmbeddedThemeName::SolarizedDark,
    EmbeddedThemeName::SolarizedLight,
    EmbeddedThemeName::Nord,
    EmbeddedThemeName::Dracula,
    EmbeddedThemeName::GruvboxDark,
    EmbeddedThemeName::GruvboxLight,
    EmbeddedThemeName::MonokaiExtended,
    EmbeddedThemeName::MonokaiExtendedBright,
    EmbeddedThemeName::MonokaiExtendedLight,
    EmbeddedThemeName::MonokaiExtendedOrigin,
    EmbeddedThemeName::Github,
    EmbeddedThemeName::InspiredGithub,
    EmbeddedThemeName::CatppuccinLatte,
    EmbeddedThemeName::CatppuccinFrappe,
    EmbeddedThemeName::CatppuccinMacchiato,
    EmbeddedThemeName::CatppuccinMocha,
    EmbeddedThemeName::ColdarkCold,
    EmbeddedThemeName::ColdarkDark,
    EmbeddedThemeName::DarkNeon,
    EmbeddedThemeName::TwoDark,
    EmbeddedThemeName::SublimeSnazzy,
    EmbeddedThemeName::Zenburn,
    EmbeddedThemeName::Leet,
    EmbeddedThemeName::Base16EightiesDark,
    EmbeddedThemeName::Base16MochaDark,
    EmbeddedThemeName::Base16OceanDark,
    EmbeddedThemeName::Base16OceanLight,
];

/// Grammars written for Limoniter, added to the ones from two-face.
const OWN_GRAMMARS: [(&str, &str); 39] = [
    (ASM_80X86, include_str!("../data/syntaxes/assembly-80x86.sublime-syntax")),
    (ASM_680X0, include_str!("../data/syntaxes/assembly-680x0.sublime-syntax")),
    (ASM_6502, include_str!("../data/syntaxes/assembly-6502.sublime-syntax")),
    (ASM_Z80, include_str!("../data/syntaxes/assembly-z80.sublime-syntax")),
    (ASM_HLASM, include_str!("../data/syntaxes/assembly-hlasm.sublime-syntax")),
    (COBOL, include_str!("../data/syntaxes/cobol.sublime-syntax")),
    (JCL, include_str!("../data/syntaxes/jcl.sublime-syntax")),
    (BASIC_SINCLAIR, include_str!("../data/syntaxes/basic-sinclair.sublime-syntax")),
    (BASIC_COMMODORE, include_str!("../data/syntaxes/basic-commodore.sublime-syntax")),
    (ASM_6809, include_str!("../data/syntaxes/assembly-6809.sublime-syntax")),
    (ASM_8080, include_str!("../data/syntaxes/assembly-8080.sublime-syntax")),
    (ASM_65816, include_str!("../data/syntaxes/assembly-65816.sublime-syntax")),
    (ASM_ARM, include_str!("../data/syntaxes/assembly-arm.sublime-syntax")),
    (ASM_PDP11, include_str!("../data/syntaxes/assembly-pdp11.sublime-syntax")),
    (ASM_VAX, include_str!("../data/syntaxes/assembly-vax.sublime-syntax")),
    (ASM_AVR, include_str!("../data/syntaxes/assembly-avr.sublime-syntax")),
    (ASM_PIC, include_str!("../data/syntaxes/assembly-pic.sublime-syntax")),
    (REXX, include_str!("../data/syntaxes/rexx.sublime-syntax")),
    (FORTH, include_str!("../data/syntaxes/forth.sublime-syntax")),
    (MODULA2, include_str!("../data/syntaxes/modula2.sublime-syntax")),
    (SMALLTALK, include_str!("../data/syntaxes/smalltalk.sublime-syntax")),
    (PROLOG, include_str!("../data/syntaxes/prolog.sublime-syntax")),
    (APL, include_str!("../data/syntaxes/apl.sublime-syntax")),
    (MUMPS, include_str!("../data/syntaxes/mumps.sublime-syntax")),
    (XBASE, include_str!("../data/syntaxes/xbase.sublime-syntax")),
    (LOGO, include_str!("../data/syntaxes/logo.sublime-syntax")),
    (PLI, include_str!("../data/syntaxes/pli.sublime-syntax")),
    (RPG, include_str!("../data/syntaxes/rpg.sublime-syntax")),
    (BASIC_QBASIC, include_str!("../data/syntaxes/basic-qbasic.sublime-syntax")),
    (BASIC_AMOS, include_str!("../data/syntaxes/basic-amos.sublime-syntax")),
    (BASIC_BLITZ, include_str!("../data/syntaxes/basic-blitz.sublime-syntax")),
    (BASIC_STOS, include_str!("../data/syntaxes/basic-stos.sublime-syntax")),
    (AMIGAGUIDE, include_str!("../data/syntaxes/amigaguide.sublime-syntax")),
    (AMIGA_INSTALLER, include_str!("../data/syntaxes/amiga-installer.sublime-syntax")),
    (CONFIG_SYS, include_str!("../data/syntaxes/config-sys.sublime-syntax")),
    (RTF, include_str!("../data/syntaxes/rtf.sublime-syntax")),
    (POSTSCRIPT, include_str!("../data/syntaxes/postscript.sublime-syntax")),
    (INFORM6, include_str!("../data/syntaxes/inform6.sublime-syntax")),
    (ZIL, include_str!("../data/syntaxes/zil.sublime-syntax")),
];
const ASM_680X0: &str = "Assembly 680x0";
const ASM_6502: &str = "Assembly 6502";
const ASM_Z80: &str = "Assembly Z80";
const ASM_80X86: &str = "Assembly 80x86";
const ASM_HLASM: &str = "Assembly HLASM";
const ASM_6809: &str = "Assembly 6809";
const ASM_8080: &str = "Assembly 8080";
const ASM_65816: &str = "Assembly 65816";
const ASM_ARM: &str = "Assembly ARM (ARM2 and ARM3)";
const ASM_PDP11: &str = "Assembly PDP-11 (MACRO-11)";
const ASM_VAX: &str = "Assembly VAX MACRO";
const ASM_AVR: &str = "Assembly AVR";
const ASM_PIC: &str = "Assembly PIC (MPASM)";
const REXX: &str = "REXX";
const FORTH: &str = "Forth";
const MODULA2: &str = "Modula-2";
const SMALLTALK: &str = "Smalltalk";
const PROLOG: &str = "Prolog";
const APL: &str = "APL";
const MUMPS: &str = "MUMPS (M)";
const XBASE: &str = "xBase (dBase, Clipper, FoxPro)";
const LOGO: &str = "Logo";
const PLI: &str = "PL/I";
const RPG: &str = "RPG (IBM i)";
const BASIC_AMOS: &str = "BASIC (AMOS)";
const BASIC_BLITZ: &str = "BASIC (Blitz Basic)";
const BASIC_STOS: &str = "BASIC (STOS)";
const AMIGAGUIDE: &str = "AmigaGuide";
const AMIGA_INSTALLER: &str = "Amiga Installer";
const CONFIG_SYS: &str = "CONFIG.SYS (DOS)";
const RTF: &str = "RTF";
const POSTSCRIPT: &str = "PostScript";
const INFORM6: &str = "Inform 6";
const ZIL: &str = "ZIL (Zork Implementation Language)";
pub const BASIC_QBASIC: &str = "BASIC (QBasic, QuickBASIC and GW-BASIC)";
const COBOL: &str = "COBOL";
const JCL: &str = "JCL";
pub const BASIC_SINCLAIR: &str = "BASIC (Sinclair and Next)";
pub const BASIC_COMMODORE: &str = "BASIC (Commodore)";

/// File name endings used for assembler of any kind, which say nothing about which processor it's for.
const GENERAL_ASM_EXTENSIONS: [&str; 8] = ["asm", "s", "a", "i", "inc", "src", "mac", "as"];
/// How many instructions it takes before the text can be said to be for one processor.
const MIN_ASM_EVIDENCE: usize = 3;

/// Which processor some assembler looks written for, from its instructions. None if it isn't clear:
/// that leaves it as whatever the file name would otherwise give, and the person can still choose.
pub fn guess_assembly(lines: &[std::sync::Arc<str>]) -> Option<&'static str> {
    const M68K: [&str; 14] = ["move", "movem", "moveq", "lea", "dbra", "dbf", "bsr", "tst", "clr", "pea", "link", "unlk", "rte", "swap"];
    const M6502: [&str; 22] = ["lda", "sta", "ldx", "stx", "ldy", "sty", "inx", "iny", "dex", "dey", "bne", "beq", "bcc", "bcs", "bpl", "bmi", "jsr", "rts", "tax", "txa", "pha", "pla"];
    const Z80: [&str; 26] = ["ld", "ldir", "lddr", "djnz", "jr", "jp", "exx", "rlca", "rrca", "cpir", "otir", "inir", "ex", "halt", "rst", "ldi", "ldd", "outi", "rla", "rra", "srl", "sla", "nextreg", "ldirx", "ldws", "swapnib"];
    const M86: [&str; 26] = ["mov", "push", "pop", "xor", "cmp", "add", "sub", "inc", "dec", "int", "call", "jmp", "je", "jne", "jz", "jnz", "lea", "shl", "shr", "rep", "lodsb", "stosb", "movsb", "test", "and", "or"];
    // Registers only the x86 family has. SP is left out because other processors have one too.
    const REG16: [&str; 18] = ["ax", "bx", "cx", "dx", "si", "di", "bp", "ds", "es", "ss", "cs", "al", "ah", "bl", "bh", "cl", "ch", "dl"];
    const REG64: [&str; 8] = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp"];
    // IBM mainframe assembler, whose register-saving and base-register habits nothing else shares.
    const HLASM: [&str; 20] = ["csect", "dsect", "using", "drop", "ltorg", "balr", "basr", "bctr", "mvc", "mvi", "clc", "cli", "stm", "lm", "ltr", "zap", "pack", "unpk", "svc", "bal"];
    // Mnemonics only one family has, so that each can be told from the others in a general .asm or .mac file.
    const M6809: [&str; 20] = ["ldb", "stb", "ldd", "std", "leax", "leay", "leas", "leau", "pshs", "puls", "pshu", "pulu", "tfr", "exg", "lbra", "lbsr", "lbne", "lbeq", "cmpx", "abx"];
    const I8080: [&str; 16] = ["mvi", "lxi", "ldax", "stax", "lhld", "shld", "xchg", "dad", "inx", "dcx", "xra", "ana", "pchl", "sphl", "xthl", "hlt"];
    const W65816: [&str; 18] = ["rep", "sep", "xce", "xba", "jsl", "jml", "rtl", "phb", "plb", "phk", "phd", "pld", "tcd", "tcs", "mvn", "mvp", "pei", "brl"];
    const ARM: [&str; 17] = ["ldr", "str", "ldrb", "strb", "stmfd", "ldmfd", "stmia", "ldmia", "swi", "orr", "bic", "teq", "rsb", "mvn", "bl", "movs", "moveq"];
    const PDP11: [&str; 15] = ["movb", "clrb", "tstb", "bisb", "bicb", "bitb", "cmpb", "swab", "sob", "emt", ".mcall", ".psect", ".asciz", ".blkw", ".globl"];
    const VAX: [&str; 18] = ["movl", "pushl", "calls", "callg", "movab", "pushab", "addl2", "addl3", "subl2", "subl3", "cmpl", "tstl", "clrl", "incl", "decl", "brb", "brw", ".entry"];
    const AVR: [&str; 15] = ["ldi", "rjmp", "rcall", "sbi", "cbi", "sbic", "sbis", "sbrc", "sbrs", "movw", "adiw", "sbiw", "lpm", "brne", "breq"];
    const PIC: [&str; 13] = ["movlw", "movwf", "bsf", "bcf", "btfss", "btfsc", "decfsz", "incfsz", "addlw", "retlw", "banksel", "clrf", "movf"];
    // Interrupts for DOS and the PC's BIOS, which only 16-bit programs call.
    const DOS_CALLS: [&str; 5] = ["10h", "13h", "16h", "20h", "21h"];
    // The 6502's own: the 6809 shares LDA, STA, BNE and the rest.
    const M6502_ONLY: [&str; 16] = ["inx", "iny", "dex", "dey", "tax", "txa", "tay", "tya", "pha", "pla", "php", "plp", "sei", "cli", "cpy", "sty"];
    let (mut m68k, mut m6502, mut z80, mut m86, mut hlasm) = (0, 0, 0, 0, 0);
    let mut m6502_only = 0;
    let mut others = [(0usize, ASM_6809, &M6809[..]), (0, ASM_8080, &I8080[..]), (0, ASM_65816, &W65816[..]), (0, ASM_ARM, &ARM[..]), (0, ASM_PDP11, &PDP11[..]), (0, ASM_VAX, &VAX[..]), (0, ASM_AVR, &AVR[..]), (0, ASM_PIC, &PIC[..])];
    let mut sixty_four = false;
    for line in lines {
        let code = line.split(';').next().unwrap_or("").to_lowercase();
        if code.trim_start().starts_with('*') {
            continue;
        }
        let words: Vec<&str> = code.split_whitespace().collect();
        let tokens: Vec<&str> = code.split(|c: char| !c.is_ascii_alphanumeric()).filter(|token| !token.is_empty()).collect();
        sixty_four |= tokens.iter().any(|token| REG64.contains(token));
        // The instruction is the first word that isn't a label: at the left edge, either of the first two.
        let places = if code.starts_with([' ', '\t']) { 1 } else { 2 };
        let names: Vec<(&str, &str)> = words.iter().take(places).map(|word| word.split_once('.').unwrap_or((word, ""))).collect();
        if names.iter().any(|(name, size)| M68K.contains(name) || (matches!(*size, "b" | "w" | "l") && !name.is_empty())) {
            m68k += 1;
        }
        if names.iter().any(|(name, _)| M6502.contains(name)) {
            m6502 += 1;
        }
        if names.iter().any(|(name, _)| M6502_ONLY.contains(name)) {
            m6502_only += 1;
        }
        // Indexing only the 6809 has: from U, S or PC, or with auto increment.
        if [",u", ",s", "pcr", ",x+", ",y+", ",-x", ",-y", "[,x", "[,y"].iter().any(|mark| code.contains(mark)) {
            others[0].0 += 1;
        }
        if names.iter().any(|(name, _)| Z80.contains(name)) {
            z80 += 1;
        }
        if names.iter().any(|(name, _)| HLASM.contains(name)) {
            hlasm += 1;
        }
        for (count, _, mnemonics) in &mut others {
            if words.iter().take(places).any(|word| mnemonics.contains(word)) {
                *count += 1;
            }
        }
        let dos_call = names.iter().any(|(name, _)| *name == "int") && tokens.iter().any(|token| DOS_CALLS.contains(token));
        let masm_only = words.first().is_some_and(|word| matches!(*word, ".model" | ".8086" | ".8087" | ".186" | ".286" | ".386" | "assume"));
        let on_16_bit_registers = names.iter().any(|(name, _)| M86.contains(name)) && tokens.iter().any(|token| REG16.contains(token));
        if dos_call || masm_only || on_16_bit_registers {
            m86 += 1;
        }
    }
    // Code that uses 64-bit registers belongs with the newer x86-64 grammar, whatever else it does.
    if sixty_four {
        m86 = 0;
    }
    // With the 6809's own instructions about, only the 6502's own count for the 6502.
    if others[0].0 > 0 {
        m6502 = m6502_only;
    }
    // The 65816 runs 6502 code, so the 6502's instructions count for it too once its own turn up.
    if others[2].0 > 0 {
        others[2].0 += m6502;
    }
    let mut ranked = vec![(m68k, ASM_680X0), (m6502, ASM_6502), (z80, ASM_Z80), (m86, ASM_80X86), (hlasm, ASM_HLASM)];
    ranked.extend(others.iter().map(|(count, name, _)| (*count, *name)));
    let best = ranked.iter().max_by_key(|entry| entry.0)?;
    let next = ranked.iter().filter(|entry| entry.1 != best.1).map(|entry| entry.0).max().unwrap_or(0);
    (best.0 >= MIN_ASM_EVIDENCE && best.0 > next * 2).then_some(best.1)
}

/// Which BASIC a `.bas` listing kept as text is in. A `.bas` is most often QBasic or GW-BASIC, so that's what it's
/// taken for unless a Sinclair or Commodore listing gives itself away: Sinclair's two-word keywords and colour
/// statements, or the `{clr}`-style codes Commodore listings write their control characters as.
pub fn guess_basic(lines: &[std::sync::Arc<str>]) -> &'static str {
    const SINCLAIR: [&str; 14] = ["GO TO", "GO SUB", "BORDER ", "PAPER ", "INK ", "BRIGHT ", "RANDOMIZE", "{INK", "{AT ", "{UDG-", "DEFPROC", "LAYER ", "ENDPROC", "DEF FN "];
    const COMMODORE: [&str; 12] = ["{clr}", "{home}", "{rvon}", "{rvof}", "{down}", "{rght}", "{wht}", "{red}", "{blu}", "POKE53280", "POKE 53280", "{cyn}"];
    let (mut sinclair, mut commodore) = (0, 0);
    for line in lines {
        let upper = line.to_uppercase();
        sinclair += SINCLAIR.iter().filter(|word| upper.contains(*word)).count();
        commodore += COMMODORE.iter().filter(|word| line.contains(*word)).count();
    }
    if commodore > sinclair && commodore > 0 {
        BASIC_COMMODORE
    } else if sinclair > 0 {
        BASIC_SINCLAIR
    } else {
        BASIC_QBASIC
    }
}

/// Whether lines read as Inform 6: its story constants, its library includes or its [ routines.
/// Whether lines read as an Amiga Installer script, whatever the file is called (Install-English, Install_HD, ...):
/// lines that start with one of the Installer's own statements, or its variables. A shell script's "; $VER:" line
/// alone doesn't say: AmigaDOS scripts start that way too.
fn looks_like_installer(lines: &[std::sync::Arc<str>]) -> bool {
    const STATEMENTS: [&str; 22] = [
        "(welcome", "(copyfiles", "(copylib", "(procedure", "(askchoice", "(askdir", "(askoptions", "(askbool", "(askfile", "(askstring", "(makedir", "(makeassign", "(startup",
        "(complete", "(working", "(tooltype", "(onerror", "(textfile", "(protect", "(getversion", "(transcript", "(set @",
    ];
    let mut found = 0;
    for line in lines {
        let lower = line.trim_start().to_lowercase();
        if STATEMENTS.iter().any(|s| lower.starts_with(s)) || lower.contains("@default-dest") || lower.contains("@app-name") {
            found += 1;
        }
    }
    found >= 2
}

fn looks_like_inform(lines: &[std::sync::Arc<str>]) -> bool {
    lines.iter().any(|line| {
        let start = line.trim_start();
        let lower = start.to_lowercase();
        lower.starts_with("constant story") || lower.starts_with("constant headline") || lower.starts_with("include \"parser") || lower.starts_with("include \"verblib") || lower.starts_with("include \"grammar") || (start.starts_with('[') && start.trim_end().ends_with(';'))
    })
}

/// Loads the grammars in the background, so opening the window doesn't wait for them.
pub fn start_loading(ctx: egui::Context) {
    std::thread::spawn(move || {
        ENGINE.get_or_init(Engine::load);
        ctx.request_repaint();
    });
}

/// The grammars, loaded now if they haven't been, for tests and screenshots.
pub fn load_now() -> &'static Engine {
    ENGINE.get_or_init(Engine::load)
}

/// The grammars, once they've loaded.
pub fn engine() -> Option<&'static Engine> {
    ENGINE.get()
}

impl Engine {
    /// A syntax colour scheme by name. One that isn't known gives the usual one for dark or light documents.
    pub fn scheme(&self, name: &str) -> std::sync::Arc<Theme> {
        let mut read = self.read.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(theme) = read.get(name) {
            return theme.clone();
        }
        let found = SCHEMES.iter().find(|s| s.as_name() == name).copied().unwrap_or(if crate::theme::document_is_light() { EmbeddedThemeName::OneHalfLight } else { EmbeddedThemeName::OneHalfDark });
        let theme = std::sync::Arc::new(self.schemes.get(found).clone());
        read.insert(name.to_string(), theme.clone());
        theme
    }

    fn load() -> Self {
        let mut builder = two_face::syntax::extra_newlines().into_builder();
        for (name, text) in OWN_GRAMMARS {
            // A grammar that won't load leaves its language out; everything else still works. The tests check them all.
            if let Ok(grammar) = SyntaxDefinition::load_from_str(text, true, Some(name)) {
                builder.add(grammar);
            }
        }
        let syntaxes = builder.build();
        let mut names: Vec<(String, usize)> = syntaxes
            .syntaxes()
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.hidden && s.name != "Plain Text")
            .map(|(i, s)| (shown_name(&s.name).to_string(), i))
            .collect();
        names.sort_by_key(|(name, _)| name.to_lowercase());
        Self {
            syntaxes,
            schemes: two_face::theme::extra(),
            read: std::sync::Mutex::new(std::collections::HashMap::new()),
            string_scope: Scope::new("string").expect("a valid scope"),
            comment_scope: Scope::new("comment").expect("a valid scope"),
            names,
        }
    }

    pub fn names(&self) -> &[(String, usize)] {
        &self.names
    }

    pub fn name_of(&self, syntax: usize) -> &str {
        self.syntaxes.syntaxes().get(syntax).map_or("", |s| shown_name(&s.name))
    }

    /// Picks a language from the file's name, or else its first line. A general assembler file is looked
    /// into, to see which processor it's for. Plain text is no language.
    pub fn detect_with_sample(&self, path: Option<&Path>, sample: &[std::sync::Arc<str>]) -> Option<usize> {
        let extension = path.and_then(|p| p.extension()).and_then(|e| e.to_str()).map(str::to_lowercase);
        // Scripts have no grammar of their own, and read best coloured as Rust, which they look a lot like.
        if extension.as_deref() == Some(crate::scripting::EXTENSION) {
            return self.syntaxes.syntaxes().iter().position(|s| s.name == "Rust");
        }
        if extension.as_deref().is_some_and(|e| GENERAL_ASM_EXTENSIONS.contains(&e))
            && let Some(name) = guess_assembly(sample)
        {
            return self.syntaxes.syntaxes().iter().position(|s| s.name == name);
        }
        // Most .inf files are Windows setup files, left to the INI grammar; one that reads as Inform 6 is Inform.
        if extension.as_deref() == Some("inf") && looks_like_inform(sample) {
            return self.syntaxes.syntaxes().iter().position(|s| s.name == INFORM6);
        }
        if extension.as_deref() == Some("bas") {
            let name = guess_basic(sample);
            return self.syntaxes.syntaxes().iter().position(|s| s.name == name);
        }
        let first_line = format!("{}\n", sample.first().map_or("", |line| &**line));
        // A file the name says nothing of may be an Amiga Installer script, which has no first line of its own.
        if self.detect(path, "").is_none() && looks_like_installer(sample) {
            return self.syntaxes.syntaxes().iter().position(|s| s.name == "Amiga Installer");
        }
        self.detect(path, &first_line)
    }

    /// Picks a language from the file's name, or else its first line. Plain text is no language.
    pub fn detect(&self, path: Option<&Path>, first_line: &str) -> Option<usize> {
        let by_name = path.and_then(|p| {
            let file_name = p.file_name()?.to_str()?;
            let extension = p.extension().and_then(|e| e.to_str());
            self.syntaxes.find_syntax_by_extension(file_name).or_else(|| extension.and_then(|e| self.syntaxes.find_syntax_by_extension(e)))
        });
        let found = by_name.or_else(|| self.syntaxes.find_syntax_by_first_line(first_line))?;
        if found.name == "Plain Text" {
            return None;
        }
        self.syntaxes.syntaxes().iter().position(|s| s.name == found.name)
    }
}

/// The name a language goes by here, where it differs from its grammar's own, so assemblers read alike.
fn shown_name(name: &str) -> &str {
    match name {
        "x86_64 Assembly" | "Assembly (x86_64)" => "Assembly x86_64",
        other => other,
    }
}

impl Engine {
    /// The functions in some lines, as their grammar sees them, with the line each is on. Assembler labels count.
    /// It reads from the top, so it goes on another thread; `stop` lets it give up when the text has moved on.
    pub fn list_functions(&self, syntax: usize, lines: &[std::sync::Arc<str>], stop: &std::sync::atomic::AtomicBool) -> Vec<(usize, String)> {
        const MOST_LISTED: usize = 20_000;
        let wanted = [Scope::new("entity.name.function").expect("a valid scope"), Scope::new("entity.name.label").expect("a valid scope")];
        let Some(reference) = self.syntaxes.syntaxes().get(syntax) else {
            return Vec::new();
        };
        let mut state = ParseState::new(reference);
        let mut path = syntect::parsing::ScopeStack::new();
        let mut found = Vec::new();
        for (number, line) in lines.iter().enumerate() {
            if stop.load(std::sync::atomic::Ordering::Relaxed) || found.len() >= MOST_LISTED {
                break;
            }
            if line.len() > MAX_LINE_BYTES {
                continue;
            }
            let text = format!("{line}\n");
            let Ok(ops) = state.parse_line(&text, &self.syntaxes) else {
                continue;
            };
            let mut name = String::new();
            for (piece, op) in ScopeRegionIterator::new(&ops, &text) {
                if path.apply(op).is_err() {
                    break;
                }
                let inside = path.as_slice().iter().any(|scope| wanted.iter().any(|one| one.is_prefix_of(*scope)));
                if inside {
                    name.push_str(piece);
                } else if !name.trim().is_empty() {
                    break;
                }
            }
            let name = name.trim();
            if !name.is_empty() {
                found.push((number, name.to_string()));
            }
        }
        found
    }
}

/// How one line is coloured.
#[derive(Default, Clone, Debug)]
pub struct ColouredLine {
    /// Runs of characters and their colour, in order.
    pub spans: Vec<(usize, Color32)>,
    /// Brackets outside strings and comments: character position, bracket and nesting depth.
    pub brackets: Vec<(usize, char, Option<usize>)>,
    /// The text has changed since, so this is shown only until the new colours are ready.
    stale: bool,
}

#[derive(Clone)]
struct State {
    parse: ParseState,
    highlight: HighlightState,
    open_brackets: Vec<u8>,
}

struct Chain {
    next: usize,
    state: State,
    /// Read from the top of the file, so its state can be saved and relied on.
    exact: bool,
}

/// One document's colours.
#[derive(Default)]
pub struct LineColours {
    syntax: Option<usize>,
    checkpoints: BTreeMap<usize, State>,
    lines: HashMap<usize, ColouredLine>,
    chain: Option<Chain>,
    /// The syntax colour scheme the lines were coloured with.
    scheme: String,
}

fn fresh_state(engine: &Engine, syntax: usize, highlighter: &Highlighter<'_>) -> State {
    let reference = &engine.syntaxes.syntaxes()[syntax];
    State {
        parse: ParseState::new(reference),
        highlight: HighlightState::new(highlighter, syntect::parsing::ScopeStack::new()),
        open_brackets: Vec::new(),
    }
}

/// Colours a line and moves the grammar's state on past it.
fn colour_line(engine: &Engine, highlighter: &Highlighter<'_>, state: &mut State, line: &str) -> ColouredLine {
    if line.len() > MAX_LINE_BYTES {
        return ColouredLine::default();
    }
    let text = format!("{line}\n");
    let Ok(ops) = state.parse.parse_line(&text, &engine.syntaxes) else {
        return ColouredLine::default();
    };

    // The stretches inside strings and comments, where brackets don't count.
    let mut path = state.highlight.path.clone();
    let mut quoted: Vec<(usize, usize)> = Vec::new();
    let mut position = 0;
    for (piece, op) in ScopeRegionIterator::new(&ops, &text) {
        if path.apply(op).is_err() || piece.is_empty() {
            continue;
        }
        let length = piece.chars().count();
        let inside = path.as_slice().iter().any(|scope| engine.string_scope.is_prefix_of(*scope) || engine.comment_scope.is_prefix_of(*scope));
        if inside {
            quoted.push((position, position + length));
        }
        position += length;
    }

    let mut coloured = ColouredLine::default();
    for (style, piece) in HighlightIterator::new(&mut state.highlight, &ops, &text, highlighter) {
        let length = piece.trim_end_matches('\n').chars().count();
        if length == 0 {
            continue;
        }
        let colour = Color32::from_rgb(style.foreground.r, style.foreground.g, style.foreground.b);
        match coloured.spans.last_mut() {
            Some((run, last)) if *last == colour => *run += length,
            _ => coloured.spans.push((length, colour)),
        }
    }
    scan_chars(&mut state.open_brackets, line, &quoted, |index, bracket, depth| coloured.brackets.push((index, bracket, depth)));
    coloured
}

impl LineColours {
    pub fn is_active(&self) -> bool {
        self.syntax.is_some()
    }

    pub fn line(&self, line: usize) -> Option<&ColouredLine> {
        self.lines.get(&line)
    }

    pub fn set_syntax(&mut self, syntax: Option<usize>) {
        if self.syntax != syntax {
            *self = Self { syntax, scheme: std::mem::take(&mut self.scheme), ..Self::default() };
        }
    }

    fn invalidate_from(&mut self, line: usize) {
        // Saved state at a line is from before that line, so one at the changed line still holds.
        self.checkpoints.split_off(&(line + 1));
        for (number, coloured) in &mut self.lines {
            if *number >= line {
                coloured.stale = true;
            }
        }
        if self.chain.as_ref().is_some_and(|chain| chain.next > line) {
            self.chain = None;
        }
    }

    fn start_chain(&self, engine: &Engine, syntax: usize, highlighter: &Highlighter<'_>, need: usize) -> Chain {
        if let Some((line, state)) = self.checkpoints.range(..=need).next_back()
            && need - line <= MAX_WALK_LINES
        {
            return Chain { next: *line, state: state.clone(), exact: true };
        }
        let exact = need <= MAX_WALK_LINES;
        let next = if exact { 0 } else { need - LOOK_BEHIND_LINES };
        Chain { next, state: fresh_state(engine, syntax, highlighter), exact }
    }

    /// Colours what's needed of the lines top..bottom for up to the given time.
    /// Returns false if there's more to do, so the window can come back to it next frame.
    pub fn prepare(&mut self, engine: &Engine, buffer: &mut TextBuffer, top: usize, bottom: usize, budget: Duration) -> bool {
        let scheme = crate::theme::syntax_scheme();
        if self.scheme != scheme {
            *self = Self { syntax: self.syntax, scheme: scheme.clone(), ..Self::default() };
        }
        if let Some(from) = buffer.take_changed_from() {
            self.invalidate_from(from);
        }
        let Some(syntax) = self.syntax else {
            return true;
        };
        let bottom = bottom.min(buffer.line_count());
        let began = Instant::now();
        let plain = scheme == NO_SYNTAX_COLOURS;
        let theme = engine.scheme(&scheme);
        let highlighter = Highlighter::new(&theme);
        let mut coloured_any = false;
        loop {
            let need = (top..bottom).find(|r| self.lines.get(r).is_none_or(|l| l.stale));
            let Some(need) = need else {
                self.lines.retain(|line, _| line + KEEP_AROUND_LINES >= top && *line < bottom + KEEP_AROUND_LINES);
                return true;
            };
            let usable = self.chain.as_ref().is_some_and(|c| c.next <= need && need - c.next <= MAX_WALK_LINES);
            if !usable {
                self.chain = Some(self.start_chain(engine, syntax, &highlighter, need));
            }
            let Some(chain) = self.chain.as_mut() else {
                return true;
            };
            while chain.next <= need {
                if coloured_any && began.elapsed() >= budget {
                    return false;
                }
                let number = chain.next;
                let mut coloured = colour_line(engine, &highlighter, &mut chain.state, &buffer.line(number));
                if plain {
                    // Brackets and the strings and comments around them are still known; only the colours go.
                    let length = coloured.spans.iter().map(|(run, _)| run).sum();
                    coloured.spans = vec![(length, crate::theme::editor_text())];
                }
                coloured_any = true;
                chain.next += 1;
                if chain.exact && chain.next.is_multiple_of(CHECKPOINT_LINES) {
                    self.checkpoints.insert(chain.next, chain.state.clone());
                }
                if (top..bottom).contains(&number) {
                    self.lines.insert(number, coloured);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rainbow::RainbowCache;

    fn test_engine() -> &'static Engine {
        ENGINE.get_or_init(Engine::load)
    }

    #[test]
    fn scripts_are_coloured_as_rust() {
        let syntax = test_engine().detect_with_sample(Some(Path::new("Tidy.rhai")), &[]).unwrap();
        assert_eq!(test_engine().name_of(syntax), "Rust");
    }

    fn buffer_of(lines: &[&str]) -> TextBuffer {
        let mut buffer = TextBuffer::default();
        buffer.set_content(lines.iter().map(|l| l.to_string()).collect());
        buffer
    }

    fn syntax_for(file: &str) -> Option<usize> {
        test_engine().detect(Some(Path::new(file)), "")
    }

    /// Colours everything, however many goes it takes.
    fn functions_in(language: &str, text: &[&str]) -> Vec<(usize, String)> {
        let engine = Engine::load();
        let (_, syntax) = engine.names.iter().find(|(name, _)| name == language).unwrap_or_else(|| panic!("no {language}"));
        let lines: Vec<std::sync::Arc<str>> = text.iter().map(|line| std::sync::Arc::from(*line)).collect();
        engine.list_functions(*syntax, &lines, &std::sync::atomic::AtomicBool::new(false))
    }

    #[test]
    fn functions_are_found_from_the_grammar_with_their_lines() {
        let found = functions_in("Rust", &["// fn not_this", "fn first() {", "    let a = 1;", "}", "pub fn second(x: u8) -> u8 { x }"]);
        assert_eq!(found, [(1, "first".to_string()), (4, "second".to_string())]);
        let python = functions_in("Python", &["def one():", "    pass", "x = 'def nope():'", "def two(a):", "    return a"]);
        assert_eq!(python, [(0, "one".to_string()), (3, "two".to_string())]);
    }

    #[test]
    fn assembler_labels_count_as_functions() {
        let found = functions_in("Assembly 6502", &["start:", "    lda #1", "loop:", "    jmp loop"]);
        assert_eq!(found.iter().map(|(line, _)| *line).collect::<Vec<_>>(), [0, 2]);
        assert!(found[0].1.starts_with("start"));
    }

    #[test]
    fn being_told_to_stop_stops_it() {
        let engine = Engine::load();
        let (_, syntax) = engine.names.iter().find(|(name, _)| name == "Rust").unwrap();
        let lines: Vec<std::sync::Arc<str>> = vec![std::sync::Arc::from("fn a() {}")];
        assert!(engine.list_functions(*syntax, &lines, &std::sync::atomic::AtomicBool::new(true)).is_empty());
        assert!(engine.list_functions(usize::MAX, &lines, &std::sync::atomic::AtomicBool::new(false)).is_empty(), "no such grammar");
    }

    fn colour_all(colours: &mut LineColours, buffer: &mut TextBuffer) {
        let bottom = buffer.line_count();
        while !colours.prepare(test_engine(), buffer, 0, bottom, Duration::from_millis(50)) {}
    }

    fn colour_of_word(colours: &LineColours, buffer: &TextBuffer, line: usize, word: &str) -> Color32 {
        let text = buffer.line(line);
        let at = text.chars().collect::<String>().find(word).expect("the word is on the line");
        let at = text[..at].chars().count();
        let mut position = 0;
        for (length, colour) in &colours.line(line).expect("coloured").spans {
            position += length;
            if at < position {
                return *colour;
            }
        }
        panic!("past the end of the spans");
    }

    #[test]
    fn amiga_installer_scripts_are_known_by_what_they_say() {
        let script = arcs(&["; $VER: Install 40.17", ";", "(set @default-dest \"SYS:\")", "(welcome \"Install the program\")", "(copyfiles (source \"df0:c\") (dest \"c:\") (all))"]);
        let found = test_engine().detect_with_sample(Some(Path::new("Install-English")), &script).map(|s| test_engine().name_of(s).to_string());
        assert_eq!(found.as_deref(), Some("Amiga Installer"));
        let shell = arcs(&["; $VER: Startup-Sequence 40.2", "C:SetPatch QUIET", "C:Version >NIL:"]);
        assert_ne!(test_engine().detect_with_sample(Some(Path::new("Startup-Sequence")), &shell).map(|s| test_engine().name_of(s).to_string()).as_deref(), Some("Amiga Installer"));
    }

    #[test]
    fn languages_are_picked_from_the_file_name_and_first_line() {
        let engine = test_engine();
        assert_eq!(engine.name_of(syntax_for("main.rs").unwrap()), "Rust");
        assert_eq!(engine.name_of(syntax_for("page.html").unwrap()), "HTML");
        assert_eq!(engine.name_of(syntax_for("config.toml").unwrap()), "TOML", "from the larger language pack");
        assert_eq!(engine.name_of(syntax_for("app.ts").unwrap()), "TypeScript", "from the larger language pack");
        assert_eq!(syntax_for("notes.txt"), None, "plain text is no highlighting");
        assert_eq!(syntax_for("unknown.qqqq"), None);
        let script = engine.detect(Some(Path::new("script")), "#!/usr/bin/env python3\n").unwrap();
        assert_eq!(engine.name_of(script), "Python");
        assert!(engine.names().len() > 100, "{} languages", engine.names().len());
        assert!(engine.names().iter().all(|(name, _)| name != "Plain Text"));
    }

    fn assembler_colours(file: &str, lines: &[&str]) -> (LineColours, TextBuffer) {
        let mut buffer = buffer_of(lines);
        let mut colours = LineColours::default();
        let syntax = syntax_for(file);
        assert!(syntax.is_some(), "{file} is recognised");
        colours.set_syntax(syntax);
        colour_all(&mut colours, &mut buffer);
        (colours, buffer)
    }

    fn colour_of_text(colours: &LineColours, buffer: &TextBuffer, line: usize, word: &str) -> Color32 {
        colour_of_word(colours, buffer, line, word)
    }

    #[test]
    fn the_three_assemblers_are_named_and_found_from_their_file_names() {
        let engine = test_engine();
        for (file, name) in [("boot.s68", ASM_680X0), ("game.x68", ASM_680X0), ("a.68k", ASM_680X0), ("c64.a65", ASM_6502), ("main.6502", ASM_6502), ("rom.z80", ASM_Z80), ("spec.asm80", ASM_Z80), ("BOOT.Z80", ASM_Z80)] {
            let syntax = syntax_for(&file.to_lowercase()).unwrap_or_else(|| panic!("{file}"));
            assert_eq!(engine.name_of(syntax), name, "{file}");
        }
        for name in [ASM_680X0, ASM_6502, ASM_Z80] {
            assert!(engine.names().iter().any(|(listed, _)| listed == name), "{name} is in the menu");
        }
    }

    #[test]
    fn every_grammar_written_here_loads_under_its_own_name() {
        for (name, text) in OWN_GRAMMARS {
            let grammar = SyntaxDefinition::load_from_str(text, true, Some(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(grammar.name, name);
            assert!(test_engine().names().iter().any(|(listed, _)| listed == name), "{name} is in the menu");
        }
        for (file, name) in [("PAYROLL.CBL", COBOL), ("copybook.cpy", COBOL), ("a.cob", COBOL), ("BUILD.JCL", JCL), ("nightly.job", JCL), ("prog.mlc", ASM_HLASM), ("x.hlasm", ASM_HLASM)] {
            assert_eq!(syntax_for(&file.to_lowercase()).map(|s| test_engine().name_of(s)), Some(name), "{file}");
        }
        let job = test_engine().detect(Some(Path::new("NIGHTLY")), "//NIGHTLY JOB (ACCT),'RUN',CLASS=A\n").unwrap();
        assert_eq!(test_engine().name_of(job), JCL, "a job card says what it is");
    }

    #[test]
    fn basic_listings_get_their_colours() {
        let lines = ["  10 BORDER 1: LAYER 2,1", "  20 REM all of this is a comment", "  30 PRINT \"{INK 2}Hi\";%a MOD 2", "  40 ; LD BC,1234", "  50 PROC draw(): GO TO @top", "  60 LET x=$ff+@101: .nexload game.nex"];
        let (colours, buffer) = assembler_colours("demo.bas", &lines);
        let comment = colour_of_word(&colours, &buffer, 1, "comment");
        assert_eq!(colour_of_word(&colours, &buffer, 3, "LD"), comment, "NextBASIC's ; comment");
        let keyword = colour_of_word(&colours, &buffer, 0, "BORDER");
        assert_eq!(colour_of_word(&colours, &buffer, 0, "LAYER"), keyword, "Next's keywords too");
        assert_ne!(colour_of_word(&colours, &buffer, 2, "Hi"), keyword);
        assert_ne!(colour_of_word(&colours, &buffer, 2, "{INK 2}"), colour_of_word(&colours, &buffer, 2, "Hi"), "escapes stand out in strings");
        assert_ne!(colour_of_word(&colours, &buffer, 4, "draw"), colour_of_word(&colours, &buffer, 5, "x"), "a procedure's name");
        assert_ne!(colour_of_word(&colours, &buffer, 5, "$ff"), colour_of_word(&colours, &buffer, 5, "x"), "hex numbers");
        assert_ne!(colour_of_word(&colours, &buffer, 5, ".nexload"), keyword);
        let (colours, buffer) = assembler_colours("game.cbmbas", &["10 PRINT\"{clr}HELLO\":GOTO10", "20 DATA PRINT,1", "30 REM PRINT"]);
        let keyword = colour_of_word(&colours, &buffer, 0, "PRINT");
        assert_eq!(colour_of_word(&colours, &buffer, 0, "GOTO"), keyword, "keywords with no spaces round them");
        assert_ne!(colour_of_word(&colours, &buffer, 1, "PRINT,"), keyword, "DATA is data");
        assert_ne!(colour_of_word(&colours, &buffer, 2, "PRINT"), keyword, "and a REM is a comment");
    }

    #[test]
    fn the_newer_assemblers_and_rexx_are_found_and_coloured() {
        for (file, name) in [("boot.s09", ASM_6809), ("cpm.8080", ASM_8080), ("snes.65816", ASM_65816), ("riscos.arm", ASM_ARM), ("kernel.m11", ASM_PDP11), ("sys.mar", ASM_VAX), ("blink.avr", ASM_AVR), ("led.pic", ASM_PIC), ("script.rexx", REXX), ("start.rx", REXX)] {
            assert_eq!(syntax_for(file).map(|s| test_engine().name_of(s)), Some(name), "{file}");
        }
        let rexx = test_engine().detect(Some(Path::new("PROFILE")), "/* REXX */\n").unwrap();
        assert_eq!(test_engine().name_of(rexx), REXX, "a REXX program says so in its first comment");
        let guess = |lines: &[&str]| guess_assembly(&arcs(lines));
        assert_eq!(guess(&["start  ldx #$1000", "       ldb ,x+", "       stb ,y+", "       leax 1,x", "       pshs a,b", "       lbra start"]), Some(ASM_6809));
        assert_eq!(guess(&["       mvi a,0", "       lxi h,buf", "       inx h", "       dad d", "       xchg", "       hlt"]), Some(ASM_8080));
        assert_eq!(guess(&["       sep #$20", "       rep #$10", "       lda #$00", "       sta $2100", "       xce", "       jsl init"]), Some(ASM_65816), "6502 code with 65816 instructions in it");
        assert_eq!(guess(&["       ldr r0,[r1]", "       str r0,[r2]", "       stmfd sp!,{r0-r3,lr}", "       ldmfd sp!,{r0-r3,pc}", "       swi &11"]), Some(ASM_ARM));
        assert_eq!(guess(&["       movb (r0)+,r1", "       clrb r2", "       swab r3", "       sob r4,loop", "       .mcall .exit"]), Some(ASM_PDP11));
        assert_eq!(guess(&["       movl r0,r1", "       pushl #1", "       calls #1,foo", "       addl2 r1,r2", "       brb loop"]), Some(ASM_VAX));
        assert_eq!(guess(&["       ldi r16,0xff", "       out DDRB,r16", "       rjmp loop", "       sbi PORTB,0", "       rcall delay"]), Some(ASM_AVR));
        assert_eq!(guess(&["       movlw 0xff", "       movwf TRISB", "       bsf STATUS,RP0", "       btfss PORTA,0", "       decfsz count,f"]), Some(ASM_PIC));
        let (colours, buffer) = assembler_colours("blink.avr", &["; set up", "main:   ldi r16, 0xFF   ; all out", "        rjmp main"]);
        let comment = colour_of_word(&colours, &buffer, 0, "set");
        assert_eq!(colour_of_word(&colours, &buffer, 1, "all"), comment);
        assert_ne!(colour_of_word(&colours, &buffer, 1, "ldi"), colour_of_word(&colours, &buffer, 2, "rjmp"), "branches have their own colour");
        assert_ne!(colour_of_word(&colours, &buffer, 1, "r16"), colour_of_word(&colours, &buffer, 1, "0xFF"));
        let (colours, buffer) = assembler_colours("x.rexx", &["/* REXX /* nested */ still a comment */", "say 'Hello'' world' arg", "loop: do i = 1 to 10", "  call show i", "end"]);
        let comment = colour_of_word(&colours, &buffer, 0, "still");
        assert_ne!(colour_of_word(&colours, &buffer, 1, "say"), comment);
        assert_ne!(colour_of_word(&colours, &buffer, 1, "world"), colour_of_word(&colours, &buffer, 1, "say"), "a doubled quote stays in the string");
        assert_ne!(colour_of_word(&colours, &buffer, 2, "loop"), colour_of_word(&colours, &buffer, 3, "show"), "a label");
    }

    #[test]
    fn the_older_languages_are_found_by_their_file_names() {
        for (file, name) in [("words.fth", FORTH), ("Storage.mod", MODULA2), ("Storage.def", MODULA2), ("Kernel.st", SMALLTALK), ("family.pro", PROLOG), ("life.apl", APL), ("XUSER.mumps", MUMPS), ("invoice.prg", XBASE), ("std.ch", XBASE), ("flower.lgo", LOGO),
                             ("payroll.pli", PLI), ("orders.rpgle", RPG), ("game.bi", BASIC_QBASIC), ("game.amos", BASIC_AMOS), ("shooter.bb2", BASIC_BLITZ), ("demo.stos", BASIC_STOS), ("help.guide", AMIGAGUIDE), ("Install", AMIGA_INSTALLER), ("CONFIG.SYS", CONFIG_SYS),
                             ("letter.rtf", RTF), ("page.ps", POSTSCRIPT), ("figure.eps", POSTSCRIPT), ("game.i6", INFORM6), ("zork1.zil", ZIL)] {
            assert_eq!(syntax_for(file).map(|s| test_engine().name_of(s)), Some(name), "{file}");
        }
        let rtf = test_engine().detect(Some(Path::new("untitled")), "{\\rtf1\\ansi\n").unwrap();
        assert_eq!(test_engine().name_of(rtf), RTF);
        let ps = test_engine().detect(Some(Path::new("untitled")), "%!PS-Adobe-3.0\n").unwrap();
        assert_eq!(test_engine().name_of(ps), POSTSCRIPT);
        // Endings other grammars already have stay theirs.
        assert_eq!(syntax_for("notes.md").map(|s| test_engine().name_of(s)), Some("Markdown"));
        assert_eq!(syntax_for("unit.p").map(|s| test_engine().name_of(s)), Some("Pascal"));
        let inf = |lines: &[&str]| test_engine().detect_with_sample(Some(Path::new("game.inf")), &arcs(lines)).map(|s| test_engine().name_of(s).to_string());
        assert_eq!(inf(&["Constant Story \"CLOAK\";", "Include \"Parser\";"]).as_deref(), Some(INFORM6));
        assert_eq!(inf(&["[Version]", "Signature=\"$Windows NT$\""]).as_deref(), Some("INI"), "a Windows setup file stays INI");
    }

    #[test]
    fn a_bas_file_is_given_the_basic_it_is_written_in() {
        assert_eq!(guess_basic(&arcs(&["SCREEN 12", "DIM x AS INTEGER", "PRINT \"hello\""])), BASIC_QBASIC);
        assert_eq!(guess_basic(&arcs(&["10 PRINT \"HI\"", "20 GOTO 10"])), BASIC_QBASIC, "GW-BASIC looks like this too");
        assert_eq!(guess_basic(&arcs(&["10 BORDER 1: PAPER 1", "20 GO TO 10"])), BASIC_SINCLAIR);
        assert_eq!(guess_basic(&arcs(&["10 PRINT\"{clr}{red}HI\"", "20 GOTO10"])), BASIC_COMMODORE);
        let found = test_engine().detect_with_sample(Some(Path::new("game.bas")), &arcs(&["SUB Main", "END SUB"])).unwrap();
        assert_eq!(test_engine().name_of(found), BASIC_QBASIC);
    }

    #[test]
    fn the_older_languages_get_their_colours() {
        let (colours, buffer) = assembler_colours("w.fth", &[": square ( n -- n*n ) dup * ;", "\\ a comment", "10 square . cr"]);
        assert_eq!(colour_of_word(&colours, &buffer, 0, "n --"), colour_of_word(&colours, &buffer, 1, "a comment"), "a stack comment is a comment");
        assert_ne!(colour_of_word(&colours, &buffer, 0, "square"), colour_of_word(&colours, &buffer, 2, "square"), "a definition's name stands out");
        assert_ne!(colour_of_word(&colours, &buffer, 0, "dup"), colour_of_word(&colours, &buffer, 2, "10"));
        let (colours, buffer) = assembler_colours("m.mod", &["MODULE Hello; (* outer (* nested *) still *)", "BEGIN WriteString('hi') END Hello."]);
        assert_eq!(colour_of_word(&colours, &buffer, 0, "nested"), colour_of_word(&colours, &buffer, 0, "still"), "comments nest");
        assert_ne!(colour_of_word(&colours, &buffer, 1, "BEGIN"), colour_of_word(&colours, &buffer, 1, "WriteString"));
        let (colours, buffer) = assembler_colours("p.pro", &["% facts", "parent(tom, Bob) :- male(tom)."]);
        assert_ne!(colour_of_word(&colours, &buffer, 1, "Bob"), colour_of_word(&colours, &buffer, 1, "tom"), "variables start with a capital");
        let (colours, buffer) = assembler_colours("g.bi", &["' comment", "SUB Draw (x AS INTEGER)", "  PRINT x: GOSUB 100"]);
        assert_eq!(colour_of_word(&colours, &buffer, 0, "comment"), colour_of_word(&colours, &buffer, 0, "'"));
        assert_ne!(colour_of_word(&colours, &buffer, 1, "Draw"), colour_of_word(&colours, &buffer, 2, "x"));
        let (colours, buffer) = assembler_colours("a.ps", &["%!PS", "/Helvetica findfont 12 scalefont setfont", "(Hello (nested) world) show"]);
        assert_eq!(colour_of_word(&colours, &buffer, 2, "nested"), colour_of_word(&colours, &buffer, 2, "world"), "strings nest");
        assert_ne!(colour_of_word(&colours, &buffer, 1, "findfont"), colour_of_word(&colours, &buffer, 1, "Helvetica"));
        let (colours, buffer) = assembler_colours("x.rtf", &["{\\rtf1\\ansi{\\fonttbl\\f0 Times;}\\f0\\fs24 Caf\\'e9}"]);
        assert_ne!(colour_of_word(&colours, &buffer, 0, "fs24"), colour_of_word(&colours, &buffer, 0, "Times"));
    }

    #[test]
    fn cobol_colours_follow_its_columns() {
        let lines = [
            "000100 IDENTIFICATION DIVISION.                                         PAYROLL1",
            "000200*THIS WHOLE LINE IS A COMMENT",
            "000300 PROCEDURE DIVISION.",
            "000400 MAIN-PARA.",
            "000500     MOVE ZERO TO WS-TOTAL.",
            "000600     DISPLAY \"TOTAL IS \" WS-TOTAL. *> inline",
            "000700 01  WS-TOTAL  PIC 9(5)V99 VALUE 0.",
        ];
        let (colours, buffer) = assembler_colours("pay.cbl", &lines);
        let comment = colour_of_word(&colours, &buffer, 1, "THIS");
        assert_eq!(colour_of_word(&colours, &buffer, 0, "000100"), comment, "the sequence area is set apart like a comment");
        assert_eq!(colour_of_word(&colours, &buffer, 0, "PAYROLL1"), comment, "so is the identification area");
        assert_eq!(colour_of_word(&colours, &buffer, 5, "inline"), comment);
        let plain = colour_of_word(&colours, &buffer, 4, "WS-TOTAL");
        let verb = colour_of_word(&colours, &buffer, 4, "MOVE");
        assert_ne!(verb, plain);
        assert_ne!(verb, comment);
        assert_ne!(colour_of_word(&colours, &buffer, 3, "MAIN-PARA"), plain, "a paragraph name stands out");
        assert_ne!(colour_of_word(&colours, &buffer, 4, "ZERO"), plain);
        assert_ne!(colour_of_word(&colours, &buffer, 5, "TOTAL IS"), plain, "a literal is a string");
        assert_ne!(colour_of_word(&colours, &buffer, 6, "9(5)V99"), plain, "a picture string");
        assert_ne!(colour_of_word(&colours, &buffer, 6, "01"), plain, "a level number");
        assert_ne!(colour_of_word(&colours, &buffer, 0, "IDENTIFICATION"), plain);
    }

    #[test]
    fn jcl_colours_statements_remarks_and_instream_data() {
        let lines = [
            "//NIGHTLY  JOB (ACCT),'NIGHTLY RUN',CLASS=A",
            "//* A COMMENT",
            "//STEP1    EXEC PGM=SORT  SORT THE FILE",
            "//SORTIN   DD DSN=MY.DATA,DISP=SHR",
            "//SYSIN    DD *",
            "  SORT FIELDS=(1,10,CH,A)",
            "/*",
            "//SYSOUT   DD SYSOUT=*",
        ];
        let (colours, buffer) = assembler_colours("nightly.jcl", &lines);
        let comment = colour_of_word(&colours, &buffer, 1, "COMMENT");
        assert_eq!(colour_of_word(&colours, &buffer, 2, "THE FILE"), comment, "a remark after the operands");
        let operation = colour_of_word(&colours, &buffer, 2, "EXEC");
        let label = colour_of_word(&colours, &buffer, 2, "STEP1");
        assert_ne!(operation, label);
        assert_ne!(colour_of_word(&colours, &buffer, 3, "DSN"), colour_of_word(&colours, &buffer, 3, "MY"));
        let string = colour_of_word(&colours, &buffer, 0, "NIGHTLY RUN");
        assert_eq!(colour_of_word(&colours, &buffer, 5, "SORT FIELDS"), string, "in-stream data reads as data");
        assert_eq!(colour_of_word(&colours, &buffer, 7, "SYSOUT"), label, "after the delimiter, statements again");
    }

    #[test]
    fn hlasm_colours_fields_remarks_and_columns() {
        let lines = [
            "* A COMMENT LINE",
            "MYPROG   CSECT",
            "         STM   R14,R12,12(R13)   SAVE THE REGISTERS",
            "         LA    R2,=C'HELLO'",
            "         BR    R14",
            "         WTO   'A MESSAGE THAT GOES ON AND ON AND ON AND ON AND ON',   X00010000",
        ];
        let (colours, buffer) = assembler_colours("prog.mlc", &lines);
        let comment = colour_of_word(&colours, &buffer, 0, "COMMENT");
        assert_eq!(colour_of_word(&colours, &buffer, 2, "SAVE"), comment, "a remark after a blank");
        assert_eq!(colour_of_word(&colours, &buffer, 5, "00010000"), comment, "the sequence field");
        let label = colour_of_word(&colours, &buffer, 1, "MYPROG");
        let directive = colour_of_word(&colours, &buffer, 1, "CSECT");
        let instruction = colour_of_word(&colours, &buffer, 2, "STM");
        assert_ne!(label, directive);
        assert_ne!(instruction, directive);
        assert_ne!(colour_of_word(&colours, &buffer, 4, "BR"), instruction, "branches have their own colour");
        assert_ne!(colour_of_word(&colours, &buffer, 3, "HELLO"), comment);
        assert_ne!(colour_of_word(&colours, &buffer, 5, "WTO"), instruction, "a macro call isn't an instruction");
        let hlasm = arcs(&["MYPROG   CSECT", "         USING *,R12", "         STM   14,12,12(13)", "         BALR  12,0", "         MVC   OUT,IN", "         LTORG"]);
        assert_eq!(guess_assembly(&hlasm), Some(ASM_HLASM), "a general .asm file of mainframe code is found");
    }

    #[test]
    fn m68k_assembler_gets_its_colours() {
        let (colours, buffer) = assembler_colours("x.s68", &["* star comment", "loop:  MOVE.L  #$FF00,d0  ; note", "       bne.s  loop", "       dc.b   \"text\",0", "done   move.l (a0)+,d1", "       lea    add,a1", "add.l  d0,d1"]);
        let label = colour_of_text(&colours, &buffer, 1, "loop");
        let instruction = colour_of_text(&colours, &buffer, 1, "MOVE");
        let number = colour_of_text(&colours, &buffer, 1, "FF00");
        let register = colour_of_text(&colours, &buffer, 1, "d0");
        let comment = colour_of_text(&colours, &buffer, 1, "note");
        let branch = colour_of_text(&colours, &buffer, 2, "bne");
        let directive = colour_of_text(&colours, &buffer, 3, "dc.b");
        let string = colour_of_text(&colours, &buffer, 3, "text");
        for (name, other) in [("label", label), ("number", number), ("register", register), ("comment", comment), ("branch", branch), ("string", string)] {
            assert_ne!(instruction, other, "{name}");
        }
        assert_ne!(register, comment);
        assert_ne!(string, comment);
        assert_eq!(colour_of_text(&colours, &buffer, 0, "star"), comment, "a star at the start of a line is a comment");
        assert_eq!(colour_of_text(&colours, &buffer, 4, "done"), label, "a word at the left edge that isn't an instruction is a label");
        assert_eq!(colour_of_text(&colours, &buffer, 4, "move"), instruction);
        assert_ne!(colour_of_text(&colours, &buffer, 5, "add"), instruction, "as an operand, add is just a name");
        assert_eq!(colour_of_text(&colours, &buffer, 6, "add.l"), instruction, "an instruction at the left edge is still one");
        assert_ne!(directive, instruction);
    }

    #[test]
    fn m6502_assembler_gets_its_colours() {
        let (colours, buffer) = assembler_colours("x.a65", &["start:  LDA #$0F     ; note", "        sta $d020,x", "        !byte %1010, 12", "        .org $c000", "        jsr start", "value = $10"]);
        let label = colour_of_text(&colours, &buffer, 0, "start");
        let instruction = colour_of_text(&colours, &buffer, 0, "LDA");
        let number = colour_of_text(&colours, &buffer, 0, "0F");
        let comment = colour_of_text(&colours, &buffer, 0, "note");
        assert_ne!(instruction, number);
        assert_ne!(instruction, comment);
        assert_ne!(label, comment);
        assert_eq!(colour_of_text(&colours, &buffer, 1, "sta"), instruction);
        assert_eq!(colour_of_text(&colours, &buffer, 1, "d020"), number);
        let register = colour_of_text(&colours, &buffer, 1, "x");
        assert_ne!(register, number);
        assert_eq!(colour_of_text(&colours, &buffer, 2, "1010"), number);
        assert_ne!(colour_of_text(&colours, &buffer, 2, "!byte"), instruction, "ACME's directives");
        assert_ne!(colour_of_text(&colours, &buffer, 3, ".org"), instruction, "ca65's directives");
        assert_ne!(colour_of_text(&colours, &buffer, 4, "jsr"), instruction, "branches stand out");
        assert_eq!(colour_of_text(&colours, &buffer, 5, "10"), number);
    }

    #[test]
    fn z80_assembler_gets_its_colours() {
        let (colours, buffer) = assembler_colours("x.z80", &["start:  LD HL,0FFh    ; note", "        ex af,af'", "        ld a,'A'", "        jp nz,start", "        ret z", "        defb 1,%1010,$FF,#7F,10b"]);
        let instruction = colour_of_text(&colours, &buffer, 0, "LD");
        let register = colour_of_text(&colours, &buffer, 0, "HL");
        let number = colour_of_text(&colours, &buffer, 0, "0FFh");
        let comment = colour_of_text(&colours, &buffer, 0, "note");
        for other in [register, number, comment] {
            assert_ne!(instruction, other);
        }
        assert_eq!(colour_of_text(&colours, &buffer, 1, "af'"), register, "the shadow register's quote doesn't start a string");
        assert_eq!(colour_of_text(&colours, &buffer, 2, "a"), register);
        let string = colour_of_text(&colours, &buffer, 2, "'A'");
        assert_ne!(string, register);
        assert_eq!(colour_of_text(&colours, &buffer, 3, "nz"), register);
        assert_ne!(colour_of_text(&colours, &buffer, 3, "jp"), instruction);
        assert_ne!(colour_of_text(&colours, &buffer, 4, "z"), colour_of_text(&colours, &buffer, 4, "ret"), "a condition after ret");
        assert_eq!(colour_of_text(&colours, &buffer, 5, "%1010"), number);
        assert_eq!(colour_of_text(&colours, &buffer, 5, "$FF"), number);
        assert_eq!(colour_of_text(&colours, &buffer, 5, "#7F"), number);
    }

    fn arcs(lines: &[&str]) -> Vec<std::sync::Arc<str>> {
        lines.iter().map(|line| std::sync::Arc::from(*line)).collect()
    }

    #[test]
    fn x86_assembler_gets_its_colours() {
        let (colours, buffer) = assembler_colours("x.a86", &[".MODEL SMALL", "start:  MOV AX,0FFh    ; note", "        mov ds,ax", "        rep movsb", "        lea dx,msg", "        int 21h", "        jne start", "msg     db 'Hello$',13,10", "        mov byte ptr [bx+si],101b", "        fld st(1)"]);
        let directive = colour_of_text(&colours, &buffer, 0, ".MODEL");
        let label = colour_of_text(&colours, &buffer, 1, "start");
        let instruction = colour_of_text(&colours, &buffer, 1, "MOV");
        let register = colour_of_text(&colours, &buffer, 1, "AX");
        let number = colour_of_text(&colours, &buffer, 1, "0FFh");
        let comment = colour_of_text(&colours, &buffer, 1, "note");
        for (name, other) in [("directive", directive), ("label", label), ("register", register), ("number", number), ("comment", comment)] {
            assert_ne!(instruction, other, "{name}");
        }
        assert_eq!(colour_of_text(&colours, &buffer, 2, "ds"), register, "segment registers");
        assert_eq!(colour_of_text(&colours, &buffer, 3, "rep"), instruction);
        assert_eq!(colour_of_text(&colours, &buffer, 3, "movsb"), instruction, "the instruction after a prefix");
        let branch = colour_of_text(&colours, &buffer, 5, "int");
        assert_ne!(branch, instruction);
        assert_eq!(colour_of_text(&colours, &buffer, 6, "jne"), branch);
        assert_eq!(colour_of_text(&colours, &buffer, 7, "msg"), label);
        assert_eq!(colour_of_text(&colours, &buffer, 7, "db"), directive);
        assert_ne!(colour_of_text(&colours, &buffer, 7, "Hello"), register);
        assert_eq!(colour_of_text(&colours, &buffer, 8, "101b"), number);
        assert_ne!(colour_of_text(&colours, &buffer, 8, "ptr"), number);
        assert_eq!(colour_of_text(&colours, &buffer, 9, "fld"), instruction, "the 8087's instructions");
    }

    #[test]
    fn both_x86_entries_are_listed_and_the_newer_one_reads_like_the_others() {
        let engine = test_engine();
        let names: Vec<&str> = engine.names().iter().map(|(name, _)| name.as_str()).collect();
        assert!(names.contains(&"Assembly x86_64"), "{names:?}");
        assert!(names.contains(&ASM_80X86));
        assert!(!names.contains(&"x86_64 Assembly") && !names.contains(&"Assembly (x86_64)"));
        let (name, syntax) = engine.names().iter().find(|(name, _)| name == "Assembly x86_64").unwrap();
        assert_eq!(engine.name_of(*syntax), name, "the status bar says the same as the menu");
        assert_eq!(engine.name_of(syntax_for("file.8088").unwrap()), ASM_80X86);
        assert_eq!(engine.name_of(syntax_for("file.a86").unwrap()), ASM_80X86);
    }

    #[test]
    fn dos_style_code_is_told_from_newer_x86_and_from_other_processors() {
        let dos = arcs(&[".model small", "start: mov ax,@data", "mov ds,ax", "mov ah,9", "int 21h"]);
        assert_eq!(guess_assembly(&dos), Some(ASM_80X86));
        let tasm = arcs(&["  mov ax,3", "  int 10h", "  xor bx,bx", "  push cx"]);
        assert_eq!(guess_assembly(&tasm), Some(ASM_80X86));
        let newer = arcs(&["  mov ax,1", "  mov rax,1", "  push bx", "  mov rdi,rsi"]);
        assert_eq!(guess_assembly(&newer), None, "64-bit registers leave it to the x86-64 grammar");
        let z80 = arcs(&["  push hl", "  add a,b", "  ld a,1", "  ld hl,de", "  jp start"]);
        assert_eq!(guess_assembly(&z80), Some(ASM_Z80), "Z80's PUSH and ADD aren't taken for x86");
        let left_edge = arcs(&["ld a,1", "ld b,2", "djnz loop"]);
        assert_eq!(guess_assembly(&left_edge), Some(ASM_Z80), "instructions at the left edge count");
        let engine = test_engine();
        let found = engine.detect_with_sample(Some(Path::new("hello.asm")), &dos).unwrap();
        assert_eq!(engine.name_of(found), ASM_80X86);
    }

    #[test]
    fn a_general_assembler_file_is_looked_into_to_see_which_processor_its_for() {
        let m68k = arcs(&["start  move.l d0,d1", "       lea table,a0", "       dbra d0,start", "       rts"]);
        let m6502 = arcs(&["start lda #1", "      sta $d020", "      ldx #0", "      rts"]);
        let z80 = arcs(&["start: ld a,1", "       ld (hl),a", "       djnz start", "       ret"]);
        assert_eq!(guess_assembly(&m68k), Some(ASM_680X0));
        assert_eq!(guess_assembly(&m6502), Some(ASM_6502));
        assert_eq!(guess_assembly(&z80), Some(ASM_Z80));
        assert_eq!(guess_assembly(&arcs(&["hello there", "this is text", "with no code"])), None);
        assert_eq!(guess_assembly(&arcs(&["  mov eax, 1", "  push ebx", "  int 80h"])), None, "x86 isn't mistaken for these");
        assert_eq!(guess_assembly(&arcs(&["  lda #1"])), None, "one line isn't enough to say");
        let engine = test_engine();
        let found = engine.detect_with_sample(Some(Path::new("game.asm")), &z80).unwrap();
        assert_eq!(engine.name_of(found), ASM_Z80);
        let found = engine.detect_with_sample(Some(Path::new("game.s")), &m68k).unwrap();
        assert_eq!(engine.name_of(found), ASM_680X0);
        let rust = engine.detect_with_sample(Some(Path::new("main.rs")), &z80).unwrap();
        assert_eq!(engine.name_of(rust), "Rust", "only general assembler endings are looked into");
    }

    #[test]
    fn brackets_in_assembler_strings_and_comments_dont_count() {
        let (colours, _) = assembler_colours("x.s68", &[" move.l (a0)+,d0 ; (((", " dc.b \"((\",0"]);
        assert_eq!(colours.line(0).unwrap().brackets.len(), 2);
        assert!(colours.line(1).unwrap().brackets.is_empty());
    }

    #[test]
    fn keywords_strings_and_comments_get_their_own_colours() {
        let mut buffer = buffer_of(&["fn main() {", "    let a = \"text\"; // note", "}"]);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        colour_all(&mut colours, &mut buffer);
        let keyword = colour_of_word(&colours, &buffer, 1, "let");
        let string = colour_of_word(&colours, &buffer, 1, "text");
        let comment = colour_of_word(&colours, &buffer, 1, "note");
        assert_ne!(keyword, string);
        assert_ne!(string, comment);
        assert_ne!(keyword, comment);
        let total: usize = colours.line(1).unwrap().spans.iter().map(|s| s.0).sum();
        assert_eq!(total, buffer.line(1).chars().count(), "every character is covered once");
    }

    #[test]
    fn brackets_in_strings_and_comments_dont_count() {
        let mut buffer = buffer_of(&["let a = \"((\"; // ((", "f(a)", "/* [", "still comment ] */", "g[0]"]);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        colour_all(&mut colours, &mut buffer);
        assert!(colours.line(0).unwrap().brackets.is_empty());
        let brackets: Vec<_> = colours.line(1).unwrap().brackets.iter().map(|b| (b.1, b.2)).collect();
        assert_eq!(brackets, [('(', Some(0)), (')', Some(0))], "nothing left open by the quoted ones");
        assert!(colours.line(2).unwrap().brackets.is_empty());
        assert!(colours.line(3).unwrap().brackets.is_empty(), "a comment carries across lines");
        assert_eq!(colours.line(4).unwrap().brackets.len(), 2);
    }

    #[test]
    fn an_edit_recolours_what_follows_it() {
        let mut buffer = buffer_of(&["let a = 1;", "let b = 2;", "let c = 3;"]);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        colour_all(&mut colours, &mut buffer);
        let plain = colour_of_word(&colours, &buffer, 2, "let");

        // Opening a comment on the first line turns everything after it into comment.
        buffer.move_caret(0, 0, false);
        buffer.type_text("/*");
        colour_all(&mut colours, &mut buffer);
        let commented = colour_of_word(&colours, &buffer, 2, "let");
        assert_ne!(plain, commented);

        buffer.undo();
        colour_all(&mut colours, &mut buffer);
        assert_eq!(colour_of_word(&colours, &buffer, 2, "let"), plain, "undo puts the colours back");
    }

    #[test]
    fn work_can_stop_and_carry_on() {
        let lines: Vec<String> = (0..3000).map(|i| format!("let value{i} = \"{i}\";")).collect();
        let mut buffer = TextBuffer::default();
        buffer.set_content(lines);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        let mut slices = 0;
        while !colours.prepare(test_engine(), &mut buffer, 2900, 2950, Duration::ZERO) {
            slices += 1;
        }
        assert!(slices > 100, "it stopped after each line: {slices}");
        assert!(colours.line(2949).is_some());
        assert!(colours.checkpoints.contains_key(&2816), "state was saved along the way");
    }

    #[test]
    fn a_far_jump_starts_near_the_view_and_saves_no_state() {
        let lines: Vec<String> = (0..20000).map(|i| format!("let value{i} = 1;")).collect();
        let mut buffer = TextBuffer::default();
        buffer.set_content(lines);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        let began = Instant::now();
        while !colours.prepare(test_engine(), &mut buffer, 19000, 19050, Duration::from_millis(50)) {}
        assert!(colours.line(19000).is_some());
        assert!(colours.checkpoints.is_empty(), "those colours aren't relied on as if read from the top");
        assert!(began.elapsed() < Duration::from_secs(20));
    }

    #[test]
    fn very_long_lines_stay_plain_and_are_safe() {
        let long = "x(".repeat(MAX_LINE_BYTES);
        let mut buffer = buffer_of(&[&long, "let a = 1;"]);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        colour_all(&mut colours, &mut buffer);
        assert!(colours.line(0).unwrap().spans.is_empty());
        assert!(!colours.line(1).unwrap().spans.is_empty());
    }

    #[test]
    fn turning_highlighting_off_forgets_the_colours() {
        let mut buffer = buffer_of(&["let a = 1;"]);
        let mut colours = LineColours::default();
        colours.set_syntax(syntax_for("main.rs"));
        colour_all(&mut colours, &mut buffer);
        assert!(colours.is_active() && colours.line(0).is_some());
        colours.set_syntax(None);
        assert!(!colours.is_active() && colours.line(0).is_none());
        assert!(colours.prepare(test_engine(), &mut buffer, 0, 1, Duration::ZERO));
    }

    #[test]
    fn without_a_language_rainbow_counts_every_bracket() {
        // The older way still applies to plain text, which has no strings or comments to skip.
        let buffer = buffer_of(&["\"(\"", "x"]);
        assert_eq!(RainbowCache::default().stack_at(&buffer, 1), [b'(']);
    }
}

#[cfg(test)]
mod embedded_tests {
    use super::*;
    use syntect::parsing::{ParseState, ScopeStack};

    /// Each stretch of some lines with the innermost language it's in: the last part of its innermost scope.
    fn languages(file: &str, lines: &[&str]) -> Vec<(String, String)> {
        let engine = Engine::load();
        let syntax = engine.detect(Some(Path::new(file)), lines[0]).unwrap();
        let mut parse = ParseState::new(&engine.syntaxes.syntaxes()[syntax]);
        let mut stack = ScopeStack::new();
        let mut out = Vec::new();
        for line in lines {
            let text = format!("{line}\n");
            let ops = parse.parse_line(&text, &engine.syntaxes).unwrap();
            for (piece, op) in syntect::easy::ScopeRegionIterator::new(&ops, &text) {
                stack.apply(op).unwrap();
                if let Some(top) = stack.as_slice().last().filter(|_| !piece.trim().is_empty()) {
                    let scope = top.build_string();
                    out.push((piece.trim().to_string(), scope.rsplit('.').next().unwrap_or("").to_string()));
                }
            }
        }
        out
    }

    fn language_of<'a>(found: &'a [(String, String)], piece: &str) -> &'a str {
        found.iter().find(|(text, _)| text == piece).map_or("", |(_, language)| language.as_str())
    }

    #[test]
    fn languages_inside_others_are_coloured_as_themselves() {
        let php = languages("page.php", &["<html><body>", "<?php echo strlen($name); ?><b>this is html</b><?php $x = 2; ?>", "<script>var a = 1;</script><style>p { color: red }</style>"]);
        assert_eq!(language_of(&php, "echo"), "php");
        assert_eq!(language_of(&php, "b"), "html", "the tag between ?> and <?php is HTML");
        assert_eq!(language_of(&php, "this is html"), "basic");
        assert_eq!(language_of(&php, "x"), "php", "and back to PHP after it");
        assert_eq!(language_of(&php, "var"), "js", "a script in the page is JavaScript");
        assert_eq!(language_of(&php, "color"), "css", "and a style is CSS");
        let markdown = languages("notes.md", &["# Title", "```rust", "fn main() { let x = 1; }", "```"]);
        assert_eq!(language_of(&markdown, "fn"), "rust", "a fenced block in Markdown is its own language");
    }
}
