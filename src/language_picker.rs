//! Colour as: the box for choosing the language a file is coloured as. With over two hundred to choose from, a
//! menu was too long to use; here they're grouped, by kind (assembly, BASIC, the web and the rest) or by machine (a
//! ZX Spectrum's, an Amiga's, a mainframe's), where one language can be in several groups, as 6502 assembly is the
//! C64's, the BBC Micro's, the Atari's and the Apple II's. Or all of them A to Z. Typing finds one wherever it is,
//! by a few of its letters. Those chosen lately are at the top, with Work it out and Plain text.

use super::*;
use std::collections::BTreeSet;

/// How the languages are grouped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(super) enum View {
    #[default]
    Kind,
    Machine,
    All,
}

/// What the status bar asks for, when its language is clicked: the box opened.
pub(super) const OPEN_ASKED: &str = "language-picker-open";

impl View {
    const ALL: [View; 3] = [View::Kind, View::Machine, View::All];

    pub(super) fn from_setting(value: Option<&str>) -> View {
        match value {
            Some("machine") => View::Machine,
            Some("all") => View::All,
            _ => View::Kind,
        }
    }

    pub(super) fn setting(self) -> &'static str {
        match self {
            View::Kind => "kind",
            View::Machine => "machine",
            View::All => "all",
        }
    }

    fn label(self) -> &'static str {
        match self {
            View::Kind => "By kind",
            View::Machine => "By machine",
            View::All => "A to Z",
        }
    }
}

/// A group of languages: its name, and the languages in it, by the highlighter's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Assembly,
    Basic,
    Systems,
    JvmNet,
    Scripting,
    Functional,
    Web,
    ShellBuild,
    Data,
    Documents,
    Hardware,
    Mainframes,
    Logs,
    Spectrum,
    Commodore,
    Acorn,
    Amiga,
    AtariSt,
    Atari8,
    Apple2,
    Nintendo,
    Motorola6809,
    Z80Machines,
    CpM,
    Pc,
    Microcontrollers,
    IbmMainframe,
    Dec,
    Fiction,
    Unix,
    WebPlatform,
    ApplePlatform,
    MicrosoftPlatform,
    JvmPlatform,
    Git,
    Games,
    Science,
    Cloud,
    Other,
}

const KIND_GROUPS: [Group; 14] = [Group::Assembly, Group::Basic, Group::Systems, Group::JvmNet, Group::Scripting, Group::Functional, Group::Web, Group::ShellBuild, Group::Data, Group::Documents, Group::Hardware, Group::Mainframes, Group::Logs, Group::Other];
const MACHINE_GROUPS: [Group; 26] = [
    Group::Spectrum,
    Group::Commodore,
    Group::Acorn,
    Group::Amiga,
    Group::AtariSt,
    Group::Atari8,
    Group::Apple2,
    Group::Nintendo,
    Group::Motorola6809,
    Group::Z80Machines,
    Group::CpM,
    Group::Pc,
    Group::Microcontrollers,
    Group::IbmMainframe,
    Group::Dec,
    Group::Fiction,
    Group::Unix,
    Group::WebPlatform,
    Group::ApplePlatform,
    Group::MicrosoftPlatform,
    Group::JvmPlatform,
    Group::Git,
    Group::Games,
    Group::Science,
    Group::Cloud,
    Group::Other,
];

