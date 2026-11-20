//! SAS/C's Amiga programs, built unchanged with amiga-gcc or vbcc where they can be. SAS/C's own keywords are
//! mapped onto each compiler's own ways by a header Limoniter writes (as SDI_compiler.h does by hand); its
//! SCOPTIONS file is read into the compiler's flags; its smakefile (and slink's WITH file) says which files make
//! the program; and the few things that won't carry over are pointed out by their lines. The person's files are
//! never changed: each is compiled from a copy made to suit the compiler, its lines where they were.

use crate::compilers::{Job, Problem, Toolchain};
use std::path::{Path, PathBuf};

/// The header every SAS/C file is compiled with.
pub const HEADER_NAME: &str = "limoniter_sasc.h";

/// SAS/C's keywords in amiga-gcc's and vbcc's ways. amiga-gcc already knows __chip, __far, __near, __saveds,
/// __interrupt, __regargs and __stdargs (its m68kamigaos.h defines them); vbcc knows them too, but its __interrupt
/// returns with RTE, so SAS/C's (an exec interrupt server's) is its __amigainterrupt.
pub const HEADER: &str = "/* Limoniter's SAS/C compatibility: SAS/C's keywords in this compiler's own ways. */
#ifndef LIMONITER_SASC_H
#define LIMONITER_SASC_H
#if defined(__VBCC__)
/* register __d0 is vbcc's __reg(\"d0\"); __asm, which says a function takes its parameters in registers, goes. */
#define __asm
#define __d0 __reg(\"d0\")
#define __d1 __reg(\"d1\")
#define __d2 __reg(\"d2\")
#define __d3 __reg(\"d3\")
#define __d4 __reg(\"d4\")
#define __d5 __reg(\"d5\")
#define __d6 __reg(\"d6\")
#define __d7 __reg(\"d7\")
#define __a0 __reg(\"a0\")
#define __a1 __reg(\"a1\")
#define __a2 __reg(\"a2\")
#define __a3 __reg(\"a3\")
#define __a4 __reg(\"a4\")
#define __a5 __reg(\"a5\")
#define __a6 __reg(\"a6\")
#define __fp0 __reg(\"fp0\")
#define __fp1 __reg(\"fp1\")
#define __interrupt __amigainterrupt
#define __aligned
#define __stackext
#define __inline inline
#elif defined(__GNUC__)
/* register __d0 is moved after the parameter's name as Limoniter compiles, as amiga-gcc's __asm(\"d0\"). */
#define __aligned __attribute__((aligned(4)))
#endif
/* SAS/C's older name for __saveds. */
#define __geta4 __saveds
#endif
";

/// Whether C source is written for SAS/C: its register parameters, __asm functions or pragmas.
pub fn looks_like_sasc(text: &str) -> bool {
    text.lines().take(3000).any(|line| {
        let line = line.trim_start();
        line.starts_with("#pragma libcall") || line.starts_with("#pragma syscall") || line.contains("register __d") || line.contains("register __a") || line.contains(" __asm ") && line.contains('(') || line.contains("__saveds") && !line.starts_with("#define")
    })
}

/// SCOPTIONS, read into a compiler's flags: what it gives, the program's name if it names one, and the options
/// that have no equivalent, left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub flags: Vec<String>,
    pub name: Option<String>,
    pub left_out: Vec<String>,
}

