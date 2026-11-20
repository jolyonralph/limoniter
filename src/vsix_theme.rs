//! VS Code colour themes made into Limoniter's: each gives a document theme (the editor's colours), a window theme
//! (the side bar's, lists' and boxes'), and a syntax colour scheme from its token colours. VS Code names a few
//! hundred colours; the ones that mean the same as one of Limoniter's are taken, and the rest of Limoniter's are
//! worked out from those as Limoniter's own themes are.

use crate::theme::{self, DocumentTheme, WindowTheme, mix};
use eframe::egui::Color32;
use serde_json::{Map, Value};
use std::path::Path;
use std::str::FromStr;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};

/// A theme as read from its file, with what it includes taken in: its named colours and its token colours.
#[derive(Default, Debug)]
pub struct ThemeFile {
    pub colours: Map<String, Value>,
    pub tokens: Vec<Value>,
    pub kind: Option<String>,
}

/// Reads a theme file: VS Code's JSON, which may include another file and may take its token colours from a
/// .tmTheme, or a .tmTheme itself.
pub fn read(path: &Path) -> Result<ThemeFile, String> {
    read_inner(path, 0)
}

fn read_inner(path: &Path, depth: usize) -> Result<ThemeFile, String> {
    if depth > 6 {
        return Err(crate::i18n::tr("its themes include each other round in a circle").into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", path.display()))?;
    let value = crate::vsix_grammar::read_structured(path, &text)?;
    let folder = path.parent().unwrap_or(Path::new("."));
    // A .tmTheme: its settings are a list whose first entry, without a scope, is the editor's own colours.
    if let Some(settings) = value.get("settings").and_then(Value::as_array) {
        let mut file = ThemeFile::default();
        for entry in settings {
            if entry.get("scope").is_none() {
                if let Some(global) = entry.get("settings").and_then(Value::as_object) {
                    for (key, vscode) in [("background", "editor.background"), ("foreground", "editor.foreground"), ("caret", "editorCursor.foreground"), ("selection", "editor.selectionBackground"), ("lineHighlight", "editor.lineHighlightBackground"), ("invisibles", "editorWhitespace.foreground")] {
                        if let Some(colour) = global.get(key) {
                            file.colours.insert(vscode.into(), colour.clone());
                        }
                    }
                }
            } else {
                file.tokens.push(entry.clone());
            }
        }
        return Ok(file);
    }
    let mut file = match value.get("include").and_then(Value::as_str) {
        Some(include) => read_inner(&folder.join(include), depth + 1)?,
        None => ThemeFile::default(),
    };
    if let Some(colours) = value.get("colors").and_then(Value::as_object) {
        for (key, colour) in colours {
            file.colours.insert(key.clone(), colour.clone());
        }
    }
    match value.get("tokenColors") {
        Some(Value::Array(tokens)) => file.tokens.extend(tokens.iter().cloned()),
        Some(Value::String(other)) => {
            let other = read_inner(&folder.join(other), depth + 1)?;
            file.tokens.extend(other.tokens);
            for (key, colour) in other.colours {
                file.colours.entry(key).or_insert(colour);
            }
        }
        _ => {}
    }
    if let Some(kind) = value.get("type").and_then(Value::as_str) {
        file.kind = Some(kind.to_string());
    }
    Ok(file)
}

/// A colour as VS Code writes it: #rgb, #rgba, #rrggbb or #rrggbbaa.
pub fn colour(text: &str) -> Option<Color32> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let short = |i: usize| u8::from_str_radix(hex.get(i..=i)?, 16).ok().map(|v| v * 17);
    let long = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        3 => Some(Color32::from_rgb(short(0)?, short(1)?, short(2)?)),
        4 => Some(Color32::from_rgba_unmultiplied(short(0)?, short(1)?, short(2)?, short(3)?)),
        6 => Some(Color32::from_rgb(long(0)?, long(2)?, long(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(long(0)?, long(2)?, long(4)?, long(6)?)),
        _ => None,
    }
}

/// A see-through colour as it looks over another, so it can be drawn as it is.
pub fn over(colour: Color32, under: Color32) -> Color32 {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    mix(under, Color32::from_rgb(r, g, b), f32::from(a) / 255.0)
}

/// Whether a colour is light, by how bright it looks.
fn is_light(colour: Color32) -> bool {
    f32::from(colour.r()) * 0.299 + f32::from(colour.g()) * 0.587 + f32::from(colour.b()) * 0.114 > 140.0
}

/// A theme made into Limoniter's.
pub struct Converted {
    pub document: DocumentTheme,
    pub window: WindowTheme,
    pub scheme: Theme,
}

/// Makes a VS Code theme into Limoniter's themes, by the name it's to have and VS Code's uiTheme (vs, vs-dark,
/// hc-black or hc-light) as the extension says it.
pub fn convert(file: &ThemeFile, name: &str, scheme_name: &str, ui_theme: &str) -> Converted {
    let get = |key: &str| file.colours.get(key).and_then(Value::as_str).and_then(colour);
    let said_light = match ui_theme {
        "vs" | "hc-light" => Some(true),
        "vs-dark" | "hc-black" => Some(false),
        _ => file.kind.as_deref().map(|k| k == "light" || k == "hc-light"),
    };
    let first = |keys: &[&str]| keys.iter().find_map(|k| get(k));
    let guess_back = first(&["editor.background"]);
    let light = said_light.or(guess_back.map(is_light)).unwrap_or(false);
    let back = guess_back.map_or(if light { Color32::WHITE } else { Color32::from_rgb(30, 30, 30) }, |c| over(c, if light { Color32::WHITE } else { Color32::BLACK }));
    let global_text = file.tokens.iter().find(|t| t.get("scope").is_none()).and_then(|t| t.pointer("/settings/foreground")).and_then(Value::as_str).and_then(colour);
    let text = first(&["editor.foreground", "foreground"]).or(global_text).map_or(if light { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(212, 212, 212) }, |c| over(c, back));
    let on_back = |keys: &[&str]| first(keys).map(|c| over(c, back));
    let accent = on_back(&["editorCursor.foreground", "focusBorder", "button.background"]).unwrap_or(text);
    let mut document = theme::document_from(name, light, back, text, accent, scheme_name);
    let set = |slot: &mut Color32, keys: &[&str]| {
        if let Some(found) = on_back(keys) {
            *slot = found;
        }
    };
    set(&mut document.active_line, &["editor.lineHighlightBackground"]);
    set(&mut document.selection, &["editor.selectionBackground"]);
    set(&mut document.highlight_all, &["editor.selectionHighlightBackground", "editor.wordHighlightBackground", "editor.findMatchHighlightBackground"]);
    set(&mut document.other_pane, &["editor.inactiveSelectionBackground"]);
    set(&mut document.whitespace, &["editorWhitespace.foreground"]);
    set(&mut document.mistake, &["editorError.foreground", "errorForeground"]);
    set(&mut document.gutter_back, &["editorGutter.background"]);
    set(&mut document.gutter_text, &["editorLineNumber.foreground"]);
    set(&mut document.ruler_text, &["editorLineNumber.foreground"]);
    set(&mut document.ruler_mark, &["editorLineNumber.activeForeground", "editorCursor.foreground"]);
    set(&mut document.change_unsaved, &["editorGutter.modifiedBackground"]);
    set(&mut document.change_saved, &["editorGutter.addedBackground"]);
    set(&mut document.map_back, &["minimap.background"]);
    set(&mut document.map_view, &["minimapSlider.background", "scrollbarSlider.background"]);
    set(&mut document.strong_line, &["editorRuler.foreground", "editorIndentGuide.background", "editorIndentGuide.background1"]);
    set(&mut document.compare_removed, &["diffEditor.removedLineBackground", "diffEditor.removedTextBackground"]);
    set(&mut document.compare_added, &["diffEditor.insertedLineBackground", "diffEditor.insertedTextBackground"]);
    set(&mut document.compare_changed, &["editorGutter.modifiedBackground"]);
    // A changed line is shaded, not coloured outright.
    document.compare_changed = mix(back, document.compare_changed, 0.3);
    for (slot, n) in [(&mut document.rainbow_1, 1), (&mut document.rainbow_2, 2), (&mut document.rainbow_3, 3), (&mut document.rainbow_4, 4), (&mut document.rainbow_5, 5), (&mut document.rainbow_6, 6)] {
        if let Some(found) = on_back(&[&format!("editorBracketHighlight.foreground{n}")]) {
            *slot = found;
        }
    }
    for (slot, key) in [(&mut document.tag_1, "terminal.ansiRed"), (&mut document.tag_3, "terminal.ansiYellow"), (&mut document.tag_4, "terminal.ansiGreen"), (&mut document.tag_5, "terminal.ansiBlue"), (&mut document.tag_6, "terminal.ansiMagenta")] {
        if let Some(found) = on_back(&[key]) {
            *slot = found;
        }
    }

    // The window: VS Code's side bar is the nearest thing to Limoniter's panels.
    let window_back = first(&["sideBar.background", "editor.background"]).map_or(back, |c| over(c, back));
    let on_window = |keys: &[&str]| first(keys).map(|c| over(c, window_back));
    let window_text = on_window(&["sideBar.foreground", "foreground", "editor.foreground"]).unwrap_or(text);
    let window_accent = on_window(&["focusBorder", "button.background", "activityBarBadge.background", "textLink.foreground"]).unwrap_or(accent);
    let mut window = theme::window_from(name, light, window_back, window_text, window_accent);
    let set = |slot: &mut Color32, keys: &[&str]| {
        if let Some(found) = on_window(keys) {
            *slot = found;
        }
    };
    set(&mut window.ribbon, &["titleBar.activeBackground", "editorGroupHeader.tabsBackground", "activityBar.background"]);
    set(&mut window.border, &["panel.border", "sideBar.border", "editorGroup.border", "contrastBorder"]);
    set(&mut window.hover, &["list.hoverBackground"]);
    set(&mut window.selected, &["list.activeSelectionBackground", "list.inactiveSelectionBackground"]);
    set(&mut window.field, &["input.background", "dropdown.background"]);
    set(&mut window.text_dim, &["descriptionForeground"]);
    set(&mut window.text_disabled, &["disabledForeground"]);
    set(&mut window.rainbow_mistake, &["editorWarning.foreground", "errorForeground"]);
    // A see-through line between panels, on a window much like it, would vanish: it's kept from being too faint.
    if contrast(window.border, window.window) < 1.25 {
        window.border = mix(window.window, window.text, 0.22);
    }

    let scheme = scheme(file, scheme_name, back, text, &document);
    Converted { document, window, scheme }
}

fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn syntect_colour(c: Color32) -> Color {
    Color { r: c.r(), g: c.g(), b: c.b(), a: 255 }
}

/// The syntax colour scheme: a syntect theme from the token colours, each scope selector with its colour and style.
fn scheme(file: &ThemeFile, name: &str, back: Color32, text: Color32, document: &DocumentTheme) -> Theme {
    let settings = ThemeSettings {
        foreground: Some(syntect_colour(text)),
        background: Some(syntect_colour(back)),
        caret: Some(syntect_colour(document.caret)),
        selection: Some(syntect_colour(document.selection)),
        line_highlight: Some(syntect_colour(document.active_line)),
        ..ThemeSettings::default()
    };
    let mut scopes = Vec::new();
    for token in &file.tokens {
        let selector = match token.get("scope") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            _ => continue,
        };
        let Ok(scope) = ScopeSelectors::from_str(&selector) else {
            continue;
        };
        let style = token.get("settings");
        let colour_of = |key: &str| style.and_then(|s| s.get(key)).and_then(Value::as_str).and_then(colour).map(|c| syntect_colour(over(c, back)));
        let font_style = style.and_then(|s| s.get("fontStyle")).and_then(Value::as_str).map(|words| {
            let mut font = FontStyle::empty();
            for word in words.split_whitespace() {
                match word {
                    "bold" => font |= FontStyle::BOLD,
                    "italic" => font |= FontStyle::ITALIC,
                    "underline" => font |= FontStyle::UNDERLINE,
                    _ => {}
                }
            }
            font
        });
        let modifier = StyleModifier { foreground: colour_of("foreground"), background: colour_of("background"), font_style };
        if modifier.foreground.is_none() && modifier.background.is_none() && modifier.font_style.is_none() {
            continue;
        }
        scopes.push(ThemeItem { scope, style: modifier });
    }
    Theme { name: Some(name.to_string()), author: None, settings, scopes }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_read_in_every_way_vs_code_writes_them() {
        assert_eq!(colour("#fff"), Some(Color32::WHITE));
        assert_eq!(colour("#102030"), Some(Color32::from_rgb(16, 32, 48)));
        assert_eq!(colour("#10203080").map(|c| c.a()), Some(128));
        assert_eq!(colour("#f008").map(|c| c.to_srgba_unmultiplied()), Some([255, 0, 0, 136]));
        assert_eq!(colour("red"), None);
        // Half see-through white over black is grey.
        assert_eq!(over(Color32::from_rgba_unmultiplied(255, 255, 255, 128), Color32::BLACK), Color32::from_rgb(128, 128, 128));
    }

    fn write(folder: &Path, name: &str, text: &str) -> std::path::PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn a_vs_code_theme_gives_document_window_and_syntax_colours() {
        let folder = std::env::temp_dir().join(format!("limoniter-vsix-theme-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        write(&folder, "base.json", r##"{ "colors": { "editor.foreground": "#eeeeee", "sideBar.background": "#202020" }, "tokenColors": [ { "scope": "comment", "settings": { "foreground": "#6272a4", "fontStyle": "italic" } } ] }"##);
        let path = write(
            &folder,
            "theme.json",
            r##"{
                // Comments are allowed, as VS Code allows them.
                "include": "./base.json",
                "type": "dark",
                "colors": { "editor.background": "#282a36", "editor.selectionBackground": "#44475a", "editorLineNumber.foreground": "#6272a4", "list.hoverBackground": "#ffffff20", },
                "tokenColors": [ { "scope": ["keyword", "storage.type"], "settings": { "foreground": "#ff79c6" } }, { "scope": "string", "settings": { "foreground": "#f1fa8c" } } ]
            }"##,
        );
        let file = read(&path).unwrap();
        let converted = convert(&file, "Demo", "Demo", "vs-dark");
        let _ = std::fs::remove_dir_all(&folder);
        let doc = &converted.document;
        assert!(!doc.light);
        assert_eq!(doc.editor_back, Color32::from_rgb(0x28, 0x2a, 0x36));
        assert_eq!(doc.editor_text, Color32::from_rgb(0xee, 0xee, 0xee), "from the included file");
        assert_eq!(doc.selection, Color32::from_rgb(0x44, 0x47, 0x5a));
        assert_eq!(doc.gutter_text, Color32::from_rgb(0x62, 0x72, 0xa4));
        assert_eq!(doc.syntax, "Demo");
        let window = &converted.window;
        assert_eq!(window.window, Color32::from_rgb(0x20, 0x20, 0x20));
        assert_eq!(window.hover, over(Color32::from_rgba_unmultiplied(255, 255, 255, 0x20), window.window), "see-through, laid over the window");
        // The token colours colour code as they would in VS Code.
        let scheme = &converted.scheme;
        assert_eq!(scheme.scopes.len(), 3);
        let highlighter = syntect::highlighting::Highlighter::new(scheme);
        let style_of = |scope: &str| highlighter.style_for_stack(&[syntect::parsing::Scope::new(scope).unwrap()]);
        assert_eq!(style_of("keyword.control.rust").foreground, Color { r: 0xff, g: 0x79, b: 0xc6, a: 255 });
        assert_eq!(style_of("storage.type.rust").foreground, Color { r: 0xff, g: 0x79, b: 0xc6, a: 255 });
        assert_eq!(style_of("comment.line").foreground, Color { r: 0x62, g: 0x72, b: 0xa4, a: 255 });
        assert!(style_of("comment.line").font_style.contains(FontStyle::ITALIC));
        assert_eq!(style_of("variable").foreground, Color { r: 0xee, g: 0xee, b: 0xee, a: 255 }, "otherwise the text colour");
    }

    #[test]
    fn a_tm_theme_is_read_too() {
        let folder = std::env::temp_dir().join(format!("limoniter-vsix-tmtheme-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = write(
            &folder,
            "old.tmTheme",
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>name</key><string>Old</string><key>settings</key><array>
                <dict><key>settings</key><dict><key>background</key><string>#FDF6E3</string><key>foreground</key><string>#586E75</string></dict></dict>
                <dict><key>scope</key><string>keyword</string><key>settings</key><dict><key>foreground</key><string>#859900</string></dict></dict>
            </array></dict></plist>"#,
        );
        let file = read(&path).unwrap();
        let _ = std::fs::remove_dir_all(&folder);
        let converted = convert(&file, "Old", "Old", "");
        assert!(converted.document.light, "worked out from its background");
        assert_eq!(converted.document.editor_back, Color32::from_rgb(0xfd, 0xf6, 0xe3));
        assert_eq!(converted.scheme.scopes.len(), 1);
    }
}
