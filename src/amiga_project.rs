//! Amiga C projects made and built as an Amiga programmer expects: a folder with the program's source, its
//! makefiles for Bebbo's amiga-gcc (Makefile.gcc), vbcc (Makefile.vbcc) and SAS/C on a real Amiga (smakefile), an
//! icon for a Workbench program, and for a multilingual one its catalog description and translations.
//!
//! The makefiles are the person's, but the lines between `# limoniter-begin` and `# limoniter-end` are
//! Limoniter's: it writes them again from the folder's .c and .h files whenever they change, with the maths
//! library when floating point is used, and with -lamiga and (for vbcc) -lauto for Workbench programs. A
//! `# limoniter:` line among them says what the program is for (its AmigaOS, whether it's for the Workbench, its
//! processor), so they can be written again from the files alone.
//!
//! How each compiler is run follows its documentation: amiga-gcc's README (-noixemul for libnix, -mcrt=nix13 for
//! Kickstart 1.3, -lm for its maths), vbcc's (vc +aos68k, or +kick13 for 1.3; -lamiga, -lauto to open libraries
//! by themselves, -lmieee for IEEE maths; -cpu=68020), and SAS/C 6's (sc with MATH=IEEE, CPU=68020; slink with
//! LIB:c.o first and LIB:scmieee.lib, LIB:sc.lib and LIB:amiga.lib after).
//!
//! Importing a project sorts a folder of Amiga C, SAS/C's files and icons among it, into what each file is.

use std::path::{Path, PathBuf};

/// The AmigaOS a program is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// Kickstart 1.2 and 1.3, with what the 1.3 NDK has.
    Kick13,
    /// AmigaOS 3.0 to 3.2, with the 3.2 NDK.
    Os3,
}

impl Os {
    fn id(self) -> &'static str {
        match self {
            Os::Kick13 => "1.3",
            Os::Os3 => "3",
        }
    }
}

/// What a project's program is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Shell,
    Workbench,
    ReAction,
    Mui,
    Multilingual,
}

/// A project to start from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Template {
    pub name: &'static str,
    pub about: &'static str,
    pub os: Os,
    pub kind: Kind,
}

pub const TEMPLATES: [Template; 7] = [
    Template { name: "Shell program for AmigaOS 1.3", about: "A program run from the Shell (the CLI), for Kickstart 1.2 and 1.3: it uses only what dos.library had then, so it writes with Write() and takes its arguments from argv.", os: Os::Kick13, kind: Kind::Shell },
    Template { name: "Workbench program for AmigaOS 1.3", about: "A program started from its icon, for Kickstart 1.2 and 1.3: it opens intuition.library and graphics.library itself, opens a window as 1.3 did, with a NewWindow, and waits for it to be closed.", os: Os::Kick13, kind: Kind::Workbench },
    Template { name: "Shell program for AmigaOS 3", about: "A program run from the Shell, for AmigaOS 3.0 to 3.2: it reads its arguments with ReadArgs() and a template, as the system's own commands do, and has a version string.", os: Os::Os3, kind: Kind::Shell },
    Template { name: "Workbench program for AmigaOS 3", about: "A program started from its icon, for AmigaOS 3.0 to 3.2: a window opened with OpenWindowTags(), its libraries opened for it by the compiler's start-up code.", os: Os::Os3, kind: Kind::Workbench },
    Template { name: "ReAction program for AmigaOS 3.2", about: "A window laid out by ReAction's classes (window.class, layout.gadget, button.gadget and label.image), as the 3.2 NDK's examples have it.", os: Os::Os3, kind: Kind::ReAction },
    Template { name: "MUI 5 program", about: "A window of MUI's objects: an application, its window, a text and a button, with notifications. It needs MUI's developer files (its SDK) to build, and a 68020 or better.", os: Os::Os3, kind: Kind::Mui },
    Template { name: "Multilingual program with its catalog", about: "A Shell program whose words come from locale.library: its strings described in a .cd, translated into German and Italian in .ct files, their catalogs and the strings' C header made by Limoniter.", os: Os::Os3, kind: Kind::Multilingual },
];

/// The processor a program is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    M68000,
    M68020,
}

/// What Limoniter keeps up to date in a project's makefiles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub program: String,
    pub os: Os,
    pub workbench: bool,
    pub cpu: Cpu,
    /// The C files, relative to the project's folder, with / between folders.
    pub sources: Vec<String>,
    pub headers: Vec<String>,
    /// Whether floating point is used, so the maths library is linked.
    pub maths: bool,
    /// The catalog description and its translations, with each one's language, for the catalogs' rules.
    pub catalog: Option<(String, Vec<(String, String)>)>,
}

pub const MAKEFILE_GCC: &str = "Makefile.gcc";
pub const MAKEFILE_VBCC: &str = "Makefile.vbcc";
pub const SMAKEFILE: &str = "smakefile";
const BEGIN: &str = "# limoniter-begin";
const END: &str = "# limoniter-end";
const SETTINGS: &str = "# limoniter:";

/// A program's name as a file and a project can have it: letters, digits, - and _, short enough for its
/// catalog's header name (hello_strings.h) to fit the Amiga's 30 characters.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some(crate::i18n::tr("Give the program a name.").into());
    }
    if name.len() > 20 {
        return Some(crate::i18n::tr("The name can be no more than 20 characters, so that its files' names fit the Amiga's 30.").into());
    }
    if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Some(crate::i18n::tr("The name can have letters, digits, - and _, starting with a letter.").into());
    }
    None
}

// ---------- The makefiles ----------

fn settings_line(build: &Build) -> String {
    let cpu = if build.cpu == Cpu::M68020 { "68020" } else { "68000" };
    format!("{SETTINGS} os={} workbench={} cpu={cpu}", build.os.id(), if build.workbench { "yes" } else { "no" })
}

/// What a `# limoniter:` line says: the AmigaOS, whether it's for the Workbench, and the processor.
fn read_settings(line: &str) -> Option<(Os, bool, Cpu)> {
    let rest = line.trim().strip_prefix(SETTINGS)?;
    let (mut os, mut workbench, mut cpu) = (Os::Os3, false, Cpu::M68000);
    for word in rest.split_whitespace() {
        match word.split_once('=') {
            Some(("os", value)) => os = if value.starts_with('1') { Os::Kick13 } else { Os::Os3 },
            Some(("workbench", value)) => workbench = value == "yes",
            Some(("cpu", value)) => cpu = if value == "68000" { Cpu::M68000 } else { Cpu::M68020 },
            _ => {}
        }
    }
    Some((os, workbench, cpu))
}

/// The object a source makes, beside it.
fn object(source: &str) -> String {
    format!("{}.o", source.strip_suffix(".c").unwrap_or(source))
}

/// The libraries each compiler links, in the order they go after the objects.
pub fn gcc_libs(build: &Build) -> Vec<String> {
    let mut libs = Vec::new();
    if build.maths {
        libs.push("-lm".to_string());
    }
    libs.push("-lamiga".into());
    libs
}

pub fn vbcc_libs(build: &Build) -> Vec<String> {
    let mut libs = Vec::new();
    if build.maths {
        libs.push("-lmieee".to_string());
    }
    // vbcc's auto.lib opens the libraries a program uses but doesn't open itself; the 1.3 programs open theirs.
    if build.workbench && build.os == Os::Os3 {
        libs.push("-lauto".into());
    }
    libs.push("-lamiga".into());
    libs
}