/// SCOPTIONS' options: one or more to a line, each a word or WORD=value, "DEFINE NAME=value" taking the next word.
pub fn read_scoptions(text: &str, toolchain: Toolchain, folder: &Path) -> Options {
    let gcc = toolchain == Toolchain::AmigaGcc;
    let mut options = Options::default();
    let mut words: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("");
        let mut word = String::new();
        let mut quoted = false;
        for c in line.chars() {
            match c {
                '"' => quoted = !quoted,
                c if c.is_whitespace() && !quoted => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                c => word.push(c),
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    let mut at = 0;
    while at < words.len() {
        let word = words[at].clone();
        at += 1;
        let (key, value) = match word.split_once('=') {
            Some((k, v)) => (k.to_ascii_uppercase(), Some(v.to_string())),
            None => (word.to_ascii_uppercase(), None),
        };
        // DEFINE and IDIR take the next word when they aren't written with =.
        let mut next = |value: Option<String>| value.filter(|v| !v.is_empty()).or_else(|| {
            let taken = words.get(at).cloned();
            at += 1;
            taken
        });
        match key.as_str() {
            "DEFINE" | "DEF" => {
                if let Some(definition) = next(value) {
                    options.flags.push(format!("-D{definition}"));
                }
            }
            "INCLUDEDIR" | "IDIR" | "INCDIR" => {
                if let Some(dir) = next(value) {
                    // An Amiga assign (include:) has nothing here to stand for: the NDK's headers are used.
                    if dir.contains(':') && !(dir.len() > 2 && dir.as_bytes()[1] == b':') {
                        options.left_out.push(format!("{key}={dir}"));
                    } else {
                        options.flags.push(format!("-I{}", folder.join(dir).display()));
                    }
                }
            }
            "OPTIMIZE" | "OPT" | "OPTIMISE" => options.flags.push("-O2".to_string()),
            "DEBUG" | "DBG" => options.flags.push("-g".to_string()),
            "CPU" => match value.as_deref().map(str::trim).map(|v| v.trim_start_matches("680").trim_start_matches("68")) {
                Some(cpu @ ("20" | "30" | "40" | "60" | "000" | "010" | "0" | "10")) => {
                    let cpu = match cpu {
                        "000" | "0" => "68000",
                        "010" | "10" => "68010",
                        "20" => "68020",
                        "30" => "68030",
                        "40" => "68040",
                        _ => "68060",
                    };
                    options.flags.push(if gcc { format!("-m{cpu}") } else { format!("-cpu={cpu}") });
                }
                _ => options.left_out.push(word.clone()),
            },
            "MATH" => match value.as_deref().map(str::to_ascii_uppercase).as_deref() {
                Some("68881" | "68882" | "8") => options.flags.extend(if gcc { vec!["-m68881".to_string()] } else { vec!["-fpu=68881".to_string(), "-lm881".to_string()] }),
                Some("IEEE" | "I") if !gcc => options.flags.push("-lmieee".to_string()),
                Some("IEEE" | "I" | "STANDARD" | "S") => {}
                _ => options.left_out.push(word.clone()),
            },
            "PROGRAMNAME" | "PNAME" => options.name = value.or_else(|| next(None)),
            "UNSIGNEDCHARS" | "UCHAR" if gcc => options.flags.push("-funsigned-char".to_string()),
            // What only SAS/C's own way of building cares about, which makes no difference to the program here.
            "NOSTACKCHECK" | "NOSTKCHK" | "STACKCHECK" | "NOICONS" | "ICONS" | "VERBOSE" | "LINK" | "NOLINK" | "STRINGMERGE" | "STRMER" | "NOVERSION" | "ERRORREXX" | "NOERRORREXX" | "NOMINC" | "MULTIPLEINCLUDES" | "MCCONS" | "STREQ" => {}
            _ => options.left_out.push(word.clone()),
        }
    }
    options
}

/// What an smakefile says makes the program: its C files, the program's name, and what in it can't be followed
/// here (assembler sources, which SAS/C's asm assembled).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub sources: Vec<PathBuf>,
    pub name: Option<String>,
    pub notes: Vec<String>,
}

/// Reads a folder's smakefile: the first rule's slink line (or its WITH file, or an `sc link` line) gives the
/// objects and the program's name; each object's .c file beside it is a source. None without an smakefile, or
/// when nothing in it says what to build.
pub fn read_smakefile(folder: &Path) -> Option<Project> {
    let path = ["smakefile", "SMakefile", "SMAKEFILE"].iter().map(|n| folder.join(n)).find(|p| p.is_file())?;
    let text = std::fs::read_to_string(&path).ok()?;
    // Lines carried on with a backslash are one.
    let joined = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut variables: Vec<(String, String)> = Vec::new();
    let expand = |line: &str, variables: &[(String, String)]| {
        let mut out = line.to_string();
        for _ in 0..8 {
            let Some(start) = out.find("$(") else { break };
            let Some(end) = out[start..].find(')').map(|e| start + e) else { break };
            let name = out[start + 2..end].to_string();
            let value = variables.iter().rev().find(|(n, _)| *n == name).map_or(String::new(), |(_, v)| v.clone());
            out.replace_range(start..=end, &value);
        }
        out
    };
    let mut first_rule: Option<(String, String)> = None;
    let mut in_first = false;
    let mut link: Option<String> = None;
    for raw in joined.lines() {
        let line = raw.split('#').next().unwrap_or("");
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        if !indented {
            if let Some((name, value)) = line.split_once('=').filter(|(n, _)| !n.contains(':') && !n.trim().is_empty() && n.trim().chars().all(|c| c.is_alphanumeric() || c == '_')) {
                let value = expand(value.trim(), &variables);
                variables.push((name.trim().to_string(), value));
                continue;
            }
            if let Some((target, deps)) = line.split_once(':') {
                let target = target.trim();
                if first_rule.is_none() && !target.starts_with('.') && !target.is_empty() {
                    first_rule = Some((expand(target, &variables), expand(deps, &variables)));
                    in_first = true;
                } else {
                    in_first = false;
                }
            }
            continue;
        }
        let command = expand(line.trim(), &variables);
        let lower = command.to_ascii_lowercase();
        if in_first && link.is_none() && (lower.starts_with("slink") || lower.starts_with("sc ") && lower.split_whitespace().any(|w| w == "link")) {
            link = Some(command);
        }
    }
    let (target, deps) = first_rule?;
    let mut objects: Vec<String> = Vec::new();
    let mut name = None;
    match link {
        Some(command) => {
            let (found, to) = read_link(&command, folder);
            objects = found;
            name = to;
        }
        None => objects.extend(deps.split_whitespace().map(str::to_string)),
    }
    let mut project = Project { name: name.or(Some(target).filter(|t| !t.contains('.'))), ..Project::default() };
    for object in objects {
        // The start-up code and SAS/C's libraries (lib:c.o, lib:sc.lib) have their equivalents in each compiler.
        if object.contains(':') || object.to_ascii_lowercase().ends_with(".lib") {
            continue;
        }
        let stem = object.trim_end_matches(".o").trim_end_matches(".c");
        let c = folder.join(format!("{stem}.c"));
        if c.is_file() {
            if !project.sources.contains(&c) {
                project.sources.push(c);
            }
            continue;
        }
        if let Some(asm) = ["a", "asm", "s"].iter().map(|e| folder.join(format!("{stem}.{e}"))).find(|p| p.is_file()) {
            project.notes.push(crate::i18n::tf!("{} was assembled by SAS/C's asm, and isn't built here: assemble it with vasm (-Fhunk) and link it in, or rewrite it as C.", asm.display()));
        }
    }
    (!project.sources.is_empty()).then_some(project)
}

/// slink's (or `sc link`'s) objects and program name from its command line, following a WITH file.
fn read_link(command: &str, folder: &Path) -> (Vec<String>, Option<String>) {
    let mut words: Vec<String> = command.split_whitespace().skip(1).map(|w| w.trim_end_matches(',').to_string()).collect();
    let mut objects = Vec::new();
    let mut name = None;
    let mut expect: Option<&str> = None;
    let mut at = 0;
    while at < words.len() {
        let word = words[at].clone();
        at += 1;
        let upper = word.to_ascii_uppercase();
        match upper.as_str() {
            "FROM" | "ROOT" => expect = Some("from"),
            "TO" => expect = Some("to"),
            "LIB" | "LIBRARY" => expect = Some("lib"),
            "WITH" => expect = Some("with"),
            "MAP" | "XREF" => expect = Some("other"),
            "LINK" | "SC" | "SD" | "NOICONS" | "ADDSYM" | "STRIPDEBUG" | "ND" | "NODEBUG" | "VERBOSE" | "SMALLCODE" | "SMALLDATA" | "QUIET" | "BATCH" | "NOALVS" => expect = None,
            _ => match expect {
                Some("to") => {
                    name = Some(word.clone());
                    expect = None;
                }
                Some("lib" | "other") => {}
                Some("with") => {
                    // The WITH file's words are read as if they were here.
                    if let Ok(text) = std::fs::read_to_string(folder.join(&word)) {
                        let more: Vec<String> = text.split_whitespace().map(|w| w.trim_end_matches([',', '+']).to_string()).filter(|w| !w.is_empty()).collect();
                        words.splice(at..at, more);
                    }
                    expect = None;
                }
                _ => objects.push(word.clone()),
            },
        }
    }
    (objects, name)
}

/// SAS/C's register parameters as amiga-gcc writes them: `register __d0 LONG x` becomes `register LONG x
/// __asm__("d0")`, and a function's `__asm` (which only says it takes them) goes. A parameter with no name is
/// given one, which a prototype doesn't mind. Nothing moves to another line.
pub fn for_gcc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        out.push_str(&gcc_line(line));
    }
    out
}