impl Group {
    fn name(self) -> &'static str {
        match self {
            Group::Assembly => "Assembly",
            Group::Basic => "BASIC",
            Group::Systems => "Systems and applications",
            Group::JvmNet => "Java, .NET and their kin",
            Group::Scripting => "Scripting",
            Group::Functional => "Functional and logic",
            Group::Web => "The web",
            Group::ShellBuild => "Shell, build and packaging",
            Group::Data => "Data and configuration",
            Group::Documents => "Documents and markup",
            Group::Hardware => "Hardware and shaders",
            Group::Mainframes => "Mainframes and minicomputers",
            Group::Logs => "Logs and system files",
            Group::Spectrum => "ZX Spectrum and Next",
            Group::Commodore => "Commodore 64, VIC-20 and PET",
            Group::Acorn => "BBC Micro and Acorn",
            Group::Amiga => "Amiga",
            Group::AtariSt => "Atari ST",
            Group::Atari8 => "Atari 8-bit and 2600",
            Group::Apple2 => "Apple II and the SNES",
            Group::Nintendo => "Game Boy, NES and SNES",
            Group::Motorola6809 => "Dragon, Color Computer and Vectrex",
            Group::Z80Machines => "MSX, Amstrad CPC and other Z80s",
            Group::CpM => "CP/M",
            Group::Pc => "IBM PC and DOS",
            Group::Microcontrollers => "Microcontrollers",
            Group::IbmMainframe => "IBM mainframes and IBM i",
            Group::Dec => "DEC PDP-11 and VAX",
            Group::Fiction => "Interactive fiction",
            Group::Unix => "Unix and Linux",
            Group::WebPlatform => "Web servers and browsers",
            Group::ApplePlatform => "Apple",
            Group::MicrosoftPlatform => "Microsoft and Windows",
            Group::JvmPlatform => "Java and Android",
            Group::Git => "Git",
            Group::Games => "Games and graphics",
            Group::Science => "Science and statistics",
            Group::Cloud => "Containers and the cloud",
            Group::Other => "Everything else",
        }
    }

    /// The languages in it, by the highlighter's names. Everything else's are worked out from the rest.
    fn members(self) -> &'static [&'static str] {
        match self {
            Group::Assembly => &["Assembly 6502", "Assembly 65816", "Assembly 6809", "Assembly 680x0", "Assembly 680x0 (GNU as)", "Assembly 8080", "Assembly 80x86", "Assembly ARM (ARM2 and ARM3)", "Assembly AVR", "Assembly HLASM", "Assembly PDP-11 (MACRO-11)", "Assembly PIC (MPASM)", "Assembly SM83 (Game Boy)", "Assembly SPC700 (SNES sound)", "Assembly VAX MACRO", "Assembly x86_64", "Assembly Z80", "LLVM"],
            Group::Basic => &["BASIC (AMOS)", "BASIC (Blitz Basic)", "BASIC (Commodore)", "BASIC (QBasic, QuickBASIC and GW-BASIC)", "BASIC (RGC BASIC)", "BASIC (Sinclair and Next)", "BASIC (STOS)", "xBase (dBase, Clipper, FoxPro)"],
            Group::Systems => &["Ada", "Amiga E", "C", "C++", "CMake C Header", "CMake C++ Header", "Crystal", "D", "Fortran (Fixed Form)", "Fortran (Modern)", "Go", "Modula-2", "Nim", "Objective-C", "Objective-C++", "Odin", "Pascal", "Rust", "Turbo Rascal (TRSE)", "Swift", "Zig", "Forth", "Logo"],
            Group::JvmNet => &["C#", "Clojure", "F#", "Groovy", "Java", "Kotlin", "Scala", "Dart"],
            Group::Scripting => &["AppleScript", "AWK", "GDScript (Godot Engine)", "JQ", "Julia", "Lua", "MATLAB", "Perl", "PHP", "Python", "R", "REXX", "Ruby", "Tcl", "Regular Expression", "Robot Framework"],
            Group::Functional => &["APL", "camlp4", "Elixir", "Elm", "Erlang", "F#", "Haskell", "Idris", "Lean 4", "Lisp", "Literate Haskell", "Nix", "OCaml", "OCamllex", "OCamlyacc", "Prolog", "PureScript", "Racket", "Smalltalk", "SML"],
            Group::Web => &["ActionScript", "ASP", "CFML", "CoffeeScript", "CSS", "GraphQL", "HTML", "HTML (ASP)", "HTML (EEx)", "HTML (Erlang)", "HTML (Jinja2)", "HTML (Rails)", "HTML (Tcl)", "HTML (Twig)", "HTTP Request and Response", "Java Server Page (JSP)", "JavaScript", "JavaScript (Rails)", "Jinja2", "Less", "LiveScript", "PHP", "Ruby Haml", "Ruby on Rails", "Ruby Slim", "Sass", "SCSS", "Stylus", "Svelte", "TypeScript", "TypeScriptReact", "Vue Component", "Solidity", "Vyper"],
            Group::ShellBuild => &["Batch File", "Bourne Again Shell (bash)", "Cabal", "CMake", "CMakeCache", "Command Help", "Crontab", "Dockerfile", "Dockerfile (with bash)", "Fish", "Gomod", "Gosum", "Makefile", "NAnt Build File", "Ninja", "NSIS", "Puppet", "Requirements.txt", "Terraform", "VimL", "Amiga Installer"],
            Group::Data => &["Apache Conf", "Comma Separated Values", "CONFIG.SYS (DOS)", "DotENV", "Fortran Namelist", "INI", "Java Properties", "JSON", "jsonnet", "nginx", "Pipe Separated Values", "Protocol Buffer", "Protocol Buffer (TEXT)", "Rego", "Semi-Colon Separated Values", "Separated Values", "SQL", "SQL (Rails)", "SSH Config", "SSHD Config", "Tab Separated Values", "TOML", "varlink", "XML", "YAML", "Graphviz (DOT)", "gnuplot"],
            Group::Documents => &["AmigaGuide", "AsciiDoc (Asciidoctor)", "BibTeX", "Email", "Groff/troff", "LaTeX", "Manpage", "Markdown", "MediaWiki", "MultiMarkdown", "orgmode", "PostScript", "Rd (R Documentation)", "reStructuredText", "RTF", "TeX", "Textile", "Todo.txt", "Typst"],
            Group::Hardware => &["GLSL", "SystemVerilog", "Verilog", "VHDL", "WGSL", "Assembly AVR", "Assembly PIC (MPASM)"],
            Group::Mainframes => &["Assembly HLASM", "COBOL", "JCL", "PL/I", "REXX", "RPG (IBM i)", "MUMPS (M)", "Assembly PDP-11 (MACRO-11)", "Assembly VAX MACRO"],
            Group::Logs => &["Authorized Keys", "CpuInfo", "debsources", "Diff", "fstab", "Git Log", "group", "Highlight non-printables", "Known Hosts", "LaTeX Log", "log", "MemInfo", "passwd", "Private Key", "R Console", "resolv", "Strace", "syslog"],
            Group::Spectrum => &["Assembly Z80", "BASIC (Sinclair and Next)"],
            Group::Commodore => &["Assembly 6502", "BASIC (Commodore)", "BASIC (RGC BASIC)", "Turbo Rascal (TRSE)"],
            Group::Acorn => &["Assembly 6502", "Assembly ARM (ARM2 and ARM3)"],
            Group::Amiga => &["Amiga E", "Assembly 680x0", "Assembly 680x0 (GNU as)", "BASIC (AMOS)", "BASIC (Blitz Basic)", "AmigaGuide", "Amiga Installer", "AmigaShell script", "REXX"],
            Group::AtariSt => &["Assembly 680x0", "BASIC (STOS)"],
            Group::Atari8 => &["Assembly 6502"],
            Group::Apple2 => &["Assembly 6502", "Assembly 65816", "AppleScript"],
            Group::Nintendo => &["Assembly SM83 (Game Boy)", "Assembly 6502", "Assembly 65816", "Assembly SPC700 (SNES sound)"],
            Group::Motorola6809 => &["Assembly 6809"],
            Group::Z80Machines => &["Assembly Z80"],
            Group::CpM => &["Assembly 8080", "Assembly Z80"],
            Group::Pc => &["Assembly 80x86", "Assembly x86_64", "BASIC (QBasic, QuickBASIC and GW-BASIC)", "Batch File", "CONFIG.SYS (DOS)", "Pascal", "xBase (dBase, Clipper, FoxPro)"],
            Group::Microcontrollers => &["Assembly AVR", "Assembly PIC (MPASM)", "Assembly ARM (ARM2 and ARM3)", "C", "C++", "Rust"],
            Group::IbmMainframe => &["Assembly HLASM", "COBOL", "JCL", "PL/I", "REXX", "RPG (IBM i)"],
            Group::Dec => &["Assembly PDP-11 (MACRO-11)", "Assembly VAX MACRO", "MUMPS (M)"],
            Group::Fiction => &["Inform 6", "ZIL (Zork Implementation Language)"],
            Group::Unix => &["AWK", "Bourne Again Shell (bash)", "C", "Crontab", "CpuInfo", "debsources", "Fish", "fstab", "Groff/troff", "group", "Makefile", "Manpage", "MemInfo", "passwd", "Perl", "resolv", "SSH Config", "SSHD Config", "Strace", "syslog", "Authorized Keys", "Known Hosts", "Private Key"],
            Group::WebPlatform => &["Apache Conf", "CSS", "HTML", "HTTP Request and Response", "JavaScript", "JSON", "nginx", "PHP", "TypeScript", "WGSL"],
            Group::ApplePlatform => &["AppleScript", "Objective-C", "Objective-C++", "Swift"],
            Group::MicrosoftPlatform => &["ASP", "Batch File", "C#", "F#", "HTML (ASP)", "TypeScript"],
            Group::JvmPlatform => &["Groovy", "Java", "Java Properties", "Java Server Page (JSP)", "Kotlin", "Scala", "Clojure"],
            Group::Git => &["Diff", "Git Attributes", "Git Commit", "Git Config", "Git Ignore", "Git Link", "Git Log", "Git Mailmap", "Git Rebase Todo"],
            Group::Games => &["GDScript (Godot Engine)", "GLSL", "Lua", "Odin", "WGSL", "C++", "C#"],
            Group::Science => &["Fortran (Fixed Form)", "Fortran (Modern)", "Fortran Namelist", "gnuplot", "Julia", "LaTeX", "MATLAB", "Python", "R", "R Console", "Rd (R Documentation)"],
            Group::Cloud => &["Dockerfile", "Dockerfile (with bash)", "nginx", "Nix", "Puppet", "Rego", "Terraform", "TOML", "YAML", "jsonnet"],
            Group::Other => &[],
        }
    }
}

