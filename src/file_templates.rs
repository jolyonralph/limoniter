//! A project's own file templates: files and folders kept with the project that new files start from, with blanks
//! (`${name}`, `${Name}`, `${folder}`, `${namespace}`, `${date}`, `${year}`, `${author}`, `${cursor}`, and any others
//! asked for) filled in. This is the part that works on text and files; the windows are in `app.rs`'s
//! `templates_host.rs`.
//!
//! Where they live: a `templates` folder beside the `.limproj`, or `.limoniter/templates` in a project that is just
//! a folder, so they go into Git with the project. A template is a file (one new file) or a folder (several files,
//! whose own names may hold blanks too). A file called `.rules` in the same folder says which template new files in
//! a folder start from, one `folder = template` to a line.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The name of the file that says which template goes with which folder.
pub const RULES_FILE: &str = ".rules";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// What it's called in the menu: its file or folder name.
    pub name: String,
    pub path: PathBuf,
    /// Whether it's a folder, so it makes several files at once.
    pub folder: bool,
}

/// Where a project's templates are kept: `root` is set for a project that's just a folder.
pub fn dir_for(root: Option<&Path>, project_folder: &Path) -> PathBuf {
    match root {
        Some(root) => root.join(".limoniter").join("templates"),
        None => project_folder.join("templates"),
    }
}

/// The templates in a folder, by name. Names starting with a dot are the folder's own business.
pub fn list(dir: &Path) -> Vec<Template> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<Template> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let folder = entry.path().is_dir();
            Some(Template { name, path: entry.path(), folder })
        })
        .collect();
    found.sort_by_key(|template| template.name.to_lowercase());
    found
}

/// The values the blanks stand for.
#[derive(Clone, Debug, Default)]
pub struct Values {
    /// As typed, without the file ending.
    pub name: String,
    /// The folder the new files go in, by its own name.
    pub folder: String,
    pub namespace: String,
    pub date: String,
    pub year: String,
    pub author: String,
    /// Blanks of the person's own, by name.
    pub extra: BTreeMap<String, String>,
}

/// Words run together with each first letter in capitals: `my-component` and `my component` are `MyComponent`.
pub fn pascal(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            letters.next().map(|first| first.to_uppercase().chain(letters).collect::<String>()).unwrap_or_default()
        })
        .collect()
}

/// The namespace for a folder of a project, as PHP and C# have it: the folders below the project's, without a
/// leading `src`, each in capitals, joined with a backslash for PHP and a dot for anything else.
pub fn namespace_for(root: &Path, folder: &Path, ending: &str) -> String {
    let Ok(below) = folder.strip_prefix(root) else { return String::new() };
    let mut parts: Vec<String> = below.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.first().is_some_and(|first| ["src", "app", "lib", "source"].contains(&first.to_lowercase().as_str())) {
        parts.remove(0);
    }
    let parts: Vec<String> = parts.iter().map(|part| pascal(part)).filter(|part| !part.is_empty()).collect();
    parts.join(if ending.eq_ignore_ascii_case("php") { "\\" } else { "." })
}

/// One blank in a template: its name, and what to start with if the template gives one (`${label:default}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blank {
    pub label: String,
    pub default: String,
}

const KNOWN: [&str; 8] = ["name", "Name", "folder", "namespace", "date", "year", "author", "cursor"];

