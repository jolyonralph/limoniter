//! Colour tags of your own: beyond the six, tags with a name and a colour you choose, and a key if you like. The
//! six can be renamed too. Each tag is kept in the text by its number: 0 to 5 are the six, 6 to 127 are your own
//! (kept in the settings) and 128 to 255 are the open project's (kept with it), so a number never means two things.
//! This module holds the tags that are in force, their names and colours, and how they're written down.

use eframe::egui::{Color32, Key, Modifiers};
use serde_json::{Value, json};
use std::cell::RefCell;

/// How many tags are built in.
pub const BUILT_IN: u8 = 6;
/// Where a project's own tag numbers begin.
pub const PROJECT_FIRST: u8 = 128;
/// The keys the six have, in order.
pub const BUILT_IN_KEYS: [&str; 6] = ["Ctrl+Shift+1", "Ctrl+Shift+2", "Ctrl+Shift+3", "Ctrl+Shift+4", "Ctrl+Shift+5", "Ctrl+Shift+6"];

/// One tag the person has made, or one of the six they've renamed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Def {
    pub id: u8,
    /// Empty for one of the six that keeps its own name.
    pub name: String,
    /// Red, green and blue; None for the six, which take their colours from the theme.
    pub colour: Option<[u8; 3]>,
    /// As in a menu: "Ctrl+Alt+T". Empty for none.
    pub key: String,
}

/// A tag as it's listed, whichever kind it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub id: u8,
    pub name: String,
    pub colour: Color32,
    pub key: String,
    pub project: bool,
}

#[derive(Default)]
struct InForce {
    user: Vec<Def>,
    project: Vec<Def>,
}

thread_local! {
    static TAGS: RefCell<InForce> = RefCell::default();
}

/// Makes these the tags in force: the person's own, and those of the open project.
pub fn set(user: &[Def], project: &[Def]) {
    TAGS.with(|cell| {
        let mut now = cell.borrow_mut();
        if now.user != user || now.project != project {
            now.user = user.to_vec();
            now.project = project.to_vec();
        }
    });
}

/// Changes the person's own tags, leaving the project's as they are.
pub fn set_user(user: &[Def]) {
    let project = project();
    set(user, &project);
}

/// Changes the open project's tags, leaving the person's as they are.
pub fn set_project(project: &[Def]) {
    let user = user();
    set(&user, project);
}

/// The person's own tags, and the six they've renamed.
pub fn user() -> Vec<Def> {
    TAGS.with(|cell| cell.borrow().user.clone())
}

/// The open project's tags.
pub fn project() -> Vec<Def> {
    TAGS.with(|cell| cell.borrow().project.clone())
}

fn find(id: u8) -> Option<Def> {
    TAGS.with(|cell| {
        let now = cell.borrow();
        now.project.iter().chain(now.user.iter()).find(|def| def.id == id).cloned()
    })
}

/// The name a tag goes by: its own, or the colour's for one of the six.
pub fn name(id: u8) -> String {
    match find(id) {
        Some(def) if !def.name.is_empty() => def.name,
        _ if id < BUILT_IN => crate::i18n::tr(crate::theme::TAG_NAMES[id as usize]).to_string(),
        _ => crate::i18n::tf!("Tag {}", id),
    }
}