/// The groups of a view, each with the languages (name and the highlighter's number) in it that the highlighter
/// has: Everything else holding any that no other group of the view does.
fn groups(view: View, names: &[(String, usize)]) -> Vec<(Group, Vec<(String, usize)>)> {
    let list: &[Group] = match view {
        View::Kind => &KIND_GROUPS,
        View::Machine => &MACHINE_GROUPS,
        View::All => &[],
    };
    let mut placed: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    for group in list {
        let mut found: Vec<(String, usize)> = if *group == Group::Other {
            Vec::new()
        } else {
            group.members().iter().filter_map(|member| names.iter().find(|(n, _)| n == member)).cloned().collect()
        };
        found.sort_by_key(|(n, _)| n.to_lowercase());
        for (name, _) in &found {
            placed.insert(names.iter().find(|(n, _)| n == name).map(|(n, _)| n.as_str()).unwrap_or(""));
        }
        if *group == Group::Other && view == View::Kind {
            found = names.iter().filter(|(n, _)| !placed.contains(n.as_str())).cloned().collect();
        }
        if !found.is_empty() {
            out.push((*group, found));
        }
    }
    // By machine, what's for no machine in particular is in the kinds; Everything else would be most of them.
    out
}

/// Every group, of either view, a language is in.
fn homes(name: &str, names: &[(String, usize)]) -> Vec<Group> {
    let mut found: Vec<Group> = KIND_GROUPS.iter().chain(MACHINE_GROUPS.iter()).copied().filter(|g| g.members().contains(&name)).collect();
    found.dedup();
    if found.is_empty() && names.iter().any(|(n, _)| n == name) {
        found.push(Group::Other);
    }
    found
}