/// What's given to amiga-gcc for this build, besides the files: its C library and processor.
pub fn gcc_flags(build: &Build) -> Vec<String> {
    let crt = if build.os == Os::Kick13 { "-mcrt=nix13" } else { "-noixemul" };
    let cpu = if build.cpu == Cpu::M68020 { "-m68020" } else { "-m68000" };
    vec![crt.into(), cpu.into()]
}

/// What's given to vc: its configuration (the target) and processor.
pub fn vbcc_flags(build: &Build) -> Vec<String> {
    let config = if build.os == Os::Kick13 { "+kick13" } else { "+aos68k" };
    let cpu = if build.cpu == Cpu::M68020 { "-cpu=68020" } else { "-cpu=68000" };
    vec![config.into(), cpu.into()]
}

/// The rules that make each catalog with FlexCat, for the makefiles run on this computer.
fn flexcat_rules(build: &Build) -> String {
    let Some((cd, translations)) = &build.catalog else { return String::new() };
    let stem = cd.strip_suffix(".cd").unwrap_or(cd);
    let stem = stem.rsplit('/').next().unwrap_or(stem);
    let targets: Vec<String> = translations.iter().map(|(_, language)| format!("Catalogs/{language}/{stem}.catalog")).collect();
    let mut out = format!("CATALOGS = {}\n\ncatalogs: $(CATALOGS)\n", targets.join(" "));
    for ((ct, _), target) in translations.iter().zip(&targets) {
        out.push_str(&format!("\n{target}: {cd} {ct}\n\tmkdir -p $(dir $@)\n\tflexcat {cd} {ct} CATALOG $@\n"));
    }
    out
}

fn kept_gcc(build: &Build) -> String {
    let mut out = format!("{BEGIN} ({})\n{}\n", crate::i18n::tr("Limoniter keeps the lines from here to limoniter-end up to date as files are added and taken away."), settings_line(build));
    out.push_str(&format!("PROGRAM = {}\nSOURCES = {}\nHEADERS = {}\n", build.program, build.sources.join(" "), build.headers.join(" ")));
    out.push_str(&format!("TARGET = {}\nLIBS = {}\n", gcc_flags(build).join(" "), gcc_libs(build).join(" ")));
    out.push_str(&flexcat_rules(build));
    out.push_str(END);
    out.push('\n');
    out
}

fn kept_vbcc(build: &Build) -> String {
    let mut out = format!("{BEGIN} ({})\n{}\n", crate::i18n::tr("Limoniter keeps the lines from here to limoniter-end up to date as files are added and taken away."), settings_line(build));
    out.push_str(&format!("PROGRAM = {}\nSOURCES = {}\nHEADERS = {}\n", build.program, build.sources.join(" "), build.headers.join(" ")));
    out.push_str(&format!("TARGET = {}\nLIBS = {}\n", vbcc_flags(build).join(" "), vbcc_libs(build).join(" ")));
    out.push_str(&flexcat_rules(build));
    out.push_str(END);
    out.push('\n');
    out
}

/// The smakefile's kept part: its objects, the link, and a rule for each object (smake, as SAS/C's own
/// smakefiles do, names each), and CatComp's rules for the catalogs.
fn kept_smake(build: &Build) -> String {
    let mut out = format!("{BEGIN} ({})\n{}\n", crate::i18n::tr("Limoniter keeps the lines from here to limoniter-end up to date as files are added and taken away."), settings_line(build));
    let objects: Vec<String> = build.sources.iter().map(|s| object(s)).collect();
    let cpu = if build.cpu == Cpu::M68020 { "CPU=68020" } else { "CPU=68000" };
    let target = if build.maths { format!("{cpu} MATH=IEEE") } else { cpu.to_string() };
    let libs = if build.maths { "LIB:scmieee.lib LIB:sc.lib LIB:amiga.lib" } else { "LIB:sc.lib LIB:amiga.lib" };
    out.push_str(&format!("PROGRAM = {}\nOBJECTS = {}\nTARGET = {target}\nLIBS = {libs}\n\n", build.program, objects.join(" ")));
    out.push_str("$(PROGRAM): $(OBJECTS)\n\tslink LIB:c.o $(OBJECTS) TO $(PROGRAM) LIB $(LIBS) $(LINKFLAGS)\n");
    for source in &build.sources {
        let mut needs = vec![source.clone()];
        needs.extend(build.headers.iter().cloned());
        out.push_str(&format!("\n{}: {}\n\tsc $(CFLAGS) $(TARGET) {source} OBJNAME={}\n", object(source), needs.join(" "), object(source)));
    }
    if let Some((cd, translations)) = &build.catalog {
        let stem = cd.strip_suffix(".cd").unwrap_or(cd);
        let stem = stem.rsplit('/').next().unwrap_or(stem);
        for (ct, language) in translations {
            out.push_str(&format!("\nCatalogs/{language}/{stem}.catalog: {cd} {ct}\n\tCatComp {cd} {ct} CATALOG Catalogs/{language}/{stem}.catalog\n"));
        }
    }
    out.push_str(END);
    out.push('\n');
    out
}

fn comment(text: &str) -> String {
    format!("# {}\n", crate::i18n::tr(text))
}

/// Makefile.gcc, for GNU make on this computer: `make -f Makefile.gcc`.
pub fn makefile_gcc(build: &Build) -> String {
    let mut out = comment("Bebbo's amiga-gcc builds the program with this: make -f Makefile.gcc");
    out.push_str(&comment("It's yours to change, apart from the lines Limoniter keeps up to date."));
    out.push_str("\nCC = m68k-amigaos-gcc\nCFLAGS = -Os -Wall\nLDFLAGS =\n");
    out.push_str(&comment("The program is what make builds when it isn't asked for anything else."));
    out.push_str(".DEFAULT_GOAL := all\n\n");
    out.push_str(&kept_gcc(build));
    out.push_str("\nOBJECTS = $(SOURCES:.c=.o)\n\nall: $(PROGRAM)\n\n$(PROGRAM): $(OBJECTS)\n\t$(CC) $(TARGET) $(LDFLAGS) -o $@ $(OBJECTS) $(LIBS)\n\n");
    out.push_str("%.o: %.c $(HEADERS)\n\t$(CC) $(TARGET) $(CFLAGS) -c -o $@ $<\n\nclean:\n\trm -f $(PROGRAM) $(OBJECTS)\n\n.PHONY: all clean catalogs\n");
    out
}

/// Makefile.vbcc, for GNU make on this computer: `make -f Makefile.vbcc`.
pub fn makefile_vbcc(build: &Build) -> String {
    let mut out = comment("vbcc builds the program with this: make -f Makefile.vbcc");
    out.push_str(&comment("It's yours to change, apart from the lines Limoniter keeps up to date."));
    out.push_str("\nCC = vc\nCFLAGS = -c99 -O1\nLDFLAGS =\n");
    out.push_str(&comment("The program is what make builds when it isn't asked for anything else."));
    out.push_str(".DEFAULT_GOAL := all\n\n");
    out.push_str(&kept_vbcc(build));
    out.push_str("\nOBJECTS = $(SOURCES:.c=.o)\n\nall: $(PROGRAM)\n\n$(PROGRAM): $(OBJECTS)\n\t$(CC) $(TARGET) $(LDFLAGS) -o $@ $(OBJECTS) $(LIBS)\n\n");
    out.push_str("%.o: %.c $(HEADERS)\n\t$(CC) $(TARGET) $(CFLAGS) -c -o $@ $<\n\nclean:\n\trm -f $(PROGRAM) $(OBJECTS)\n\n.PHONY: all clean catalogs\n");
    out
}

