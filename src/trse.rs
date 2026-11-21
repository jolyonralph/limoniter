//! TRSE, Turbo Rascal Syntax Error: Turbo Rascal source (`.ras`, and units in `.tru`) built as TRSE builds a
//! project. TRSE's own command line (`trse -cli op=project project=<the .trse file> input_file=<the .ras file>`,
//! as its `-cli` help gives it) is run in the project's folder, and it leaves the program beside the source, named
//! for it: a .prg for the Commodores, .bin for the Atari 2600, .vec for the Vectrex, .nes and .gb for the NES and
//! Game Boy. The machine is the one the project's `system =` line says.
//!
//! Its built-ins (from TRSE's resources/text/syntax.txt) are offered as code is typed, and described on hover with
//! their parameters and the systems that have them.

use crate::running::Machine;
use std::path::{Path, PathBuf};

const BUILTINS: &str = include_str!("../data/trse-builtins.txt");

/// File, New's starter, for a C64 project: save it in a TRSE project's folder, and F9 builds it.
pub const STARTER: &str = "program Hello;
// A Turbo Rascal program. Save it in a TRSE project's folder (one with a .trse file for the C64), and F9 builds it
// with TRSE and runs it.
var
   i : byte;
begin
   ClearScreen(key_space, screen_char_loc);
   MoveTo(10, 12, hi(screen_char_loc));
   PrintString(\"HELLO FROM LIMONITER\", 0, 20);
   while (true) do
   begin
      screen_bg_col := i;
      i := i + 1;
   end;
end.
";

/// Turbo Rascal's reserved words, as TRSE's syntax file lists them.
const RESERVED: [&str; 62] = [
    "program", "var", "begin", "end", "procedure", "function", "interrupt", "if", "then", "else", "for", "fori", "to", "do", "while", "repeat", "until", "step",
    "array", "of", "byte", "integer", "long", "boolean", "string", "cstring", "pointer", "ppointer", "lpointer", "incbin", "incsid", "incnsf", "asm", "at",
    "absolute", "and", "or", "xor", "not", "shl", "shr", "global", "const", "unit", "use", "include", "macro", "endmacro", "break", "continue", "return",
    "case", "record", "class", "offpage", "onpage", "startblock", "endblock", "forward", "public", "private", "address",
];

/// Every built-in: its name as TRSE spells it, the systems that have it, and its parameters' letters.
fn builtins() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    BUILTINS.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()).filter_map(|line| {
        let mut parts = line.split(';');
        Some((parts.next()?, parts.next().unwrap_or(""), parts.next().unwrap_or("")))
    })
}

/// The words offered as Turbo Rascal is typed: its reserved words and its built-ins.
pub fn keywords() -> Vec<String> {
    let mut words: Vec<String> = RESERVED.iter().map(|w| w.to_string()).chain(builtins().map(|(name, ..)| name.to_string())).collect();
    words.sort_by_key(|w| w.to_lowercase());
    words.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    words
}

/// A parameter's letter in TRSE's syntax file, in words.
fn parameter(code: &str) -> String {
    match code {
        "b" => crate::i18n::tr("byte").into(),
        "i" => crate::i18n::tr("integer").into(),
        "a" => crate::i18n::tr("address").into(),
        "s" => crate::i18n::tr("string").into(),
        "n" => crate::i18n::tr("number").into(),
        "p" => crate::i18n::tr("procedure").into(),
        "l" => crate::i18n::tr("long").into(),
        "ib" | "bi" => crate::i18n::tr("integer or byte").into(),
        "ai" | "ia" => crate::i18n::tr("address or integer").into(),
        other => other.to_string(),
    }
}