/// The Colour as box's state.
pub(super) struct Picker {
    pub query: String,
    pub view: View,
    /// The group shown, by its place in the view's list.
    pub group: usize,
    /// The language chosen with the arrows, in what's shown.
    chosen: usize,
    focus: bool,
}

impl Limoniter {
    /// Opens the Colour as box, on the group the file's language is in.
    pub(super) fn open_language_picker(&mut self) {
        if self.no_file() || self.docs[self.active].hex_mode || highlight::engine().is_none() {
            return;
        }
        let view = self.language_view;
        let names = highlight::engine().map(|e| e.names().to_vec()).unwrap_or_default();
        let current = self.docs[self.active].resolved_syntax.flatten().and_then(|s| names.iter().find(|(_, n)| *n == s).map(|(n, _)| n.clone()));
        let group = current.and_then(|c| groups(view, &names).iter().position(|(_, list)| list.iter().any(|(n, _)| *n == c))).unwrap_or(0);
        self.language_picker = Some(Box::new(Picker { query: String::new(), view, group, chosen: 0, focus: true }));
    }

    /// Colours the file in front as a language, remembering it among those chosen lately.
    fn choose_language(&mut self, language: Language) {
        if let Language::Named(syntax) = language
            && let Some(name) = highlight::engine().and_then(|e| e.names().iter().find(|(_, s)| *s == syntax).map(|(n, _)| n.clone()))
        {
            self.recent_languages.retain(|n| *n != name);
            self.recent_languages.insert(0, name);
            self.recent_languages.truncate(MOST_RECENT_LANGUAGES);
            self.save_settings_if_changed();
        }
        self.doc().set_language(language);
    }