/// The smakefile, for SAS/C 6's smake on an Amiga.
pub fn smakefile(build: &Build) -> String {
    let mut out = comment("SAS/C 6 builds the program with this, on an Amiga: smake");
    out.push_str(&comment("It's yours to change, apart from the lines Limoniter keeps up to date."));
    out.push_str("\nCFLAGS = NOSTACKCHECK OPTIMIZE NOVERSION\nLINKFLAGS = SMALLCODE SMALLDATA NODEBUG NOICONS\n\n");
    out.push_str(&kept_smake(build));
    out
}

/// A makefile with its kept lines written again for a build; None if it has no kept lines (it's the person's own).
pub fn refresh(text: &str, build: &Build, which: &str) -> Option<String> {
    let start = text.find(BEGIN)?;
    let end = text[start..].find(END).map(|e| start + e)?;
    let end = text[end..].find('\n').map_or(text.len(), |n| end + n + 1);
    let kept = match which {
        MAKEFILE_GCC => kept_gcc(build),
        MAKEFILE_VBCC => kept_vbcc(build),
        _ => kept_smake(build),
    };
    Some(format!("{}{kept}{}", &text[..start], &text[end..]))
}

/// What a makefile's kept lines say: its settings and its PROGRAM and SOURCES, if it has them.
pub fn read_kept(text: &str) -> Option<(Os, bool, Cpu, String, Vec<String>)> {
    let start = text.find(BEGIN)?;
    let end = text[start..].find(END).map_or(text.len(), |e| start + e);
    let kept = &text[start..end];
    let (os, workbench, cpu) = kept.lines().find_map(read_settings)?;
    let value = |name: &str| kept.lines().find_map(|l| l.strip_prefix(name).map(|r| r.trim_start()).and_then(|r| r.strip_prefix('=')).map(|v| v.trim().to_string()));
    let program = value("PROGRAM")?;
    let sources = match value("SOURCES") {
        Some(sources) => sources.split_whitespace().map(str::to_string).collect(),
        None => value("OBJECTS").map(|o| o.split_whitespace().map(|o| format!("{}.c", o.strip_suffix(".o").unwrap_or(o))).collect()).unwrap_or_default(),
    };
    Some((os, workbench, cpu, program, sources))
}

// ---------- The project's files ----------

/// Folders that aren't the program's source.
const NOT_SOURCE: [&str; 6] = [".git", "Catalogs", "catalogs", "build", "target", ".svn"];

/// The files in a folder and below it, relative to it with / between folders, in name order.
fn files_below(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut waiting = vec![(folder.to_path_buf(), String::new(), 0)];
    while let Some((next, prefix, depth)) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 && !NOT_SOURCE.contains(&name.as_str()) && !name.starts_with('.') && !name.ends_with("-backups") {
                    waiting.push((path, format!("{prefix}{name}/"), depth + 1));
                }
            } else {
                found.push(format!("{prefix}{name}"));
            }
        }
    }
    found.sort_by_key(|f| f.to_lowercase());
    found
}

fn ends_with(name: &str, ending: &str) -> bool {
    name.to_ascii_lowercase().ends_with(ending)
}

/// Whether C source uses floating point: float or double as words outside comments and strings, or math.h.
pub fn uses_floating_point(text: &str) -> bool {
    let mut in_block = false;
    for line in text.lines() {
        let mut code = String::new();
        let mut chars = line.chars().peekable();
        let mut quote: Option<char> = None;
        while let Some(c) = chars.next() {
            if in_block {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    in_block = false;
                }
                continue;
            }
            match quote {
                Some(q) => {
                    if c == '\\' {
                        chars.next();
                    } else if c == q {
                        quote = None;
                    }
                    code.push(' ');
                }
                None => match c {
                    '"' | '\'' => quote = Some(c),
                    '/' if chars.peek() == Some(&'/') => break,
                    '/' if chars.peek() == Some(&'*') => {
                        chars.next();
                        in_block = true;
                    }
                    _ => code.push(c),
                },
            }
        }
        let trimmed = code.trim_start();
        if trimmed.starts_with('#') && trimmed.contains("include") && trimmed.contains("math.h") {
            return true;
        }
        if code.split(|c: char| !c.is_ascii_alphanumeric() && c != '_').any(|w| w == "float" || w == "double") {
            return true;
        }
    }
    false
}

/// Whether C source defines main().
fn has_main(text: &str) -> bool {
    text.lines().any(|line| {
        let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
        ["int main(", "void main(", "int main (", "void main (", "main(", "int __saveds main("].iter().any(|start| words.starts_with(start))
    })
}

/// A project's build as its folder has it now: its .c and .h files, whether they use floating point, and its
/// catalog, with the settings given (from the makefiles' `# limoniter:` line). The program's own file (the one
/// named after it, or else the one with main) comes first.
pub fn scan(folder: &Path, program: &str, os: Os, workbench: bool, cpu: Cpu) -> Build {
    let files = files_below(folder);
    let mut sources: Vec<String> = files.iter().filter(|f| ends_with(f, ".c")).cloned().collect();
    let headers: Vec<String> = files.iter().filter(|f| ends_with(f, ".h")).cloned().collect();
    let read = |f: &String| std::fs::read(folder.join(f)).map(|b| crate::amiga_catalog::decode(&b)).unwrap_or_default();
    let maths = sources.iter().chain(&headers).any(|f| uses_floating_point(&read(f)));
    let own = format!("{program}.c").to_lowercase();
    let first = sources.iter().position(|s| s.to_lowercase() == own).or_else(|| sources.iter().position(|s| has_main(&read(s))));
    if let Some(at) = first {
        let main = sources.remove(at);
        sources.insert(0, main);
    }
    let catalog = files.iter().find(|f| ends_with(f, ".cd")).map(|cd| {
        let translations = files.iter().filter(|f| ends_with(f, ".ct")).map(|ct| (ct.clone(), crate::amiga_catalog::language_of(&crate::amiga_catalog::read_translation(&read(ct)), Path::new(ct)))).collect();
        (cd.clone(), translations)
    });
    Build { program: program.to_string(), os, workbench, cpu, sources, headers, maths, catalog }
}

/// The build a folder's makefiles keep, from the first of them with kept lines, as its files are now.
pub fn build_of(folder: &Path) -> Option<Build> {
    [MAKEFILE_GCC, MAKEFILE_VBCC, SMAKEFILE].iter().find_map(|name| {
        let text = std::fs::read_to_string(folder.join(name)).ok()?;
        let (os, workbench, cpu, program, _) = read_kept(&text)?;
        Some(scan(folder, &program, os, workbench, cpu))
    })
}

/// Brings a folder's makefiles up to date with its files: each with kept lines is written again where they've
/// changed. The makefiles changed.
pub fn refresh_folder(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(build) = build_of(folder) else { return Ok(Vec::new()) };
    let mut changed = Vec::new();
    for name in [MAKEFILE_GCC, MAKEFILE_VBCC, SMAKEFILE] {
        let path = folder.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(new) = refresh(&text, &build, name)
            && new != text
        {
            crate::codec::write_file(&path, new.as_bytes()).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
            changed.push(path);
        }
    }
    Ok(changed)
}