/// What a built-in is, for hover: how it's called, with its parameters, and which systems have it.
pub fn describe(word: &str) -> Option<String> {
    let (name, systems, parameters) = builtins().find(|(name, ..)| name.eq_ignore_ascii_case(word))?;
    let shown: Vec<String> = parameters.split(',').filter(|p| !p.is_empty()).map(parameter).collect();
    let systems = if systems.eq_ignore_ascii_case("ALL") { crate::i18n::tr("every system").to_string() } else { systems.replace(',', ", ") };
    Some(crate::i18n::tf!("{name}({}): one of TRSE's built-ins, on {systems}", shown.join(", ")))
}

/// Whether a column is in a comment (after //, or inside /* */ or { } begun on the line) or a string.
pub fn in_comment_or_string(line: &str, col: usize) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut quote: Option<char> = None;
    let mut block: Option<&str> = None;
    let mut i = 0;
    while i < col.min(chars.len()) {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match (quote, block) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, Some("*/")) if c == '*' && next == Some('/') => {
                block = None;
                i += 1;
            }
            (None, Some("}")) if c == '}' => block = None,
            (None, Some(_)) => {}
            (None, None) if c == '/' && next == Some('/') => return true,
            (None, None) if c == '/' && next == Some('*') => {
                block = Some("*/");
                i += 1;
            }
            (None, None) if c == '{' => block = Some("}"),
            (None, None) if c == '"' || c == '\'' => quote = Some(c),
            (None, None) => {}
        }
        i += 1;
    }
    quote.is_some() || block.is_some()
}

/// The machine a TRSE system is, of those whose programs Limoniter runs.
pub fn machine_of(system: &str) -> Option<Machine> {
    Some(match system.trim().to_ascii_uppercase().as_str() {
        "C64" => Machine::C64,
        "C128" => Machine::C128,
        "VIC20" => Machine::Vic20,
        "PLUS4" => Machine::Plus4,
        "PET" => Machine::Pet,
        "ATARI2600" => Machine::Atari2600,
        "VECTREX" => Machine::Vectrex,
        "NES" => Machine::Nes,
        "GAMEBOY" => Machine::GameBoy,
        _ => return None,
    })
}

/// The ending TRSE gives the program it makes for a machine.
pub fn ending(machine: Machine) -> &'static str {
    match machine {
        Machine::Atari2600 => "bin",
        Machine::Vectrex => "vec",
        Machine::Nes => "nes",
        Machine::GameBoy => "gb",
        _ => "prg",
    }
}

/// The project a source file is in: the first .trse file in its folder or the two above it, and the system its
/// `system =` line names.
pub fn project_of(source: &Path) -> Option<(PathBuf, String)> {
    let mut folder = source.parent();
    for _ in 0..3 {
        let here = folder?;
        let mut projects: Vec<PathBuf> = std::fs::read_dir(here).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("trse"))).collect();
        projects.sort();
        if let Some(project) = projects.into_iter().next() {
            let text = std::fs::read_to_string(&project).unwrap_or_default();
            return Some((project, system_in(&text).unwrap_or_default()));
        }
        folder = here.parent();
    }
    None
}

/// The system a project's settings name.
pub fn system_in(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim().eq_ignore_ascii_case("system")).then(|| value.trim().to_string())
    })
}

/// The command line that builds a source file in a project.
pub fn arguments(project: &Path, source: &Path) -> Vec<String> {
    vec!["-cli".into(), "op=project".into(), format!("project={}", project.display()), format!("input_file={}", source.display())]
}