/// The colour a tag is shown in. A tag that's since been deleted, still in an old file, shows grey.
pub fn colour(id: u8) -> Color32 {
    if id < BUILT_IN {
        return crate::theme::tag_colour(id);
    }
    find(id).and_then(|def| def.colour).map_or(Color32::GRAY, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Whether a tag number means something: one of the six, or one made and not deleted.
pub fn exists(id: u8) -> bool {
    id < BUILT_IN || find(id).is_some()
}

/// Every tag to offer, the project's first, then the six, then the person's own.
pub fn all() -> Vec<Shown> {
    TAGS.with(|cell| {
        let now = cell.borrow();
        let own = |def: &Def, project: bool| Shown { id: def.id, name: if def.name.is_empty() { name(def.id) } else { def.name.clone() }, colour: colour(def.id), key: def.key.clone(), project };
        let mut shown: Vec<Shown> = now.project.iter().filter(|d| d.id >= BUILT_IN).map(|d| own(d, true)).collect();
        for id in 0..BUILT_IN {
            let renamed = now.user.iter().find(|d| d.id == id).filter(|d| !d.name.is_empty());
            shown.push(Shown { id, name: renamed.map_or_else(|| crate::i18n::tr(crate::theme::TAG_NAMES[id as usize]).to_string(), |d| d.name.clone()), colour: crate::theme::tag_colour(id), key: BUILT_IN_KEYS[id as usize].into(), project: false });
        }
        shown.extend(now.user.iter().filter(|d| d.id >= BUILT_IN).map(|d| own(d, false)));
        shown
    })
}

/// The tags that have a key of their own, as the number and the key.
pub fn keys() -> Vec<(u8, String)> {
    all().into_iter().filter(|t| t.id >= BUILT_IN && !t.key.is_empty()).map(|t| (t.id, t.key)).collect()
}

/// The tags whose name contains what's typed, ignoring capitals. Nothing typed gives every tag.
pub fn matching(typed: &str) -> Vec<Shown> {
    let typed = typed.trim().to_lowercase();
    all().into_iter().filter(|t| t.name.to_lowercase().contains(&typed)).collect()
}

/// The first free number for a tag of your own, or a project's.
pub fn free_id(taken: &[Def], project: bool) -> Option<u8> {
    let range = if project { PROJECT_FIRST..=255 } else { BUILT_IN..=PROJECT_FIRST - 1 };
    range.into_iter().find(|id| !taken.iter().any(|d| d.id == *id))
}

/// A shortcut as the menus write it ("Ctrl+Alt+T"), as the modifiers and key to listen for. It must have Ctrl or
/// Alt held, so ordinary typing is never taken.
pub fn parse_key(text: &str) -> Option<(Modifiers, Key)> {
    let mut modifiers = Modifiers::NONE;
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, held) = parts.split_last()?;
    for part in held {
        match part.to_lowercase().as_str() {
            "ctrl" | "cmd" => modifiers.command = true,
            "alt" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            _ => return None,
        }
    }
    if !modifiers.command && !modifiers.alt {
        return None;
    }
    Some((modifiers, Key::from_name(key)?))
}

/// A key written one way is the same as written another: "ctrl+alt+t" and "Ctrl+Alt+T".
pub fn normal_key(text: &str) -> String {
    let Some((modifiers, key)) = parse_key(text) else { return text.trim().to_string() };
    let mut written = String::new();
    for (held, word) in [(modifiers.command, "Ctrl"), (modifiers.alt, "Alt"), (modifiers.shift, "Shift")] {
        if held {
            written += word;
            written += "+";
        }
    }
    written + key.name()
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

fn unhex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let part = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

fn clean(name: &str) -> String {
    name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// The person's tags as lines of the settings file.
pub fn to_settings(defs: &[Def]) -> String {
    defs.iter().map(|d| format!("own_tag={}\t{}\t{}\t{}\n", d.id, clean(&d.name), d.colour.map(hex).unwrap_or_default(), d.key)).collect()
}

/// The person's tags from the settings file. A line that makes no sense is skipped, so an older or newer file
/// still reads.
pub fn from_settings(text: &str) -> Vec<Def> {
    let mut defs: Vec<Def> = Vec::new();
    for value in text.lines().filter_map(|line| line.strip_prefix("own_tag=")) {
        let mut parts = value.split('\t');
        let (Some(id), Some(name), Some(colour), Some(key)) = (parts.next(), parts.next(), parts.next(), parts.next()) else { continue };
        let Ok(id) = id.parse::<u8>() else { continue };
        let colour = unhex(colour);
        let name = clean(name);
        let fits = if id < BUILT_IN { !name.is_empty() } else { id < PROJECT_FIRST && colour.is_some() && !name.is_empty() };
        if fits && !defs.iter().any(|d| d.id == id) {
            defs.push(Def { id, name, colour: if id < BUILT_IN { None } else { colour }, key: if id < BUILT_IN { String::new() } else { key.trim().to_string() } });
        }
    }
    defs
}

/// A project's tags as JSON.
pub fn to_json(defs: &[Def]) -> Value {
    Value::Array(defs.iter().map(|d| json!({"id": d.id, "name": d.name, "colour": d.colour.map(hex).unwrap_or_default(), "key": d.key})).collect())
}

/// A project's tags from JSON, skipping anything that doesn't fit.
pub fn from_json(value: Option<&Value>) -> Vec<Def> {
    let mut defs: Vec<Def> = Vec::new();
    for item in value.and_then(Value::as_array).into_iter().flatten() {
        let id = item.get("id").and_then(Value::as_u64).and_then(|n| u8::try_from(n).ok());
        let name = item.get("name").and_then(Value::as_str).map(clean).unwrap_or_default();
        let colour = item.get("colour").and_then(Value::as_str).and_then(unhex);
        let key = item.get("key").and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let Some(id) = id.filter(|id| *id >= PROJECT_FIRST) else { continue };
        if colour.is_some() && !name.is_empty() && !defs.iter().any(|d| d.id == id) {
            defs.push(Def { id, name, colour, key });
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mine(id: u8, name: &str, key: &str) -> Def {
        Def { id, name: name.into(), colour: Some([10, 200, 30]), key: key.into() }
    }

    #[test]
    fn a_tag_of_your_own_is_kept_in_the_settings_and_read_back() {
        let defs = vec![mine(6, "To check", "Ctrl+Alt+K"), Def { id: 1, name: "Slow".into(), colour: None, key: String::new() }, mine(7, "Ask Sam", "")];
        let text = to_settings(&defs);
        assert_eq!(from_settings(&format!("theme=dark\n{text}own_tag=bad\nown_tag=x\ty\tz\tw\n")), defs);
    }

    #[test]
    fn a_projects_tags_are_kept_in_json_and_read_back() {
        let defs = vec![mine(128, "Release", ""), mine(129, "Ask Sam", "Alt+Q")];
        assert_eq!(from_json(Some(&to_json(&defs))), defs);
        // A number in the person's range, or one with no colour, is no project tag.
        let odd = json!([{"id": 6, "name": "x", "colour": "#000000"}, {"id": 130, "name": "x"}, 5]);
        assert!(from_json(Some(&odd)).is_empty());
    }

    #[test]
    fn names_and_colours_come_from_the_tags_in_force() {
        set(&[mine(6, "To check", ""), Def { id: 2, name: "Maybe".into(), colour: None, key: String::new() }], &[mine(128, "Release", "")]);
        assert_eq!(name(6), "To check");
        assert_eq!(name(2), "Maybe");
        assert_eq!(name(0), "Red");
        assert_eq!(colour(6), Color32::from_rgb(10, 200, 30));
        let names: Vec<String> = all().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["Release", "Red", "Orange", "Maybe", "Green", "Blue", "Purple", "To check"]);
        assert_eq!(matching("che").len(), 1);
        assert_eq!(matching("  E").len(), 8);
        set(&[], &[]);
    }

    #[test]
    fn keys_are_read_and_need_ctrl_or_alt() {
        assert_eq!(parse_key("Ctrl+Alt+T"), Some((Modifiers::COMMAND | Modifiers::ALT, Key::T)));
        assert!(parse_key("T").is_none());
        assert!(parse_key("Shift+T").is_none());
        assert!(parse_key("Ctrl+Nonsense").is_none());
        assert_eq!(normal_key("alt+shift+q"), "Alt+Shift+Q");
    }

    #[test]
    fn numbers_come_from_the_right_range() {
        assert_eq!(free_id(&[], false), Some(6));
        assert_eq!(free_id(&[mine(6, "a", "")], false), Some(7));
        assert_eq!(free_id(&[], true), Some(128));
        let full: Vec<Def> = (6..128).map(|id| mine(id, "a", "")).collect();
        assert_eq!(free_id(&full, false), None);
    }
}