/// Whether a folder's smakefile is one Limoniter keeps, rather than a SAS/C project's own.
pub fn smakefile_is_kept(folder: &Path) -> bool {
    ["smakefile", "SMakefile", "SMAKEFILE"].iter().any(|n| std::fs::read_to_string(folder.join(n)).is_ok_and(|t| t.contains(BEGIN)))
}

/// The folder whose makefiles a file belongs to: its own, or one above it (up to three) that has kept makefiles.
pub fn project_folder_of(file: &Path) -> Option<PathBuf> {
    let mut folder = file.parent();
    for _ in 0..4 {
        let here = folder?;
        if [MAKEFILE_GCC, MAKEFILE_VBCC, SMAKEFILE].iter().any(|n| std::fs::read_to_string(here.join(n)).is_ok_and(|t| t.contains(BEGIN))) {
            return Some(here.to_path_buf());
        }
        folder = here.parent();
    }
    None
}

/// How F9 builds a file of a project with kept makefiles: the files, the program's name, and what goes on each
/// compiler's command line (before the files, and the libraries after).
pub struct F9Build {
    pub folder: PathBuf,
    pub sources: Vec<PathBuf>,
    pub program: String,
    pub gcc: (Vec<String>, Vec<String>),
    pub vbcc: (Vec<String>, Vec<String>),
}

pub fn f9_build(file: &Path) -> Option<F9Build> {
    let folder = project_folder_of(file)?;
    let build = build_of(&folder)?;
    Some(F9Build {
        // Joined a folder at a time, so the paths are written as this computer writes them.
        sources: build.sources.iter().map(|s| s.split('/').fold(folder.clone(), |path, part| path.join(part))).collect(),
        folder: folder.clone(),
        program: build.program.clone(),
        gcc: (gcc_flags(&build), gcc_libs(&build)),
        vbcc: (vbcc_flags(&build), vbcc_libs(&build)),
    })
}

// ---------- A Workbench icon ----------

/// A tool's icon (.info) as icon.library writes one, so the program can be started from the Workbench: a
/// DiskObject (workbench/workbench.h: WB_DISKMAGIC, version 1, its Gadget, WBTOOL, the stack it needs) followed by
/// its picture, an Image (intuition/intuition.h) of 32 by 16 pixels in two bitplanes in the Workbench's four
/// colours.
pub fn tool_icon(stack: u32) -> Vec<u8> {
    const WIDTH: u16 = 32;
    const HEIGHT: u16 = 16;
    let mut out = Vec::new();
    let word = |out: &mut Vec<u8>, w: u16| out.extend_from_slice(&w.to_be_bytes());
    let long = |out: &mut Vec<u8>, l: u32| out.extend_from_slice(&l.to_be_bytes());
    word(&mut out, 0xE310); // do_Magic
    word(&mut out, 1); // do_Version
    // do_Gadget: NextGadget, LeftEdge, TopEdge, Width, Height, Flags (GFLG_GADGIMAGE | GFLG_GADGBACKFILL),
    // Activation (GACT_RELVERIFY | GACT_IMMEDIATE), GadgetType (GTYP_BOOLGADGET), GadgetRender (there is one),
    // SelectRender, GadgetText, MutualExclude, SpecialInfo, GadgetID, UserData (WB_DISKREVISION).
    long(&mut out, 0);
    word(&mut out, 0);
    word(&mut out, 0);
    word(&mut out, WIDTH);
    word(&mut out, HEIGHT + 1);
    word(&mut out, 0x0005);
    word(&mut out, 0x0003);
    word(&mut out, 0x0001);
    long(&mut out, 1);
    long(&mut out, 0);
    long(&mut out, 0);
    long(&mut out, 0);
    long(&mut out, 0);
    word(&mut out, 0);
    long(&mut out, 1);
    out.push(3); // do_Type: WBTOOL
    out.push(0);
    long(&mut out, 0); // do_DefaultTool
    long(&mut out, 0); // do_ToolTypes
    long(&mut out, 0x8000_0000); // do_CurrentX: NO_ICON_POSITION
    long(&mut out, 0x8000_0000); // do_CurrentY
    long(&mut out, 0); // do_DrawerData
    long(&mut out, 0); // do_ToolWindow
    long(&mut out, stack); // do_StackSize
    // The Image: LeftEdge, TopEdge, Width, Height, Depth, ImageData (there is some), PlanePick, PlaneOnOff,
    // NextImage.
    word(&mut out, 0);
    word(&mut out, 0);
    word(&mut out, WIDTH);
    word(&mut out, HEIGHT);
    word(&mut out, 2);
    long(&mut out, 1);
    out.push(3);
    out.push(0);
    long(&mut out, 0);
    // A little window: black edges (colour 1) round white (colour 2), with a blue title bar (colour 3).
    let colour = |x: u16, y: u16| -> u8 {
        let inside = (2..30).contains(&x) && (1..15).contains(&y);
        let edge = inside && (x == 2 || x == 29 || y == 1 || y == 14);
        if !inside {
            0
        } else if edge {
            1
        } else if y <= 4 {
            3
        } else {
            2
        }
    };
    for plane in 0..2 {
        for y in 0..HEIGHT {
            let mut bits = 0u32;
            for x in 0..WIDTH {
                if colour(x, y) >> plane & 1 == 1 {
                    bits |= 1 << (31 - x);
                }
            }
            long(&mut out, bits);
        }
    }
    out
}

// ---------- The templates ----------

/// What a new project is made of: each file's place in its folder and its bytes.
pub struct Made {
    pub files: Vec<(String, Vec<u8>)>,
    /// The file to open first.
    pub main: String,
    pub build: Build,
}

/// The year's date as version strings have it, for today.
fn today() -> (String, i32) {
    use chrono::Datelike;
    let now = chrono::Local::now().date_naive();
    (crate::amiga_catalog::amiga_date(now.year(), now.month(), now.day()), now.year())
}

/// A project made from a template, for a program of this name, dated `date` (20.11.2026).
pub fn make(template: &Template, program: &str, date: &str) -> Made {
    let fill = |text: &str| crate::i18n::comments(&text.replace("@NAME@", program).replace("@DATE@", date));
    let source = match template.kind {
        Kind::Shell if template.os == Os::Kick13 => SHELL_13,
        Kind::Workbench if template.os == Os::Kick13 => WORKBENCH_13,
        Kind::Shell => SHELL_3,
        Kind::Workbench => WORKBENCH_3,
        Kind::ReAction => REACTION,
        Kind::Mui => MUI,
        Kind::Multilingual => MULTILINGUAL,
    };
    let main = format!("{program}.c");
    let mut files: Vec<(String, Vec<u8>)> = vec![(main.clone(), fill(source).into_bytes())];
    let workbench = matches!(template.kind, Kind::Workbench | Kind::ReAction | Kind::Mui);
    let cpu = if template.kind == Kind::Mui { Cpu::M68020 } else { Cpu::M68000 };
    let mut catalog = None;
    if template.kind == Kind::Multilingual {
        // Catalog descriptions and translations are kept in Latin 1, as CatComp and FlexCat on the Amiga read them.
        let cd_text = fill(CATALOG_DESCRIPTION);
        let cd = crate::amiga_catalog::read_description(&cd_text);
        files.push((format!("{program}.cd"), crate::amiga_catalog::latin1(&cd_text)));
        files.push((format!("{program}_strings.h"), crate::amiga_catalog::c_header(&cd, program).into_bytes()));
        let mut translations = Vec::new();
        for (language, text) in [("deutsch", GERMAN), ("italiano", ITALIAN)] {
            let ct_text = fill(text);
            let ct = crate::amiga_catalog::read_translation(&ct_text);
            let ct_name = format!("locale/{language}.ct");
            files.push((ct_name.clone(), crate::amiga_catalog::latin1(&ct_text)));
            let catalog_file = format!("{program}.catalog");
            if let Ok(bytes) = crate::amiga_catalog::write_catalog(&cd, &ct, &catalog_file, language, true) {
                files.push((format!("Catalogs/{language}/{catalog_file}"), bytes));
            }
            translations.push((ct_name, language.to_string()));
        }
        catalog = Some((format!("{program}.cd"), translations));
    }
    if workbench {
        // MUI asks for more stack than the Workbench's usual 4096.
        files.push((format!("{program}.info"), tool_icon(if template.kind == Kind::Mui { 16384 } else { 8192 })));
    }
    let headers: Vec<String> = files.iter().map(|(f, _)| f.clone()).filter(|f| f.ends_with(".h")).collect();
    let maths = files.iter().filter(|(f, _)| f.ends_with(".c")).any(|(_, b)| uses_floating_point(&String::from_utf8_lossy(b)));
    let build = Build { program: program.to_string(), os: template.os, workbench, cpu, sources: vec![main.clone()], headers, maths, catalog };
    files.push((MAKEFILE_GCC.into(), makefile_gcc(&build).into_bytes()));
    files.push((MAKEFILE_VBCC.into(), makefile_vbcc(&build).into_bytes()));
    files.push((SMAKEFILE.into(), smakefile(&build).into_bytes()));
    Made { files, main, build }
}