fn is_label(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|first| first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Splits text into pieces: plain text, and the blanks written as `${…}`. `$${` stands for a literal `${`, so a
/// template for JavaScript can hold its own template strings.
enum Piece<'a> {
    Text(&'a str),
    Escaped,
    Blank(&'a str, Option<&'a str>),
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut plain_from = 0;
    let mut at = 0;
    while let Some(found) = rest.find("$") {
        let start = at + found;
        let after = &text[start + 1..];
        if after.starts_with("${") {
            out.push(Piece::Text(&text[plain_from..start]));
            out.push(Piece::Escaped);
            plain_from = start + 3;
            at = plain_from;
        } else {
            let inner = after.strip_prefix('{').and_then(|inner| inner.find('}').map(|end| &inner[..end]));
            let split = inner.map(|inner| inner.split_once(':').map_or((inner, None), |(label, default)| (label, Some(default))));
            match (inner, split) {
                (Some(inner), Some((label, default))) if is_label(label) => {
                    out.push(Piece::Text(&text[plain_from..start]));
                    out.push(Piece::Blank(label, default));
                    plain_from = start + 3 + inner.len();
                    at = plain_from;
                }
                _ => at = start + 1,
            }
        }
        rest = &text[at..];
    }
    out.push(Piece::Text(&text[plain_from..]));
    out
}

/// The blanks of the person's own that some text asks for, each once, in the order they first come.
pub fn blanks(text: &str) -> Vec<Blank> {
    let mut found: Vec<Blank> = Vec::new();
    for piece in pieces(text) {
        if let Piece::Blank(label, default) = piece
            && !KNOWN.contains(&label)
            && !found.iter().any(|blank| blank.label == label)
        {
            found.push(Blank { label: label.to_string(), default: default.unwrap_or_default().to_string() });
        }
    }
    found
}

/// Fills in the blanks. Also says where `${cursor}` was, as a line and a character in it counted from 0, with the
/// marker taken out. A blank nobody has a value for is left as it is.
pub fn fill(text: &str, values: &Values) -> (String, Option<(usize, usize)>) {
    let mut out = String::with_capacity(text.len());
    let mut cursor = None;
    for piece in pieces(text) {
        match piece {
            Piece::Text(plain) => out.push_str(plain),
            Piece::Escaped => out.push_str("${"),
            Piece::Blank(label, default) => match label {
                "name" => out.push_str(&values.name),
                "Name" => out.push_str(&pascal(&values.name)),
                "folder" => out.push_str(&values.folder),
                "namespace" => out.push_str(&values.namespace),
                "date" => out.push_str(&values.date),
                "year" => out.push_str(&values.year),
                "author" => out.push_str(&values.author),
                "cursor" => {
                    if cursor.is_none() {
                        let line = out.matches('\n').count();
                        let start = out.rfind('\n').map_or(0, |at| at + 1);
                        cursor = Some((line, out[start..].chars().count()));
                    }
                }
                other => match values.extra.get(other).map(String::as_str).or(default) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push_str("${");
                        out.push_str(other);
                        out.push('}');
                    }
                },
            },
        }
    }
    (out, cursor)
}

/// Every blank of the person's own a template asks for, in its files' names and their text.
pub fn blanks_of(template: &Template) -> Vec<Blank> {
    let mut found: Vec<Blank> = Vec::new();
    let mut add = |more: Vec<Blank>| {
        for blank in more {
            if !found.iter().any(|known| known.label == blank.label) {
                found.push(blank);
            }
        }
    };
    for (relative, path) in files_of(template) {
        add(blanks(&relative.to_string_lossy()));
        if let Ok(text) = std::fs::read_to_string(path) {
            add(blanks(&text));
        }
    }
    found
}

/// A template's files: each with the path it makes, relative to the folder it's made in, and where it's read from.
fn files_of(template: &Template) -> Vec<(PathBuf, PathBuf)> {
    if !template.folder {
        return vec![(PathBuf::from(&template.name), template.path.clone())];
    }
    let mut out = Vec::new();
    let mut pending = vec![template.path.clone()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if let Ok(relative) = path.strip_prefix(&template.path) {
                out.push((relative.to_path_buf(), path.clone()));
            }
        }
    }
    out.sort();
    out
}

/// The file ending of a template's first file, which says what kind of files it makes.
pub fn ending_of_template(template: &Template) -> String {
    files_of(template).first().map(|(relative, _)| ending_of(&relative.to_string_lossy()).to_string()).unwrap_or_default()
}

/// What making a template's files comes to.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    /// Each file to write, with its whole path and its bytes.
    pub files: Vec<(PathBuf, Vec<u8>)>,
    /// The first `${cursor}`: the file's place in `files`, and its line and character.
    pub cursor: Option<(usize, usize, usize)>,
}

/// The file ending of a name, without the dot.
fn ending_of(name: &str) -> &str {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("")
}

/// What a new file's name is once the person has typed `typed` for a template that's a single file: a typed name
/// with an ending is kept; one without gets the template's ending. A template whose own name holds blanks
/// (`${Name}Controller.php`) names the file itself.
pub fn single_file_name(template: &Template, typed: &str, values: &Values) -> String {
    if template.name.contains("${") {
        return fill(&template.name, values).0;
    }
    let typed = typed.trim();
    let ending = ending_of(&template.name);
    if ending.is_empty() || !ending_of(typed).is_empty() {
        typed.to_string()
    } else {
        format!("{typed}.{ending}")
    }
}