fn gcc_line(line: &str) -> String {
    let mut line = line.to_string();
    // "LONG __asm Function(" -> "LONG Function(": a function's __asm, not an __asm("...") of gcc's own.
    let mut from = 0;
    while let Some(found) = line[from..].find("__asm").map(|f| from + f) {
        let after = &line[found + 5..];
        let before_ok = found == 0 || !line.as_bytes()[found - 1].is_ascii_alphanumeric() && line.as_bytes()[found - 1] != b'_';
        let next = after.trim_start();
        let statement = ["volatile", "__volatile__", "goto"].iter().any(|w| next.starts_with(w));
        if before_ok && !statement && after.starts_with([' ', '\t']) && next.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            line.replace_range(found..found + 5 + (after.len() - next.len()), "");
            from = found;
        } else {
            from = found + 5;
        }
    }
    // Each "register __xN" up to the end of its parameter.
    let registers = ["d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "a0", "a1", "a2", "a3", "a4", "a5", "a6", "fp0", "fp1", "fp2", "fp3", "fp4", "fp5", "fp6", "fp7"];
    let mut from = 0;
    while let Some(found) = line[from..].find("register").map(|f| from + f) {
        let rest = &line[found + 8..];
        let trimmed = rest.trim_start();
        let Some(register) = registers.iter().find(|r| trimmed.strip_prefix("__").is_some_and(|t| t.starts_with(*r) && !t[r.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))) else {
            from = found + 8;
            continue;
        };
        let start = found + 8 + (rest.len() - trimmed.len());
        let end = start + 2 + register.len();
        // The parameter ends at the comma or bracket at its own depth.
        let mut depth = 0i32;
        let mut stop = line.len();
        for (i, c) in line[end..].char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' if depth == 0 => {
                    stop = end + i;
                    break;
                }
                ')' | ']' => depth -= 1,
                ',' if depth == 0 => {
                    stop = end + i;
                    break;
                }
                _ => {}
            }
        }
        let parameter = line[end..stop].trim_end().to_string();
        let words: Vec<&str> = parameter.split(|c: char| c.is_whitespace() || c == '*').filter(|w| !w.is_empty()).collect();
        let named = words.len() >= 2 && words.last().is_some_and(|w| !matches!(*w, "int" | "long" | "short" | "char" | "void" | "unsigned" | "signed" | "const" | "volatile" | "float" | "double"));
        let name = if named { String::new() } else { format!(" __reg_{register}") };
        let replacement = format!("{}{name} __asm__(\"{register}\")", parameter);
        line.replace_range(start..stop, replacement.trim_start());
        from = start + replacement.trim_start().len();
    }
    line
}