/// The project's tasks: building and cleaning with each compiler's makefile.
pub fn tasks(catalog: bool) -> Vec<crate::tasks::Task> {
    use crate::tasks::{Kind, Task};
    let task = |name: &str, command: &str, kind: Kind| Task { name: name.into(), command: command.into(), kind };
    let mut tasks = vec![
        task("amiga-gcc: build", "make -f Makefile.gcc", Kind::Build),
        task("vbcc: build", "make -f Makefile.vbcc", Kind::Build),
        task("amiga-gcc: clean", "make -f Makefile.gcc clean", Kind::Other),
        task("vbcc: clean", "make -f Makefile.vbcc clean", Kind::Other),
    ];
    if catalog {
        tasks.push(task("FlexCat: the catalogs", "make -f Makefile.gcc catalogs", Kind::Other));
    }
    tasks
}

/// Makes a project in a new folder: its files, and a project file opening the folder. The project file's path.
pub fn create(parent: &Path, template: &Template, program: &str) -> Result<(PathBuf, PathBuf), String> {
    if let Some(problem) = name_problem(program) {
        return Err(problem);
    }
    let folder = parent.join(program);
    if folder.exists() && std::fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_some()) {
        return Err(crate::i18n::tf!("{} already has files in it. Choose another name or folder.", folder.display()));
    }
    let made = make(template, program, &today().0);
    for (name, bytes) in &made.files {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", parent.display()))?;
        }
        crate::codec::write_file(&path, bytes).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
    }
    let project_path = folder.join(format!("{program}.{}", crate::project::EXTENSION));
    let mut project = crate::project::Project::of_folder(&project_path, &folder);
    project.tasks = tasks(made.build.catalog.is_some());
    project.notes = crate::i18n::tr(template.about).to_string();
    project.save()?;
    Ok((project_path, folder.join(&made.main)))
}

// ---------- Importing ----------

/// A folder of Amiga C sorted into what each file is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sorted {
    pub sources: Vec<String>,
    pub headers: Vec<String>,
    pub assembler: Vec<String>,
    pub guides: Vec<String>,
    pub installer: Vec<String>,
    pub programs: Vec<String>,
    pub icons: Vec<String>,
    /// SAS/C's own: smakefile, SCOPTIONS, slink's WITH files, global symbol tables.
    pub sasc: Vec<String>,
    pub catalogs: Vec<String>,
    /// Object files and link libraries.
    pub objects: Vec<String>,
    pub makefiles: Vec<String>,
    pub other: Vec<String>,
}