/// Works out the files a template makes in a folder, filling in the blanks, without writing anything. Fails if one
/// of them is there already, so nothing is ever written over.
pub fn plan(template: &Template, target: &Path, typed: &str, values: &Values) -> Result<Plan, String> {
    let mut files = Vec::new();
    let mut cursor = None;
    for (relative, source) in files_of(template) {
        let wanted = if template.folder {
            let named = fill(&relative.to_string_lossy().replace('\\', "/"), values).0;
            named.split('/').fold(target.to_path_buf(), |path, part| path.join(part))
        } else {
            target.join(single_file_name(template, typed, values))
        };
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => {
                let (filled, at) = fill(&text, values);
                if cursor.is_none()
                    && let Some((line, column)) = at
                {
                    cursor = Some((files.len(), line, column));
                }
                filled.into_bytes()
            }
            // Not text, so there's nothing to fill in.
            Err(not_text) => not_text.into_bytes(),
        };
        files.push((wanted, bytes));
    }
    if files.is_empty() {
        return Err(crate::i18n::tr("That template has no files in it.").to_string());
    }
    if let Some((path, _)) = files.iter().find(|(path, _)| path.exists()) {
        return Err(crate::i18n::tf!("There's already something called {} there.", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    Ok(Plan { files, cursor })
}

/// Writes the files of a plan, making folders as needed.
pub fn write(plan: &Plan) -> Result<(), String> {
    for (path, bytes) in &plan.files {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Keeps a file as a template: its text goes in the templates folder under `name`. With `name_blank`, the file's own
/// name wherever it appears in the text becomes `${Name}`, so the next file made from it has its own.
pub fn save_as(dir: &Path, name: &str, text: &str, file_stem: &str, name_blank: bool) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(crate::i18n::tr("Give the template a name without slashes, and not starting with a dot.").to_string());
    }
    let path = dir.join(name);
    if path.exists() {
        return Err(crate::i18n::tf!("There's already a template called {name}."));
    }
    // A dollar and brace in the file are its own, so they're written so they come back as they were.
    let mut body = text.replace("${", "$${");
    if name_blank && file_stem.chars().count() >= 3 {
        body = body.replace(file_stem, "${Name}");
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

/// The folder-to-template rules, as written: folders by their path from the project's, with slashes.
pub fn rules(dir: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(dir.join(RULES_FILE)).unwrap_or_default();
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let (folder, template) = line.split_once('=')?;
            let folder = folder.trim().trim_matches('/').replace('\\', "/");
            let template = template.trim();
            (!template.is_empty()).then(|| (folder, template.to_string()))
        })
        .collect()
}

/// A folder's path from the project's, written with slashes.
pub fn relative_folder(root: &Path, folder: &Path) -> Option<String> {
    let below = folder.strip_prefix(root).ok()?;
    Some(below.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
}

/// The template new files in a folder start from, if a rule gives one: the rule for the folder itself, or else for
/// the nearest folder above it. The template has to exist.
pub fn rule_for(dir: &Path, root: &Path, folder: &Path) -> Option<Template> {
    let wanted = relative_folder(root, folder)?;
    let mut best: Option<(usize, String)> = None;
    for (rule_folder, template) in rules(dir) {
        let inside = rule_folder.is_empty() || wanted == rule_folder || wanted.starts_with(&format!("{rule_folder}/"));
        if inside && best.as_ref().is_none_or(|(length, _)| rule_folder.len() >= *length) {
            best = Some((rule_folder.len(), template));
        }
    }
    let (_, name) = best?;
    list(dir).into_iter().find(|template| template.name == name)
}

/// Says that new files in a folder start from a template, or (with None) that they no longer do.
pub fn set_rule(dir: &Path, folder: &str, template: Option<&str>) -> Result<(), String> {
    let folder = folder.trim_matches('/');
    let mut kept: Vec<(String, String)> = rules(dir).into_iter().filter(|(rule_folder, _)| rule_folder != folder).collect();
    if let Some(template) = template {
        kept.push((folder.to_string(), template.to_string()));
    }
    let path = dir.join(RULES_FILE);
    if kept.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let mut text = String::from("# Which template new files in a folder start from: folder = template\n");
    for (rule_folder, name) in kept {
        text.push_str(&format!("{rule_folder} = {name}\n"));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-templates-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn values() -> Values {
        Values { name: "my-card".into(), folder: "components".into(), namespace: "App\\Http".into(), date: "2026-11-22".into(), year: "2026".into(), author: "Sam".into(), extra: BTreeMap::new() }
    }

    #[test]
    fn the_usual_blanks_are_filled_in() {
        let (text, cursor) = fill("// ${name} / ${Name} / ${folder} / ${namespace} / ${date} / ${year} / ${author}", &values());
        assert_eq!(text, "// my-card / MyCard / components / App\\Http / 2026-11-22 / 2026 / Sam");
        assert_eq!(cursor, None);
    }

    #[test]
    fn the_cursor_marker_goes_and_its_place_is_kept() {
        let (text, cursor) = fill("class A {\n    ${cursor}\n}\n", &values());
        assert_eq!(text, "class A {\n    \n}\n");
        assert_eq!(cursor, Some((1, 4)));
        // Only the first counts; the rest just go.
        assert_eq!(fill("a${cursor}b${cursor}", &values()), ("ab".to_string(), Some((0, 1))));
    }

    #[test]
    fn blanks_of_ones_own_are_found_filled_and_given_defaults() {
        assert_eq!(blanks("${name} ${tag} ${tag:div} ${size:10}"), vec![Blank { label: "tag".into(), default: String::new() }, Blank { label: "size".into(), default: "10".into() }]);
        let mut with = values();
        with.extra.insert("tag".into(), "span".into());
        assert_eq!(fill("<${tag}>${size:10}${other}", &with).0, "<span>10${other}");
    }

    #[test]
    fn a_doubled_dollar_keeps_a_template_string_as_it_is() {
        assert!(blanks("const s = `$${x}`;").is_empty());
        assert_eq!(fill("const s = `$${x}`; ${name}", &values()).0, "const s = `${x}`; my-card");
        // Things that only look like blanks are left alone.
        assert_eq!(fill("cost: $5, ${ spaced }, ${", &values()).0, "cost: $5, ${ spaced }, ${");
    }

    #[test]
    fn names_become_pascal_case() {
        assert_eq!(pascal("my-card"), "MyCard");
        assert_eq!(pascal("user profile_view"), "UserProfileView");
        assert_eq!(pascal("Already"), "Already");
    }

    #[test]
    fn a_namespace_comes_from_the_folder() {
        let root = Path::new("/p");
        assert_eq!(namespace_for(root, Path::new("/p/src/Http/controllers"), "php"), "Http\\Controllers");
        assert_eq!(namespace_for(root, Path::new("/p/Models/user-data"), "cs"), "Models.UserData");
        assert_eq!(namespace_for(root, root, "php"), "");
        assert_eq!(namespace_for(root, Path::new("/elsewhere"), "php"), "");
    }

    #[test]
    fn templates_are_kept_beside_the_project_or_in_its_hidden_folder() {
        assert_eq!(dir_for(None, Path::new("/p")), Path::new("/p/templates"));
        assert_eq!(dir_for(Some(Path::new("/r")), Path::new("/elsewhere")), Path::new("/r/.limoniter/templates"));
    }

    #[test]
    fn a_file_template_makes_one_file_named_as_typed() {
        let dir = scratch("single");
        let templates = dir.join("templates");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::write(templates.join("component.tsx"), "export function ${Name}() {\n  return <div>${cursor}</div>;\n}\n").unwrap();
        std::fs::write(templates.join(".rules"), "").unwrap();
        let found = list(&templates);
        assert_eq!(found.len(), 1, "the dot file isn't a template");
        let target = dir.join("src");
        let plan = plan(&found[0], &target, "my-card", &values()).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].0, target.join("my-card.tsx"));
        assert_eq!(String::from_utf8_lossy(&plan.files[0].1), "export function MyCard() {\n  return <div></div>;\n}\n");
        assert_eq!(plan.cursor, Some((0, 1, 14)));
        write(&plan).unwrap();
        assert!(target.join("my-card.tsx").exists());
        // Nothing is written over.
        assert!(super::plan(&found[0], &target, "my-card", &values()).unwrap_err().contains("already"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_typed_ending_is_kept_and_a_template_that_names_its_files_does() {
        let single = Template { name: "page.html".into(), path: PathBuf::new(), folder: false };
        assert_eq!(single_file_name(&single, "index", &values()), "index.html");
        assert_eq!(single_file_name(&single, "index.htm", &values()), "index.htm");
        let named = Template { name: "${Name}Controller.php".into(), path: PathBuf::new(), folder: false };
        assert_eq!(single_file_name(&named, "ignored", &values()), "MyCardController.php");
    }

    #[test]
    fn a_folder_template_makes_several_files_with_filled_in_names() {
        let dir = scratch("folder");
        let template = dir.join("templates").join("react-component");
        std::fs::create_dir_all(template.join("styles")).unwrap();
        std::fs::write(template.join("${Name}.tsx"), "import './styles/${Name}.css';\n// ${author}").unwrap();
        std::fs::write(template.join("${Name}.test.tsx"), "test('${name}', () => {});").unwrap();
        std::fs::write(template.join("styles").join("${Name}.css"), ".${name} {}").unwrap();
        let found = list(&dir.join("templates"));
        assert!(found[0].folder);
        let target = dir.join("src").join("components");
        let plan = plan(&found[0], &target, "my-card", &values()).unwrap();
        let mut made: Vec<String> = plan.files.iter().map(|(path, _)| path.strip_prefix(&target).unwrap().to_string_lossy().replace('\\', "/")).collect();
        made.sort();
        assert_eq!(made, ["MyCard.test.tsx", "MyCard.tsx", "styles/MyCard.css"]);
        write(&plan).unwrap();
        assert_eq!(std::fs::read_to_string(target.join("MyCard.tsx")).unwrap(), "import './styles/MyCard.css';\n// Sam");
        assert_eq!(std::fs::read_to_string(target.join("styles").join("MyCard.css")).unwrap(), ".my-card {}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_template_says_which_blanks_it_needs_asking_for() {
        let dir = scratch("asks");
        std::fs::write(dir.join("model.php"), "<?php // ${table:items} ${name} ${owner}\n").unwrap();
        let template = Template { name: "model.php".into(), path: dir.join("model.php"), folder: false };
        let asked = blanks_of(&template);
        assert_eq!(asked.iter().map(|b| (b.label.as_str(), b.default.as_str())).collect::<Vec<_>>(), [("table", "items"), ("owner", "")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_a_file_as_a_template_turns_its_name_into_a_blank_and_it_comes_back_the_same() {
        let dir = scratch("save");
        let templates = dir.join("templates");
        let text = "class Invoice {}\nconst t = `${x}`;\n";
        let path = save_as(&templates, "class.php", text, "Invoice", true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "class ${Name} {}\nconst t = `$${x}`;\n");
        let found = list(&templates);
        let plan = plan(&found[0], &dir, "Receipt", &Values { name: "Receipt".into(), ..Values::default() }).unwrap();
        assert_eq!(String::from_utf8_lossy(&plan.files[0].1), "class Receipt {}\nconst t = `${x}`;\n");
        assert!(save_as(&templates, "class.php", text, "Invoice", true).unwrap_err().contains("already"));
        assert!(save_as(&templates, "a/b", text, "x", false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_template_can_be_chosen_by_itself_for_a_folder_and_what_is_inside_it() {
        let dir = scratch("rules");
        let templates = dir.join(".limoniter").join("templates");
        std::fs::create_dir_all(&templates).unwrap();
        std::fs::write(templates.join("component.tsx"), "x").unwrap();
        std::fs::write(templates.join("controller.php"), "y").unwrap();
        set_rule(&templates, "src/components", Some("component.tsx")).unwrap();
        set_rule(&templates, "src/components/forms", Some("controller.php")).unwrap();
        assert_eq!(rule_for(&templates, &dir, &dir.join("src/components")).unwrap().name, "component.tsx");
        assert_eq!(rule_for(&templates, &dir, &dir.join("src/components/buttons")).unwrap().name, "component.tsx", "below the folder too");
        assert_eq!(rule_for(&templates, &dir, &dir.join("src/components/forms/deep")).unwrap().name, "controller.php", "the nearest wins");
        assert!(rule_for(&templates, &dir, &dir.join("src/other")).is_none());
        assert!(rule_for(&templates, &dir, &dir.join("src/componentsX")).is_none(), "a longer name isn't inside");
        set_rule(&templates, "src/components", None).unwrap();
        assert!(rule_for(&templates, &dir, &dir.join("src/components")).is_none());
        // A rule for a template that's gone is no rule.
        std::fs::remove_file(templates.join("controller.php")).unwrap();
        assert!(rule_for(&templates, &dir, &dir.join("src/components/forms")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