/// What in SAS/C source won't carry over to amiga-gcc or vbcc, by line (from 1).
pub fn wont_carry_over(text: &str, toolchain: Toolchain) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let gcc = toolchain == Toolchain::AmigaGcc;
    for (n, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("").trim();
        let say = |words: String, found: &mut Vec<(usize, String)>| found.push((n + 1, words));
        if code.starts_with("#pragma") {
            let what = code.split_whitespace().nth(1).unwrap_or("");
            match what {
                "libcall" | "syscall" | "tagcall" | "flibcall" => say(crate::i18n::tf!("#pragma {what} is SAS/C's way of calling a library: include the library's <proto/...> header instead, which {} understands.", if gcc { "amiga-gcc" } else { "vbcc" }), &mut found),
                "msg" | "header" | "options" => say(crate::i18n::tf!("#pragma {what} is SAS/C's own, and is left out here."), &mut found),
                _ => {}
            }
        }
        if code.contains("__emit(") {
            say(crate::i18n::tr("__emit puts words into the code only SAS/C understands: write it as inline assembler.").into(), &mut found);
        }
        if ["getreg(", "putreg(", "geta4("].iter().any(|f| code.contains(f)) {
            say(crate::i18n::tr("getreg, putreg and geta4 read and set registers in SAS/C only.").into(), &mut found);
        }
        if code.starts_with("#include") && code.contains("<dos.h>") {
            say(crate::i18n::tr("<dos.h> is SAS/C's own header (getreg, putreg and the like); the others don't have it.").into(), &mut found);
        }
        if code.contains("_STI") || code.contains("_STD") {
            let starts = code.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w.starts_with("_STI") || w.starts_with("_STD"));
            if starts && code.contains('(') {
                say(if gcc { crate::i18n::tr("SAS/C calls functions named _STI... as the program starts and _STD... as it ends; amiga-gcc doesn't: mark them __attribute__((constructor)) and ((destructor)).").into() } else { crate::i18n::tr("SAS/C calls functions named _STI... as the program starts and _STD... as it ends; vbcc's are named _INIT_ and _EXIT_.").into() }, &mut found);
            }
        }
        if code.contains("__oslibversion") {
            say(crate::i18n::tr("__oslibversion sets the version of the libraries SAS/C's start-up opens; check which version the libraries are opened at here.").into(), &mut found);
        }
    }
    found
}