    pub(super) fn language_picker_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.language_picker.take() else { return };
        let Some(engine) = highlight::engine() else { return };
        let names: Vec<(String, usize)> = engine.names().to_vec();
        let doc = &self.docs[self.active];
        let current = doc.language;
        let detected = {
            let named = doc.path_or_name();
            let sample: Vec<Arc<str>> = (0..doc.text.line_count().min(ASM_SAMPLE_LINES)).map(|i| doc.text.line(i)).collect();
            engine.detect_with_sample(Some(&named), &sample).map(|s| engine.name_of(s).to_string())
        };
        let searching = !state.query.trim().is_empty();
        // What's shown on the right: the matches when searching, else the group chosen (or all, A to Z).
        let view_groups = groups(state.view, &names);
        state.group = state.group.min(view_groups.len().saturating_sub(1));
        let shown: Vec<(String, usize, Vec<usize>)> = if searching {
            // By its own name first; then by the name of a group it's in (amiga, mainframe), below those.
            let by_group = |n: &str| homes(n, &names).into_iter().filter(|g| *g != Group::Other).filter_map(|g| crate::workspace::score(&state.query, crate::i18n::tr(g.name())).map(|(score, _)| score)).max();
            let mut scored: Vec<(i64, String, usize, Vec<usize>)> = names
                .iter()
                .filter_map(|(n, s)| match crate::workspace::score(&state.query, n) {
                    Some((score, m)) => Some((score, n.clone(), *s, m)),
                    None => by_group(n).map(|score| (score - i64::MAX / 4, n.clone(), *s, Vec::new())),
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
            scored.into_iter().map(|(_, n, s, m)| (n, s, m)).collect()
        } else if state.view == View::All {
            let mut all: Vec<(String, usize, Vec<usize>)> = names.iter().map(|(n, s)| (n.clone(), *s, Vec::new())).collect();
            all.sort_by_key(|(n, ..)| n.to_lowercase());
            all
        } else {
            view_groups.get(state.group).map(|(_, list)| list.iter().map(|(n, s)| (n.clone(), *s, Vec::new())).collect()).unwrap_or_default()
        };
        state.chosen = state.chosen.min(shown.len().saturating_sub(1));
        let (mut pick, mut close) = (None, false);
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                state.chosen = (state.chosen + 1).min(shown.len().saturating_sub(1));
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                state.chosen = state.chosen.saturating_sub(1);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                pick = shown.get(state.chosen).map(|(_, s, _)| Language::Named(*s));
            }
        });
        let recent: Vec<(String, usize)> = self.recent_languages.iter().filter_map(|r| names.iter().find(|(n, _)| n == r)).cloned().collect();
        crate::fit::Dialog::new(crate::i18n::tr("Colour as")).collapsible(false).resizable(true).default_width(780.0).default_height(520.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(crate::i18n::tr("Type to find a language, such as 6502 or rust")).desired_width(340.0));
                if state.focus {
                    field.request_focus();
                    state.focus = false;
                }
                if field.changed() {
                    state.chosen = 0;
                }
                ui.add_space(12.0);
                for view in View::ALL {
                    if ui.selectable_label(state.view == view && !searching, crate::i18n::tr(view.label())).clicked() {
                        state.view = view;
                        state.query.clear();
                        state.group = 0;
                        state.chosen = 0;
                    }
                }
            });
            // Work it out, Plain text, and those chosen lately.
            ui.horizontal_wrapped(|ui| {
                let auto = match &detected {
                    Some(name) => crate::i18n::tf!("Work it out ({name})"),
                    None => crate::i18n::tr("Work it out").to_string(),
                };
                if ui.selectable_label(current == Language::Auto, auto).clicked() {
                    pick = Some(Language::Auto);
                }
                if ui.selectable_label(current == Language::Off, crate::i18n::tr("Plain text")).clicked() {
                    pick = Some(Language::Off);
                }
                if !recent.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Lately:")).color(theme::text_faint()));
                    for (name, syntax) in &recent {
                        if ui.selectable_label(current == Language::Named(*syntax), name.as_str()).clicked() {
                            pick = Some(Language::Named(*syntax));
                        }
                    }
                }
            });
            ui.separator();
            // At least 300 high, unless the screen hasn't room for that.
            let height = ui.available_height().max(300.0_f32.min(crate::fit::height_left(ui))) - 8.0;
            ui.horizontal_top(|ui| {
                // The groups, down the left, unless searching or A to Z.
                if !searching && state.view != View::All {
                    egui::ScrollArea::vertical().id_salt("language-groups").max_height(height).auto_shrink([true, false]).show(ui, |ui| {
                        ui.set_width(250.0);
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        for (n, (group, list)) in view_groups.iter().enumerate() {
                            let has_current = list.iter().any(|(_, s)| current == Language::Named(*s));
                            let words = RichText::new(crate::i18n::tf!("{}  ({})", crate::i18n::tr(group.name()), list.len()));
                            let words = if has_current { words.strong() } else { words };
                            if ui.selectable_label(n == state.group, words).clicked() {
                                state.group = n;
                                state.chosen = 0;
                            }
                        }
                        });
                    });
                    ui.separator();
                }
                egui::ScrollArea::vertical().id_salt("language-list").max_height(height).auto_shrink([false, false]).show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    if shown.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("No language has those letters.")).color(theme::text_faint()));
                    }
                    for (n, (name, syntax, matched)) in shown.iter().enumerate() {
                        let mut job = egui::text::LayoutJob::default();
                        let plain = egui::TextFormat { color: theme::text(), ..Default::default() };
                        let strong = egui::TextFormat { color: theme::accent(), ..Default::default() };
                        for (at, c) in name.chars().enumerate() {
                            job.append(&c.to_string(), 0.0, if matched.contains(&at) { strong.clone() } else { plain.clone() });
                        }
                        // Searching, where it's grouped; in a group, the other groups it's in too.
                        let places: Vec<String> = homes(name, &names).into_iter().filter(|g| searching || view_groups.get(state.group).is_none_or(|(here, _)| here != g)).filter(|g| *g != Group::Other).map(|g| crate::i18n::tr(g.name()).to_string()).collect();
                        if !places.is_empty() {
                            job.append(&format!("   {}", places.join(", ")), 0.0, egui::TextFormat { color: theme::text_faint(), font_id: egui::FontId::proportional(11.5), ..Default::default() });
                        }
                        let selected = n == state.chosen || current == Language::Named(*syntax);
                        let row = ui.selectable_label(selected, job);
                        if n == state.chosen {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            pick = Some(Language::Named(*syntax));
                        }
                    }
                    });
                });
            });
        });
        self.language_view = state.view;
        if let Some(language) = pick {
            self.choose_language(language);
            self.pending_focus = true;
        } else if close {
            self.pending_focus = true;
        } else {
            self.language_picker = Some(state);
        }
    }
}