impl Sorted {
    /// Each group with its name, in the order they're shown.
    pub fn groups(&self) -> [(&'static str, &Vec<String>); 12] {
        [
            ("C source", &self.sources),
            ("Headers", &self.headers),
            ("Assembler", &self.assembler),
            ("AmigaGuide documents", &self.guides),
            ("Installer scripts", &self.installer),
            ("Programs", &self.programs),
            ("Icons", &self.icons),
            ("SAS/C's files", &self.sasc),
            ("Catalogs", &self.catalogs),
            ("Objects and link libraries", &self.objects),
            ("Makefiles", &self.makefiles),
            ("Other files", &self.other),
        ]
    }
}

/// What one file is, by its name and, where that doesn't say, its first bytes.
fn sort_one(name: &str, head: &[u8], sorted: &mut Sorted) {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    let ending = base.rsplit_once('.').map_or("", |(_, e)| e);
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let group = match ending {
        "c" | "cpp" | "cc" => &mut sorted.sources,
        "h" | "hpp" => &mut sorted.headers,
        "s" | "asm" | "i" => &mut sorted.assembler,
        "a" if head.starts_with(b"!<arch>") => &mut sorted.objects,
        "a" => &mut sorted.assembler,
        "guide" => &mut sorted.guides,
        "info" => &mut sorted.icons,
        "cd" | "ct" | "catalog" | "sd" => &mut sorted.catalogs,
        "o" | "lib" => &mut sorted.objects,
        "lnk" | "with" | "gst" => &mut sorted.sasc,
        _ if base == "smakefile" || base == "scoptions" => &mut sorted.sasc,
        _ if base.starts_with("makefile") || base == "dmakefile" || base == "gnumakefile" => &mut sorted.makefiles,
        // An Installer script: named for it, or reading like one.
        "install" | "installer" => &mut sorted.installer,
        _ if base.starts_with("install") && (text.contains("(welcome") || text.contains("(copyfiles") || text.contains("(set @default-dest") || text.contains("(complete")) => &mut sorted.installer,
        _ if head.starts_with(&[0, 0, 3, 0xF3]) => &mut sorted.programs,
        _ if text.contains("@database") => &mut sorted.guides,
        _ => &mut sorted.other,
    };
    group.push(name.to_string());
}

/// Sorts a folder's files (and those in the folders below it) into what each is.
pub fn sort_folder(folder: &Path) -> Sorted {
    let mut sorted = Sorted::default();
    for name in files_below(folder) {
        let mut head = vec![0u8; 512];
        let read = std::fs::File::open(folder.join(&name)).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
        head.truncate(read);
        sort_one(&name, &head, &mut sorted);
    }
    sorted
}

/// Whether C source looks like a Workbench program: a window, a screen, ReAction, MUI or GadTools.
fn looks_like_workbench(text: &str) -> bool {
    ["OpenWindow", "OpenScreen", "WBStartup", "ApplicationObject", "WindowObject", "CreateGadget", "proto/intuition.h", "proto/muimaster.h", "reaction/"].iter().any(|w| text.contains(w))
}

/// What importing a folder did.
pub struct Imported {
    pub sorted: Sorted,
    pub project: PathBuf,
    /// The makefiles written, if there were none.
    pub written: Vec<String>,
}

/// Imports a folder of Amiga C as a project: sorts its files, writes Makefile.gcc and Makefile.vbcc where it has
/// none (from its smakefile's files if it's a SAS/C project, otherwise from its .c files), and makes a project
/// file for the folder with tasks to build it. Nothing already there is changed.
pub fn import(folder: &Path) -> Result<Imported, String> {
    let sorted = sort_folder(folder);
    let name = folder.file_name().map_or_else(|| "program".to_string(), |n| n.to_string_lossy().into_owned());
    let read = |f: &str| std::fs::read(folder.join(f)).map(|b| crate::amiga_catalog::decode(&b)).unwrap_or_default();
    let smake = crate::sasc::read_smakefile(folder);
    let program: String = smake.as_ref().and_then(|p| p.name.clone()).unwrap_or_else(|| name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect());
    let program = if program.is_empty() { "program".to_string() } else { program };
    let workbench = sorted.sources.iter().any(|s| looks_like_workbench(&read(s)));
    let mut build = scan(folder, &program, Os::Os3, workbench, Cpu::M68000);
    if sorted.sources.iter().any(|s| read(s).contains("proto/muimaster.h")) {
        build.cpu = Cpu::M68020;
    }
    // A SAS/C project's smakefile says which files make the program.
    if let Some(project) = &smake {
        let listed: Vec<String> = project.sources.iter().filter_map(|p| p.strip_prefix(folder).ok()).map(|p| p.to_string_lossy().replace('\\', "/")).filter(|p| p.ends_with(".c")).collect();
        if !listed.is_empty() {
            build.sources = listed;
        }
    }
    let mut written = Vec::new();
    if !build.sources.is_empty() {
        for (file, text) in [(MAKEFILE_GCC, makefile_gcc(&build)), (MAKEFILE_VBCC, makefile_vbcc(&build))] {
            let path = folder.join(file);
            if !path.exists() {
                crate::codec::write_file(&path, text.as_bytes()).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
                written.push(file.to_string());
            }
        }
    }
    let project_path = folder.join(format!("{program}.{}", crate::project::EXTENSION));
    if !project_path.exists() {
        let mut project = crate::project::Project::of_folder(&project_path, folder);
        project.tasks = tasks(build.catalog.is_some());
        if smake.is_some() {
            project.tasks.push(crate::tasks::Task { name: "SAS/C: smake (on an Amiga)".into(), command: "smake".into(), kind: crate::tasks::Kind::Other });
        }
        project.notes = summary(&sorted);
        project.save()?;
    }
    Ok(Imported { sorted, project: project_path, written })
}

/// The sorting, a line for each group that has files in it.
pub fn summary(sorted: &Sorted) -> String {
    sorted.groups().iter().filter(|(_, files)| !files.is_empty()).map(|(group, files)| format!("{}: {}", crate::i18n::tr(group), files.join(", "))).collect::<Vec<_>>().join("\n")
}

// ---------- The programs ----------

const SHELL_13: &str = r#"// machine: amiga
// @NAME@: a Shell program for AmigaOS 1.3 (Kickstart 1.2 and 1.3).
// It uses only what dos.library had then: Printf() and ReadArgs() came with 2.0, so it writes with Write()
// and takes its arguments from argv, as 1.3's own commands did.
#include <exec/types.h>
#include <libraries/dos.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <string.h>

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

// Writes text to the Shell's window.
static void say(const char *text)
{
    Write(Output(), (APTR)text, (LONG)strlen(text));
}

int main(int argc, char **argv)
{
    int i;

    // Started from the Workbench there's no Shell window to write in.
    if (argc == 0)
        return 0;
    say("Hello from @NAME@\n");
    for (i = 1; i < argc; i++)
    {
        say("You gave: ");
        say(argv[i]);
        say("\n");
    }
    return RETURN_OK;
}
"#;

const WORKBENCH_13: &str = r#"// machine: amiga
// @NAME@: a Workbench program for AmigaOS 1.3 (Kickstart 1.2 and 1.3).
// It opens intuition.library and graphics.library itself, as programs for 1.3 did, opens a window
// on the Workbench's screen with a NewWindow, writes in it, and waits for its close gadget.
#include <exec/types.h>
#include <libraries/dos.h>
#include <intuition/intuition.h>
#include <proto/exec.h>
#include <proto/intuition.h>
#include <proto/graphics.h>
#include <string.h>

struct IntuitionBase *IntuitionBase = NULL;
struct GfxBase *GfxBase = NULL;

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

int main(void)
{
    static const char greeting[] = "Hello from @NAME@";
    struct NewWindow newWindow;
    struct Window *window;
    struct IntuiMessage *message;
    BOOL running = TRUE;

    // Version 33 is Kickstart 1.2; 1.3 is 34.
    IntuitionBase = (struct IntuitionBase *)OpenLibrary((UBYTE *)"intuition.library", 33);
    GfxBase = (struct GfxBase *)OpenLibrary((UBYTE *)"graphics.library", 33);
    if (IntuitionBase != NULL && GfxBase != NULL)
    {
        // The names of 1.3's flags: 2.0 renamed them (IDCMP_CLOSEWINDOW, WFLG_CLOSEGADGET), keeping these.
        newWindow.LeftEdge = 20;
        newWindow.TopEdge = 20;
        newWindow.Width = 300;
        newWindow.Height = 80;
        newWindow.DetailPen = 0;
        newWindow.BlockPen = 1;
        newWindow.IDCMPFlags = CLOSEWINDOW;
        newWindow.Flags = WINDOWCLOSE | WINDOWDRAG | WINDOWDEPTH | ACTIVATE | SMART_REFRESH;
        newWindow.FirstGadget = NULL;
        newWindow.CheckMark = NULL;
        newWindow.Title = (UBYTE *)"@NAME@";
        newWindow.Screen = NULL;
        newWindow.BitMap = NULL;
        newWindow.MinWidth = 0;
        newWindow.MinHeight = 0;
        newWindow.MaxWidth = 0;
        newWindow.MaxHeight = 0;
        newWindow.Type = WBENCHSCREEN;
        window = OpenWindow(&newWindow);
        if (window != NULL)
        {
            SetAPen(window->RPort, 1);
            Move(window->RPort, 20, 40);
            Text(window->RPort, (UBYTE *)greeting, (LONG)strlen(greeting));
            while (running)
            {
                WaitPort(window->UserPort);
                while ((message = (struct IntuiMessage *)GetMsg(window->UserPort)) != NULL)
                {
                    if (message->Class == CLOSEWINDOW)
                        running = FALSE;
                    ReplyMsg((struct Message *)message);
                }
            }
            CloseWindow(window);
        }
    }
    if (GfxBase != NULL)
        CloseLibrary((struct Library *)GfxBase);
    if (IntuitionBase != NULL)
        CloseLibrary((struct Library *)IntuitionBase);
    return RETURN_OK;
}
"#;

const SHELL_3: &str = r#"// machine: amiga
// @NAME@: a Shell program for AmigaOS 3.
// Its arguments are read with ReadArgs() and a template, as the system's own commands read theirs:
// @NAME@ ? shows the template. Version @NAME@ shows the version string.
#include <exec/types.h>
#include <dos/dos.h>
#include <dos/rdargs.h>
#include <proto/exec.h>
#include <proto/dos.h>

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

#define TEMPLATE "NAME,QUIET/S"

enum { ARG_NAME, ARG_QUIET, ARG_COUNT };

int main(int argc, char **argv)
{
    LONG args[ARG_COUNT] = { 0, 0 };
    struct RDArgs *rdargs;
    STRPTR name;

    (void)argv;
    // Started from the Workbench there's no Shell window to write in.
    if (argc == 0)
        return RETURN_OK;
    rdargs = ReadArgs((CONST_STRPTR)TEMPLATE, args, NULL);
    if (rdargs == NULL)
    {
        PrintFault(IoErr(), (CONST_STRPTR)"@NAME@");
        return RETURN_FAIL;
    }
    name = args[ARG_NAME] ? (STRPTR)args[ARG_NAME] : (STRPTR)"world";
    if (!args[ARG_QUIET])
        Printf((CONST_STRPTR)"Hello, %s, from @NAME@\n", (LONG)name);
    FreeArgs(rdargs);
    return RETURN_OK;
}
"#;

const WORKBENCH_3: &str = r#"// machine: amiga
// @NAME@: a Workbench program for AmigaOS 3.
// The compiler's start-up code opens intuition.library and graphics.library for it (vbcc's auto.lib,
// amiga-gcc's libnix and SAS/C's sc.lib all do), and answers the Workbench's start-up message.
#include <exec/types.h>
#include <dos/dos.h>
#include <intuition/intuition.h>
#include <proto/exec.h>
#include <proto/intuition.h>
#include <proto/graphics.h>
#include <string.h>

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

int main(void)
{
    static const char greeting[] = "Hello from @NAME@";
    struct Window *window;
    struct IntuiMessage *message;
    BOOL running = TRUE;

    window = OpenWindowTags(NULL,
        WA_Title, (ULONG)"@NAME@",
        WA_Left, 20, WA_Top, 20,
        WA_InnerWidth, 280, WA_InnerHeight, 60,
        WA_IDCMP, IDCMP_CLOSEWINDOW | IDCMP_VANILLAKEY,
        WA_CloseGadget, TRUE, WA_DragBar, TRUE, WA_DepthGadget, TRUE,
        WA_Activate, TRUE, WA_SmartRefresh, TRUE,
        TAG_DONE);
    if (window == NULL)
        return RETURN_FAIL;
    SetAPen(window->RPort, 1);
    Move(window->RPort, window->BorderLeft + 20, window->BorderTop + 30);
    Text(window->RPort, (CONST_STRPTR)greeting, (ULONG)strlen(greeting));
    while (running)
    {
        // The window's messages, or Ctrl-C from a Shell it was started from.
        ULONG signals = Wait((1UL << window->UserPort->mp_SigBit) | SIGBREAKF_CTRL_C);
        if (signals & SIGBREAKF_CTRL_C)
            running = FALSE;
        while ((message = (struct IntuiMessage *)GetMsg(window->UserPort)) != NULL)
        {
            // The close gadget, or Escape.
            if (message->Class == IDCMP_CLOSEWINDOW || (message->Class == IDCMP_VANILLAKEY && message->Code == 27))
                running = FALSE;
            ReplyMsg((struct Message *)message);
        }
    }
    CloseWindow(window);
    return RETURN_OK;
}
"#;

const REACTION: &str = r#"// machine: amiga
// @NAME@: a ReAction program for AmigaOS 3.2.
// Its window is laid out by ReAction's classes: window.class holds a layout.gadget, which holds a
// label and a button. The classes are opened here, as the 3.2 NDK's examples open them.
#include <exec/types.h>
#include <dos/dos.h>
#include <intuition/intuition.h>
#include <intuition/classusr.h>
#include <clib/alib_protos.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/intuition.h>
#include <proto/window.h>
#include <proto/layout.h>
#include <proto/button.h>
#include <proto/label.h>
#include <classes/window.h>
#include <gadgets/layout.h>
#include <gadgets/button.h>
#include <images/label.h>
#include <reaction/reaction.h>
#include <reaction/reaction_macros.h>

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

struct Library *WindowBase = NULL;
struct Library *LayoutBase = NULL;
struct Library *ButtonBase = NULL;
struct Library *LabelBase = NULL;

// Each gadget's number, which comes back when it's used.
enum { GID_HELLO = 1, GID_QUIT };

int main(void)
{
    Object *windowObject;
    struct Window *window;
    ULONG signals, result, waitFor;
    UWORD code;
    BOOL running = TRUE;
    int clicks = 0;

    // Version 44 is ReAction as AmigaOS 3.5 brought it; 3.2's are newer still.
    WindowBase = OpenLibrary((CONST_STRPTR)"window.class", 44);
    LayoutBase = OpenLibrary((CONST_STRPTR)"gadgets/layout.gadget", 44);
    ButtonBase = OpenLibrary((CONST_STRPTR)"gadgets/button.gadget", 44);
    LabelBase = OpenLibrary((CONST_STRPTR)"images/label.image", 44);
    if (WindowBase != NULL && LayoutBase != NULL && ButtonBase != NULL && LabelBase != NULL)
    {
        windowObject = WindowObject,
            WA_Title, (ULONG)"@NAME@",
            WA_Activate, TRUE,
            WA_DepthGadget, TRUE,
            WA_DragBar, TRUE,
            WA_CloseGadget, TRUE,
            WA_SizeGadget, TRUE,
            WINDOW_Position, WPOS_CENTERSCREEN,
            WINDOW_ParentGroup, VLayoutObject,
                LAYOUT_SpaceOuter, TRUE,
                LAYOUT_DeferLayout, TRUE,
                LAYOUT_AddImage, LabelObject,
                    LABEL_Text, (ULONG)"Hello from @NAME@",
                LabelEnd,
                LAYOUT_AddChild, NewObject(BUTTON_GetClass(), NULL,
                    GA_ID, GID_HELLO,
                    GA_RelVerify, TRUE,
                    GA_Text, (ULONG)"_Hello",
                ButtonEnd,
                LAYOUT_AddChild, NewObject(BUTTON_GetClass(), NULL,
                    GA_ID, GID_QUIT,
                    GA_RelVerify, TRUE,
                    GA_Text, (ULONG)"_Quit",
                ButtonEnd,
            LayoutEnd,
        WindowEnd;
        if (windowObject != NULL)
        {
            window = RA_OpenWindow(windowObject);
            if (window != NULL)
            {
                GetAttr(WINDOW_SigMask, windowObject, &waitFor);
                while (running)
                {
                    signals = Wait(waitFor | SIGBREAKF_CTRL_C);
                    if (signals & SIGBREAKF_CTRL_C)
                        running = FALSE;
                    while ((result = RA_HandleInput(windowObject, &code)) != WMHI_LASTMSG)
                    {
                        switch (result & WMHI_CLASSMASK)
                        {
                        case WMHI_CLOSEWINDOW:
                            running = FALSE;
                            break;
                        case WMHI_GADGETUP:
                            if ((result & WMHI_GADGETMASK) == GID_QUIT)
                                running = FALSE;
                            else
                                Printf((CONST_STRPTR)"Hello number %ld\n", (LONG)++clicks);
                            break;
                        }
                    }
                }
            }
            // Disposing of the window object closes the window and disposes of everything in it.
            DisposeObject(windowObject);
        }
    }
    if (LabelBase != NULL)
        CloseLibrary(LabelBase);
    if (ButtonBase != NULL)
        CloseLibrary(ButtonBase);
    if (LayoutBase != NULL)
        CloseLibrary(LayoutBase);
    if (WindowBase != NULL)
        CloseLibrary(WindowBase);
    return RETURN_OK;
}
"#;

const MUI: &str = r#"// machine: amiga
// @NAME@: an MUI program, for MUI 5 (it works with MUI 3.8 and 4 too).
// It needs MUI's developer files (the SDK from muidev.de): their libraries/mui.h and
// proto/muimaster.h. MUI 5 for AmigaOS 3 needs a 68020 or better, so the makefiles build for one.
#include <exec/types.h>
#include <dos/dos.h>
#include <libraries/mui.h>
#include <clib/alib_protos.h>
#include <proto/exec.h>
#include <proto/intuition.h>
#include <proto/muimaster.h>

#ifndef MAKE_ID
#define MAKE_ID(a, b, c, d) ((ULONG)(a) << 24 | (ULONG)(b) << 16 | (ULONG)(c) << 8 | (ULONG)(d))
#endif

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

struct Library *MUIMasterBase = NULL;

int main(void)
{
    Object *app, *window, *hello, *quit;
    ULONG signals = 0;
    LONG clicks = 0;
    BOOL running = TRUE;

    MUIMasterBase = OpenLibrary((CONST_STRPTR)MUIMASTER_NAME, MUIMASTER_VMIN);
    if (MUIMasterBase == NULL)
        return RETURN_FAIL;
    app = ApplicationObject,
        MUIA_Application_Title, (ULONG)"@NAME@",
        MUIA_Application_Version, (ULONG)version,
        MUIA_Application_Copyright, (ULONG)"Free to use",
        MUIA_Application_Author, (ULONG)"You",
        MUIA_Application_Description, (ULONG)"An MUI program made with Limoniter",
        MUIA_Application_Base, (ULONG)"@NAME@",
        SubWindow, window = WindowObject,
            MUIA_Window_Title, (ULONG)"@NAME@",
            MUIA_Window_ID, MAKE_ID('M', 'A', 'I', 'N'),
            WindowContents, VGroup,
                Child, TextObject,
                    MUIA_Text_Contents, (ULONG)"\33cHello from @NAME@",
                End,
                Child, HGroup,
                    Child, hello = SimpleButton("_Hello"),
                    Child, quit = SimpleButton("_Quit"),
                End,
            End,
        End,
    End;
    if (app == NULL)
    {
        CloseLibrary(MUIMasterBase);
        return RETURN_FAIL;
    }
    // The close gadget and Quit end the program; Hello comes back to the loop as its own number.
    DoMethod(window, MUIM_Notify, MUIA_Window_CloseRequest, TRUE, MUIV_Notify_Application, 2, MUIM_Application_ReturnID, MUIV_Application_ReturnID_Quit);
    DoMethod(quit, MUIM_Notify, MUIA_Pressed, FALSE, MUIV_Notify_Application, 2, MUIM_Application_ReturnID, MUIV_Application_ReturnID_Quit);
    DoMethod(hello, MUIM_Notify, MUIA_Pressed, FALSE, MUIV_Notify_Application, 2, MUIM_Application_ReturnID, 1);
    set(window, MUIA_Window_Open, TRUE);
    while (running)
    {
        LONG id = (LONG)DoMethod(app, MUIM_Application_NewInput, (ULONG)&signals);
        if (id == MUIV_Application_ReturnID_Quit)
            running = FALSE;
        else if (id == 1)
            set(window, MUIA_Window_Title, ++clicks % 2 ? (ULONG)"Hello!" : (ULONG)"@NAME@");
        if (running && signals != 0)
        {
            signals = Wait(signals | SIGBREAKF_CTRL_C);
            if (signals & SIGBREAKF_CTRL_C)
                running = FALSE;
        }
    }
    set(window, MUIA_Window_Open, FALSE);
    // Disposing of the application disposes of its windows and everything in them.
    MUI_DisposeObject(app);
    CloseLibrary(MUIMasterBase);
    return RETURN_OK;
}
"#;

const MULTILINGUAL: &str = r#"// machine: amiga
// @NAME@: a multilingual Shell program for AmigaOS 3 (locale.library came with 2.1).
// Its words are described in @NAME@.cd; Limoniter makes @NAME@_strings.h from it, and a catalog
// from each translation in the locale folder, in Catalogs/<language>/@NAME@.catalog. Copy the Catalogs
// folder beside the program, or into LOCALE:Catalogs, and it speaks the language Locale prefs choose.
#include <exec/types.h>
#include <dos/dos.h>
#include <dos/rdargs.h>
#include <libraries/locale.h>
#include <proto/exec.h>
#include <proto/dos.h>
#include <proto/locale.h>

// The strings' numbers, and GetString(), which asks the catalog for each.
#define CATCOMP_NUMBERS
#define CATCOMP_CODE
#include "@NAME@_strings.h"

const char version[] = "$VER: @NAME@ 1.0 (@DATE@)";

struct LocaleBase *LocaleBase = NULL;

int main(int argc, char **argv)
{
    struct LocaleInfo li;
    LONG args[1] = { 0 };
    struct RDArgs *rdargs;
    STRPTR name;

    (void)argv;
    if (argc == 0)
        return RETURN_OK;
    li.li_LocaleBase = NULL;
    li.li_Catalog = NULL;
    // Without locale.library (before 2.1), the program's own English strings are used.
    LocaleBase = (struct LocaleBase *)OpenLibrary((CONST_STRPTR)"locale.library", 38);
    if (LocaleBase != NULL)
    {
        li.li_LocaleBase = LocaleBase;
        li.li_Catalog = OpenCatalog(NULL, (CONST_STRPTR)"@NAME@.catalog", OC_BuiltInLanguage, (ULONG)"english", TAG_DONE);
    }
    rdargs = ReadArgs((CONST_STRPTR)"NAME", args, NULL);
    if (rdargs == NULL)
        PrintFault(IoErr(), (CONST_STRPTR)"@NAME@");
    else
    {
        name = args[0] ? (STRPTR)args[0] : GetString(&li, MSG_WORLD);
        Printf((CONST_STRPTR)GetString(&li, MSG_HELLO), (LONG)name);
        Printf((CONST_STRPTR)GetString(&li, MSG_BYE));
        FreeArgs(rdargs);
    }
    if (LocaleBase != NULL)
    {
        CloseCatalog(li.li_Catalog);
        CloseLibrary((struct Library *)LocaleBase);
    }
    return RETURN_OK;
}
"#;

const CATALOG_DESCRIPTION: &str = "; The strings @NAME@ says: each a name, its number and its shortest and longest lengths in brackets,
; and on the next line the string in English, the language built into the program.
#language english
#version 1
MSG_HELLO (0/1/60)
Hello, %s, from @NAME@!\\n
;
MSG_BYE (1/1/40)
Goodbye.\\n
;
MSG_WORLD (2/1/20)
world
";

const GERMAN: &str = "## version $VER: @NAME@.catalog 1.0 (@DATE@)
## language deutsch
## codeset 0
; The German translation: each string's name, then its translation, then the English as a comment.
MSG_HELLO
Hallo, %s, Gr\u{fc}\u{df}e von @NAME@!\\n
; Hello, %s, from @NAME@!\\n
;
MSG_BYE
Tsch\u{fc}ss.\\n
; Goodbye.\\n
;
MSG_WORLD
Welt
; world
";

const ITALIAN: &str = "## version $VER: @NAME@.catalog 1.0 (@DATE@)
## language italiano
## codeset 0
; The Italian translation: each string's name, then its translation, then the English as a comment.
MSG_HELLO
Ciao, %s, da @NAME@!\\n
; Hello, %s, from @NAME@!\\n
;
MSG_BYE
Arrivederci.\\n
; Goodbye.\\n
;
MSG_WORLD
mondo
; world
";

#[cfg(test)]
mod tests;