/// A problem as TRSE prints one: a fatal error's line ("Fatal error  on line: 12"), or a warning ("Warning at line
/// 7 in file 'unit.tru': Unused variables : x"). The file is left empty when TRSE doesn't say, for the source's.
pub fn problem_in(line: &str) -> Option<crate::compilers::Problem> {
    let text = line.trim();
    if let Some(at) = text.find("Fatal error") {
        let rest = &text[at..];
        let number = rest.split("on line:").nth(1)?.split_whitespace().next()?.parse().ok()?;
        let before = text[..at].trim();
        let message = if before.is_empty() { crate::i18n::tr("TRSE stopped here with a fatal error.").to_string() } else { before.to_string() };
        return Some(crate::compilers::Problem { file: String::new(), line: number, message, warning: false });
    }
    let rest = text.strip_prefix("Warning at line ")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let number = digits.parse().ok()?;
    let rest = &rest[digits.len()..];
    let (file, message) = match rest.strip_prefix(" in file '").and_then(|r| r.split_once("':")) {
        Some((file, message)) => (file.to_string(), message),
        None => (String::new(), rest.strip_prefix(':').unwrap_or(rest)),
    };
    Some(crate::compilers::Problem { file, line: number, message: message.trim().to_string(), warning: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_built_ins_are_offered_and_described() {
        let words = keywords();
        assert!(words.iter().any(|w| w == "PlaySound") && words.iter().any(|w| w == "begin"));
        assert_eq!(words.iter().filter(|w| w.eq_ignore_ascii_case("moveto")).count(), 1, "each once");
        let said = describe("playsound").unwrap();
        assert!(said.starts_with("PlaySound(byte, byte, byte, byte, byte, byte, byte)") && said.contains("C64, MEGA65"), "{said}");
        assert!(describe("Poke").unwrap().contains("(address, byte, byte)"));
        assert!(describe("nothing_of_the_kind").is_none());
    }

    #[test]
    fn comments_and_strings_are_told_from_code() {
        assert!(in_comment_or_string("x := 1; // a comment", 15));
        assert!(!in_comment_or_string("x := 1; // a comment", 3));
        assert!(in_comment_or_string("s := \"it's here\";", 8));
        assert!(in_comment_or_string("a /* here */ b", 5) && !in_comment_or_string("a /* here */ b", 13));
        assert!(in_comment_or_string("{ Pascal's } x", 4) && !in_comment_or_string("{ Pascal's } x", 13));
    }

    #[test]
    fn the_project_says_the_machine_and_the_command_line_builds_it() {
        let dir = std::env::temp_dir().join(format!("limoniter-trse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("game.trse"), "zeropages = ,$02\nsystem = VIC20\nmain_ras_file = main.ras\n").unwrap();
        let source = dir.join("src").join("main.ras");
        std::fs::write(&source, "program Main;\nbegin\nend.\n").unwrap();
        let (project, system) = project_of(&source).unwrap();
        assert_eq!((project.file_name().unwrap().to_str().unwrap(), system.as_str()), ("game.trse", "VIC20"));
        assert_eq!(machine_of(&system), Some(Machine::Vic20));
        assert_eq!(machine_of("ATARI2600").map(ending), Some("bin"));
        assert_eq!(machine_of("AMIGA"), None, "not one Limoniter runs TRSE's programs on yet");
        let args = arguments(&project, &source);
        assert_eq!(args[..2], ["-cli", "op=project"]);
        assert!(args[2].starts_with("project=") && args[2].ends_with("game.trse") && args[3].starts_with("input_file=") && args[3].ends_with("main.ras"));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(project_of(Path::new("/nowhere/at/all/x.ras")), None);
    }

    #[test]
    fn its_problems_are_read_by_line() {
        let fatal = problem_in("Unknown identifier 'foo' Fatal error  on line: 12").unwrap();
        assert_eq!((fatal.line, fatal.message.as_str(), fatal.warning), (12, "Unknown identifier 'foo'", false));
        assert_eq!(problem_in("Fatal error  on line: 3").unwrap().line, 3);
        let warning = problem_in("Warning at line 7 in file 'unit.tru': Unused variables : x").unwrap();
        assert_eq!((warning.file.as_str(), warning.line, warning.message.as_str(), warning.warning), ("unit.tru", 7, "Unused variables : x", true));
        assert_eq!(problem_in("Warning at line 9: something").unwrap().message, "something");
        assert!(problem_in("Compiling 'main.ras' for system: C64").is_none());
    }
}