/// Copies a job's SAS/C files to the work folder, made to suit its compiler: each starts with the compatibility
/// header and says, with #line, which file and line it really is, so problems are found in the person's own files.
/// The project's own headers are copied the same way, and found first. The sources, the notes on what won't carry
/// over, and the folders to look for headers in.
pub fn prepare(job: &Job, work: &Path) -> Result<(Vec<PathBuf>, Vec<Problem>, Vec<PathBuf>), String> {
    let copies = work.join("sasc");
    std::fs::create_dir_all(&copies).map_err(|e| e.to_string())?;
    std::fs::write(copies.join(HEADER_NAME), HEADER).map_err(|e| e.to_string())?;
    let gcc = job.compiler.toolchain == Toolchain::AmigaGcc;
    let mut notes = Vec::new();
    let copy = |path: &Path, notes: &mut Vec<Problem>| -> Result<PathBuf, String> {
        let text = std::fs::read_to_string(path).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", path.display()))?;
        for (line, message) in wont_carry_over(&text, job.compiler.toolchain) {
            notes.push(Problem { file: path.display().to_string(), line, message, warning: true });
        }
        let body = if gcc { for_gcc(&text) } else { text };
        let name = path.file_name().unwrap_or_default();
        let target = copies.join(name);
        let quoted = path.display().to_string().replace('\\', "\\\\");
        std::fs::write(&target, format!("#include \"{HEADER_NAME}\"\n#line 1 \"{quoted}\"\n{body}")).map_err(|e| e.to_string())?;
        Ok(target)
    };
    let mut sources = Vec::new();
    for source in &job.sources {
        sources.push(copy(source, &mut notes)?);
    }
    let mut folders = vec![copies.clone()];
    for folder in job.sources.iter().filter_map(|s| s.parent()) {
        if !folders.iter().any(|f| f == folder) {
            if let Ok(entries) = std::fs::read_dir(folder) {
                for header in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("h"))) {
                    copy(&header, &mut notes)?;
                }
            }
            folders.push(folder.to_path_buf());
        }
    }
    Ok((sources, notes, folders))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sasc_source_is_known_by_its_own_ways() {
        assert!(looks_like_sasc("LONG __asm Add(register __d0 LONG a, register __d1 LONG b)\n"));
        assert!(looks_like_sasc("#pragma libcall MyBase MyFunc 1e 0802\n"));
        assert!(looks_like_sasc("void __saveds Handler(void)\n"));
        assert!(!looks_like_sasc("#include <stdio.h>\nint main(void) { return 0; }\n"));
        assert!(!looks_like_sasc("#define SAVEDS __saveds\n"), "a header that maps it isn't SAS/C's");
    }

    #[test]
    fn the_header_maps_each_keyword_for_vbcc_and_leaves_gccs_own() {
        assert!(HEADER.contains("#define __d0 __reg(\"d0\")") && HEADER.contains("#define __a6 __reg(\"a6\")"));
        assert!(HEADER.contains("#define __interrupt __amigainterrupt"));
        assert!(HEADER.contains("#define __geta4 __saveds"));
        let gcc_part = HEADER.split("#elif defined(__GNUC__)").nth(1).unwrap().split("#endif").next().unwrap();
        assert!(!gcc_part.contains("__chip") && !gcc_part.contains("__saveds"), "amiga-gcc defines those itself");
    }

    #[test]
    fn register_parameters_are_moved_after_their_names_for_gcc() {
        assert_eq!(gcc_line("LONG __asm Add(register __d0 LONG a, register __d1 LONG b)"), "LONG Add(register LONG a __asm__(\"d0\"), register LONG b __asm__(\"d1\"))");
        assert_eq!(gcc_line("void __asm __saveds Hook(register __a0 struct Hook *h, register __a2 APTR o, register __a1 struct Msg *m)"), "void __saveds Hook(register struct Hook *h __asm__(\"a0\"), register APTR o __asm__(\"a2\"), register struct Msg *m __asm__(\"a1\"))");
        assert_eq!(gcc_line("LONG __asm Proto(register __d0 LONG);"), "LONG Proto(register LONG __reg_d0 __asm__(\"d0\"));", "a parameter without a name gets one");
        assert_eq!(gcc_line("    register int count = 0;"), "    register int count = 0;", "an ordinary register variable stays");
        assert_eq!(gcc_line("    __asm(\"nop\");"), "    __asm(\"nop\");", "gcc's own __asm stays");
        assert_eq!(for_gcc("a\nLONG __asm F(register __d0 LONG x)\nb").lines().count(), 3, "no line moves");
    }

    #[test]
    fn scoptions_become_each_compilers_flags() {
        let text = "DATA=FAR\nCODE=FAR\nOPTIMIZE\nNOSTACKCHECK\nDEFINE DEBUG=1\nDEFINE=VERSION=\"2\"\nCPU=68020 MATH=68881\nINCLUDEDIR=include\nIDIR=include:\nPROGRAMNAME=MyDemo\nIGNORE=73\n";
        let folder = Path::new("/p");
        let gcc = read_scoptions(text, Toolchain::AmigaGcc, folder);
        assert_eq!(gcc.flags, ["-O2", "-DDEBUG=1", "-DVERSION=2", "-m68020", "-m68881", &format!("-I{}", folder.join("include").display())]);
        assert_eq!(gcc.name.as_deref(), Some("MyDemo"));
        assert_eq!(gcc.left_out, ["DATA=FAR", "CODE=FAR", "IDIR=include:", "IGNORE=73"]);
        let vbcc = read_scoptions(text, Toolchain::Vbcc, folder);
        assert!(vbcc.flags.contains(&"-cpu=68020".to_string()) && vbcc.flags.contains(&"-fpu=68881".to_string()) && vbcc.flags.contains(&"-lm881".to_string()));
    }

    #[test]
    fn an_smakefile_and_slinks_with_file_say_what_to_build() {
        let base = std::env::temp_dir().join(format!("limoniter-smake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        for name in ["main.c", "gfx.c", "sound.c", "copper.a"] {
            std::fs::write(base.join(name), "").unwrap();
        }
        std::fs::write(base.join("smakefile"), "# A demo\nOBJS = main.o gfx.o \\\n  copper.o\n\nDemo: $(OBJS)\n\tslink lib:c.o $(OBJS) TO Demo LIB lib:sc.lib lib:amiga.lib SC SD\n\n.c.o:\n\tsc $*.c\n").unwrap();
        let project = read_smakefile(&base).unwrap();
        assert_eq!(project.sources, [base.join("main.c"), base.join("gfx.c")]);
        assert_eq!(project.name.as_deref(), Some("Demo"));
        assert_eq!(project.notes.len(), 1, "copper.a is pointed out");
        std::fs::write(base.join("smakefile"), "all: game\n\tslink WITH game.lnk\n").unwrap();
        std::fs::write(base.join("game.lnk"), "FROM lib:c.o+\nmain.o sound.o\nTO Game\nLIB lib:sc.lib\n").unwrap();
        let project = read_smakefile(&base).unwrap();
        assert_eq!(project.sources, [base.join("main.c"), base.join("sound.c")]);
        assert_eq!(project.name.as_deref(), Some("Game"));
        std::fs::write(base.join("smakefile"), "Tool: tool.o\n\tsc link main.c gfx.c to Tool\n").unwrap();
        let project = read_smakefile(&base).unwrap();
        assert_eq!(project.sources, [base.join("main.c"), base.join("gfx.c")]);
        assert_eq!(project.name.as_deref(), Some("Tool"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn what_wont_carry_over_is_pointed_out_by_line() {
        let text = "#include <dos.h>\n#pragma libcall MyBase MyFunc 1e 0802\nvoid _STIinit(void) { }\nint x = getreg(REG_A4);\n__emit(0x4e71);\nint ok;\n";
        let lines: Vec<usize> = wont_carry_over(text, Toolchain::AmigaGcc).iter().map(|(l, _)| *l).collect();
        assert_eq!(lines, [1, 2, 3, 4, 5]);
        assert!(wont_carry_over(text, Toolchain::Vbcc)[2].1.contains("_INIT_"));
    }

    #[test]
    fn copies_keep_their_lines_and_say_where_they_came_from() {
        let base = std::env::temp_dir().join(format!("limoniter-sasc-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("hook.c");
        std::fs::write(&source, "#include \"hook.h\"\nLONG __asm F(register __d0 LONG x) { return x; }\n").unwrap();
        std::fs::write(base.join("hook.h"), "LONG __asm F(register __d0 LONG);\n").unwrap();
        let job = Job { compiler: crate::compilers::by_id("amiga-gcc").unwrap(), program: PathBuf::from("gcc"), machine: crate::running::Machine::Amiga, main: source.clone(), sources: vec![source.clone()], folder: base.clone(), flags: Vec::new(), stem: "hook".into(), sasc: true };
        let work = base.join("work");
        let (sources, _, folders) = prepare(&job, &work).unwrap();
        let copy = std::fs::read_to_string(&sources[0]).unwrap();
        let lines: Vec<&str> = copy.lines().collect();
        assert_eq!(lines[0], "#include \"limoniter_sasc.h\"");
        assert!(lines[1].starts_with("#line 1 \"") && lines[1].contains("hook.c"));
        assert_eq!(lines[3], "LONG F(register LONG x __asm__(\"d0\")) { return x; }");
        assert!(std::fs::read_to_string(work.join("sasc/hook.h")).unwrap().contains("__asm__(\"d0\")"), "the project's header too");
        assert_eq!(folders, [work.join("sasc"), base.clone()]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