/// How many languages chosen lately are kept.
const MOST_RECENT_LANGUAGES: usize = 6;

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// Every group's languages are ones the highlighter has, and every language is in some kind.
    #[test]
    fn every_language_has_a_kind_and_every_member_is_real() {
        crate::highlight::load_now();
        let names = highlight::engine().unwrap().names().to_vec();
        for group in KIND_GROUPS.iter().chain(MACHINE_GROUPS.iter()) {
            for member in group.members() {
                assert!(names.iter().any(|(n, _)| n == member), "{member} in {} isn't a language", group.name());
            }
        }
        let kinds = groups(View::Kind, &names);
        let count: BTreeSet<&str> = kinds.iter().flat_map(|(_, l)| l.iter().map(|(n, _)| n.as_str())).collect();
        assert_eq!(count.len(), names.len(), "every language in some kind");
        // 6502 assembly is in several machines' groups.
        let machines = groups(View::Machine, &names);
        let with_6502 = machines.iter().filter(|(_, l)| l.iter().any(|(n, _)| n == "Assembly 6502")).count();
        assert!(with_6502 >= 4, "{with_6502}");
        assert!(homes("Assembly 6502", &names).contains(&Group::Assembly));
    }

    /// Typing finds a language; Enter colours the file as it, and it's remembered among those chosen lately.
    #[test]
    fn typing_finds_a_language_and_enter_chooses_it() {
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        app.new_document();
        app.open_language_picker();
        app.language_picker.as_mut().unwrap().query = "asm6809".into();
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.input_mut(|i| i.events.push(egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }));
            app.language_picker_window(ui.ctx());
        });
        assert!(app.language_picker.is_none());
        app.doc().update_syntax();
        assert_eq!(app.doc().language_name(), "Assembly 6809");
        assert_eq!(app.recent_languages.first().map(String::as_str), Some("Assembly 6809"));
        // A machine's name finds its languages.
        app.open_language_picker();
        app.language_picker.as_mut().unwrap().query = "amiga".into();
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.input_mut(|i| for _ in 0..2 { i.events.push(egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }) });
            app.language_picker_window(ui.ctx());
        });
        assert!(app.language_picker.is_some());
        // Drawn in each view.
        for view in View::ALL {
            app.open_language_picker();
            app.language_picker.as_mut().unwrap().view = view;
            let _ = ctx.run_ui(Default::default(), |ui| app.language_picker_window(ui.ctx()));
            assert!(app.language_picker.is_some());
        }
    }
}
