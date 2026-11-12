//! The colours used across the window, in one place. A window theme colours the menus, panels and boxes; a
//! document theme colours the text and everything drawn with it, and can differ from one document to the next.

use eframe::egui::{self, Color32, Context, CornerRadius, Stroke};
use std::cell::RefCell;
use std::rc::Rc;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// A colour part way from one to another: none of the way at 0, all of it at 1.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(channel(a.r(), b.r()), channel(a.g(), b.g()), channel(a.b(), b.b()), channel(a.a(), b.a()))
}

/// A colour as a theme file writes it: #rrggbb, or #rrggbbaa if it's see-through.
pub fn to_hex(colour: Color32) -> String {
    let [r, g, b, a] = colour.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

pub fn from_hex(text: &str) -> Option<Color32> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
        _ => None,
    }
}

/// A set of named colours, with the key each has in a theme file, its name, and the group it's shown under.
macro_rules! colour_set {
    ($(#[$meta:meta])* $set:ident, $cell:ident, $kind:literal, { $($xname:ident: $xty:ty),* }, $($group:literal => [$($name:ident: $label:literal),* $(,)?]),* $(,)?) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $set {
            pub name: String,
            /// A light theme or a dark one, which decides egui's own look and shadows.
            pub light: bool,
            $(pub $xname: $xty,)*
            $($(pub $name: Color32,)*)*
        }

        impl $set {
            /// What a theme file says it is.
            pub const KIND: &'static str = $kind;
            pub const COLOURS: &'static [(&'static str, &'static str, &'static str)] = &[$($((stringify!($name), $label, $group),)*)*];

            pub fn colour(&self, key: &str) -> Option<Color32> {
                match key {
                    $($(stringify!($name) => Some(self.$name),)*)*
                    _ => None,
                }
            }

            pub fn colour_mut(&mut self, key: &str) -> Option<&mut Color32> {
                match key {
                    $($(stringify!($name) => Some(&mut self.$name),)*)*
                    _ => None,
                }
            }
        }

        $($(#[allow(dead_code, reason = "every colour has a getter, though not every one is drawn by name")]
        pub fn $name() -> Color32 {
            $cell.with(|theme| theme.borrow().$name)
        })*)*
    };
}

colour_set! {
    /// The window's colours: menus, panels, boxes and buttons.
    WindowTheme, WINDOW, "window", {},
    "Window" => [window: "Window", ribbon: "Bars and buttons", border: "Borders", hover: "Under the pointer", selected: "Chosen item", field: "Boxes to type in"],
    "Words" => [text: "Text", strong_text: "Strong text", text_dim: "Quieter text", text_faint: "Faint text", text_disabled: "Unavailable", accent: "Accent", rainbow_mistake: "Warnings"],
}

colour_set! {
    /// A document's colours: the text and everything drawn with it.
    DocumentTheme, DOCUMENT, "document", { syntax: String },
    "Text" => [editor_back: "Background", editor_text: "Text", active_line: "The cursor's line", selection: "Selection", highlight_all: "Other copies of the selection", other_pane: "The other pane's selection", caret: "Cursor", overwrite_caret: "Cursor when overwriting", whitespace: "Spaces, tabs and line ends", mistake: "Mistakes and unmatched brackets"],
    "Margins" => [gutter_back: "Line number margin", gutter_text: "Line numbers", ruler_back: "Ruler", ruler_text: "Ruler numbers", ruler_mark: "Ruler's cursor mark", change_unsaved: "Changed, not saved", change_saved: "Changed and saved", map_back: "Document map", map_view: "Map: what's in view", map_text: "Map: text"],
    "Bytes and codes" => [zero_byte: "Zero bytes in hex", strong_line: "Dividing lines", token_back: "Control codes", token_undefined: "Bytes with no character", token_state: "Character set changes", token_text: "Control codes' names", sequence: "Escape sequences"],
    "Comparing" => [compare_changed: "Changed lines", compare_removed: "Removed lines", compare_added: "Added lines"],
    "Brackets, by depth" => [rainbow_1: "First", rainbow_2: "Second", rainbow_3: "Third", rainbow_4: "Fourth", rainbow_5: "Fifth", rainbow_6: "Sixth"],
    "Colour tags" => [tag_1: "Red", tag_2: "Orange", tag_3: "Yellow", tag_4: "Green", tag_5: "Blue", tag_6: "Purple"],
}

/// Writing and reading a theme as a small text file: its kind, name, light or dark, then one colour a line.
macro_rules! theme_file {
    ($set:ident, $base:expr) => {
        impl $set {
            pub fn to_text(&self) -> String {
                let mut out = format!("kind = {}\nname = {}\nlight = {}\n", Self::KIND, self.name, if self.light { "yes" } else { "no" });
                out += &self.extra_text();
                for (key, _, _) in Self::COLOURS {
                    out += &format!("{key} = {}\n", to_hex(self.colour(key).unwrap_or_default()));
                }
                out
            }

            /// Reads a theme. Colours it doesn't give are taken from the built-in theme of the same lightness, so a
            /// theme made before a colour was added still reads.
            pub fn from_text(text: &str) -> Result<Self, String> {
                let mut values = std::collections::HashMap::new();
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                        continue;
                    }
                    let (key, value) = line.split_once('=').ok_or_else(|| format!("\u{201C}{line}\u{201D} isn't written key = value."))?;
                    values.insert(key.trim().to_string(), value.trim().to_string());
                }
                if values.get("kind").is_some_and(|k| k != Self::KIND) {
                    return Err(format!("That's a {} theme, not a {} theme.", values["kind"], Self::KIND));
                }
                let light = values.get("light").is_some_and(|v| matches!(v.as_str(), "yes" | "true" | "light"));
                let mut theme = $base(light);
                theme.name = values.get("name").cloned().filter(|n| !n.is_empty()).ok_or("The theme has no name.")?;
                theme.light = light;
                theme.read_extra(&values);
                for (key, value) in &values {
                    if let Some(slot) = theme.colour_mut(key) {
                        *slot = from_hex(value).ok_or_else(|| format!("{key}: \u{201C}{value}\u{201D} isn't a colour written #rrggbb."))?;
                    }
                }
                Ok(theme)
            }
        }
    };
}

theme_file!(WindowTheme, |light| if light { limoniter_light_window() } else { limoniter_dark_window() });
theme_file!(DocumentTheme, |light| if light { limoniter_light_document() } else { limoniter_dark_document() });

impl WindowTheme {
    fn extra_text(&self) -> String {
        String::new()
    }

    fn read_extra(&mut self, _values: &std::collections::HashMap<String, String>) {}
}

impl DocumentTheme {
    fn extra_text(&self) -> String {
        format!("syntax = {}\n", self.syntax)
    }

    fn read_extra(&mut self, values: &std::collections::HashMap<String, String>) {
        if let Some(syntax) = values.get("syntax") {
            self.syntax.clone_from(syntax);
        }
    }
}

const RED: Color32 = rgb(220, 50, 47);
const GREEN: Color32 = rgb(80, 190, 110);
const YELLOW: Color32 = rgb(230, 200, 60);
const DARK_RAINBOW: [Color32; 6] = [rgb(255, 215, 0), rgb(218, 112, 214), rgb(23, 159, 255), rgb(139, 209, 124), rgb(255, 166, 87), rgb(86, 212, 221)];
const LIGHT_RAINBOW: [Color32; 6] = [rgb(166, 124, 0), rgb(160, 40, 160), rgb(0, 100, 200), rgb(30, 130, 50), rgb(190, 90, 0), rgb(0, 130, 140)];
const DARK_TAGS: [Color32; 6] = [rgb(232, 92, 92), rgb(238, 150, 62), rgb(226, 200, 72), rgb(92, 192, 112), rgb(84, 152, 242), rgb(178, 122, 232)];
const LIGHT_TAGS: [Color32; 6] = [rgb(204, 52, 52), rgb(214, 116, 20), rgb(184, 150, 0), rgb(36, 146, 70), rgb(30, 104, 210), rgb(132, 72, 196)];

/// A whole window theme from three colours: the window, its text and an accent.
pub fn window_from(name: &str, light: bool, window: Color32, text: Color32, accent: Color32) -> WindowTheme {
    WindowTheme {
        name: name.to_string(),
        light,
        window,
        ribbon: mix(window, text, 0.05),
        border: mix(window, text, 0.22),
        hover: mix(window, text, 0.12),
        selected: mix(window, accent, 0.32),
        field: mix(window, if light { Color32::WHITE } else { Color32::BLACK }, 0.45),
        text,
        strong_text: if light { Color32::BLACK } else { Color32::WHITE },
        text_dim: mix(window, text, 0.75),
        text_faint: mix(window, text, 0.62),
        text_disabled: mix(window, text, 0.38),
        accent,
        rainbow_mistake: if light { rgb(200, 30, 30) } else { rgb(255, 96, 96) },
    }
}

/// A whole document theme from three colours: the background, the text and an accent, with syntax colours by name.
pub fn document_from(name: &str, light: bool, back: Color32, text: Color32, accent: Color32, syntax: &str) -> DocumentTheme {
    let rainbow = if light { LIGHT_RAINBOW } else { DARK_RAINBOW };
    let tags = if light { LIGHT_TAGS } else { DARK_TAGS };
    let [r, g, b, _] = accent.to_array();
    DocumentTheme {
        name: name.to_string(),
        light,
        syntax: syntax.to_string(),
        editor_back: back,
        editor_text: text,
        active_line: mix(back, text, 0.07),
        selection: mix(back, accent, if light { 0.25 } else { 0.28 }),
        highlight_all: mix(back, YELLOW, 0.22),
        other_pane: mix(back, accent, 0.2),
        caret: accent,
        overwrite_caret: Color32::from_rgba_unmultiplied(r, g, b, 90),
        whitespace: mix(back, text, 0.3),
        mistake: if light { rgb(200, 30, 30) } else { rgb(255, 96, 96) },
        gutter_back: back,
        gutter_text: mix(back, text, 0.68),
        ruler_back: mix(back, text, 0.05),
        ruler_text: mix(back, text, 0.72),
        ruler_mark: accent,
        change_unsaved: if light { rgb(196, 118, 0) } else { rgb(240, 170, 60) },
        change_saved: if light { rgb(30, 140, 80) } else { rgb(80, 190, 130) },
        map_back: mix(back, text, 0.03),
        map_view: mix(back, text, 0.14),
        map_text: mix(back, text, 0.5),
        zero_byte: mix(back, text, 0.45),
        strong_line: mix(back, text, 0.22),
        token_back: mix(back, text, 0.1),
        token_undefined: mix(back, RED, 0.3),
        token_state: mix(back, YELLOW, 0.25),
        token_text: text,
        sequence: mix(text, GREEN, 0.55),
        compare_changed: mix(back, YELLOW, 0.22),
        compare_removed: mix(back, RED, 0.25),
        compare_added: mix(back, GREEN, 0.25),
        rainbow_1: rainbow[0],
        rainbow_2: rainbow[1],
        rainbow_3: rainbow[2],
        rainbow_4: rainbow[3],
        rainbow_5: rainbow[4],
        rainbow_6: rainbow[5],
        tag_1: tags[0],
        tag_2: tags[1],
        tag_3: tags[2],
        tag_4: tags[3],
        tag_5: tags[4],
        tag_6: tags[5],
    }
}

// Limoniter's own colours: weathered iron browns with a rust-orange accent at night; ochre-tinted parchment with a
// deep rust accent by day.

pub fn limoniter_dark_window() -> WindowTheme {
    WindowTheme {
        name: "Limoniter Dark".into(),
        light: false,
        window: rgb(28, 22, 18),
        ribbon: rgb(36, 28, 22),
        border: rgb(70, 54, 42),
        hover: rgb(54, 41, 31),
        selected: rgb(84, 48, 22),
        field: rgb(22, 17, 14),
        accent: rgb(230, 132, 52),
        text: rgb(238, 229, 218),
        strong_text: rgb(255, 255, 255),
        text_dim: rgb(186, 170, 152),
        text_faint: rgb(152, 134, 116),
        text_disabled: rgb(102, 88, 74),
        rainbow_mistake: rgb(255, 96, 96),
    }
}

pub fn limoniter_light_window() -> WindowTheme {
    WindowTheme {
        name: "Limoniter Light".into(),
        light: true,
        window: rgb(245, 238, 227),
        ribbon: rgb(236, 225, 208),
        border: rgb(214, 194, 166),
        hover: rgb(229, 212, 187),
        selected: rgb(245, 212, 172),
        field: rgb(254, 251, 245),
        accent: rgb(160, 70, 12),
        text: rgb(40, 28, 20),
        strong_text: rgb(0, 0, 0),
        text_dim: rgb(94, 74, 58),
        text_faint: rgb(112, 94, 78),
        text_disabled: rgb(176, 160, 140),
        rainbow_mistake: rgb(200, 30, 30),
    }
}

pub fn limoniter_dark_document() -> DocumentTheme {
    DocumentTheme {
        name: "Limoniter Dark".into(),
        editor_back: rgb(22, 17, 14),
        editor_text: rgb(234, 225, 214),
        active_line: rgb(33, 26, 20),
        gutter_back: rgb(22, 17, 14),
        gutter_text: rgb(132, 112, 92),
        ruler_back: rgb(30, 23, 18),
        ruler_text: rgb(156, 124, 96),
        ruler_mark: rgb(230, 132, 52),
        selection: rgb(96, 56, 26),
        highlight_all: rgb(68, 62, 30),
        other_pane: rgb(86, 70, 30),
        caret: rgb(255, 176, 96),
        overwrite_caret: Color32::from_rgba_unmultiplied(230, 132, 52, 90),
        zero_byte: rgb(124, 108, 92),
        strong_line: rgb(70, 54, 42),
        whitespace: rgb(96, 78, 62),
        mistake: rgb(255, 96, 96),
        token_back: rgb(62, 45, 32),
        token_undefined: rgb(96, 46, 48),
        token_state: rgb(74, 64, 30),
        token_text: rgb(236, 224, 210),
        sequence: rgb(110, 200, 160),
        map_back: rgb(26, 20, 16),
        map_view: rgb(62, 47, 35),
        map_text: rgb(144, 126, 108),
        compare_changed: rgb(70, 62, 30),
        compare_removed: rgb(84, 40, 44),
        compare_added: rgb(30, 70, 54),
        change_unsaved: rgb(240, 170, 60),
        change_saved: rgb(80, 190, 130),
        ..document_from("", false, rgb(22, 17, 14), rgb(234, 225, 214), rgb(255, 176, 96), "OneHalfDark")
    }
}

pub fn limoniter_light_document() -> DocumentTheme {
    DocumentTheme {
        name: "Limoniter Light".into(),
        editor_back: rgb(254, 251, 245),
        editor_text: rgb(36, 26, 18),
        active_line: rgb(248, 240, 226),
        gutter_back: rgb(254, 251, 245),
        gutter_text: rgb(132, 108, 84),
        ruler_back: rgb(246, 238, 225),
        ruler_text: rgb(122, 88, 60),
        ruler_mark: rgb(160, 70, 12),
        selection: rgb(248, 210, 166),
        highlight_all: rgb(255, 236, 170),
        other_pane: rgb(248, 224, 170),
        caret: rgb(120, 46, 6),
        overwrite_caret: Color32::from_rgba_unmultiplied(160, 70, 12, 80),
        zero_byte: rgb(172, 156, 136),
        strong_line: rgb(214, 194, 166),
        whitespace: rgb(196, 180, 160),
        mistake: rgb(200, 30, 30),
        token_back: rgb(242, 226, 204),
        token_undefined: rgb(246, 210, 206),
        token_state: rgb(240, 226, 176),
        token_text: rgb(54, 34, 20),
        sequence: rgb(16, 116, 80),
        map_back: rgb(248, 241, 230),
        map_view: rgb(228, 212, 188),
        map_text: rgb(136, 116, 96),
        compare_changed: rgb(255, 244, 196),
        compare_removed: rgb(252, 216, 214),
        compare_added: rgb(210, 242, 226),
        change_unsaved: rgb(196, 118, 0),
        change_saved: rgb(30, 140, 80),
        ..document_from("", true, rgb(254, 251, 245), rgb(36, 26, 18), rgb(120, 46, 6), "OneHalfLight")
    }
}

/// The window themes that come with Limoniter.
pub fn built_in_windows() -> Vec<WindowTheme> {
    vec![
        limoniter_dark_window(),
        limoniter_light_window(),
        window_from("Graphite", false, rgb(36, 38, 42), rgb(230, 232, 235), rgb(96, 168, 255)),
        window_from("Paper", true, rgb(246, 246, 243), rgb(30, 30, 32), rgb(36, 98, 200)),
        window_from("Nord", false, rgb(46, 52, 64), rgb(229, 233, 240), rgb(136, 192, 208)),
        window_from("Solarized Light", true, rgb(253, 246, 227), rgb(40, 56, 64), rgb(38, 110, 180)),
        window_from("Workbench", false, rgb(0, 72, 150), rgb(255, 255, 255), rgb(255, 150, 40)),
        window_from("High contrast", false, rgb(0, 0, 0), rgb(255, 255, 255), rgb(255, 230, 0)),
    ]
}

/// The document themes that come with Limoniter, the everyday ones first, then those of older machines.
pub fn built_in_documents() -> Vec<DocumentTheme> {
    vec![
        limoniter_dark_document(),
        limoniter_light_document(),
        document_from("Solarized Dark", false, rgb(0, 43, 54), rgb(147, 161, 161), rgb(181, 137, 0), "Solarized (dark)"),
        document_from("Solarized Light", true, rgb(253, 246, 227), rgb(60, 79, 86), rgb(203, 75, 22), "Solarized (light)"),
        document_from("Nord", false, rgb(46, 52, 64), rgb(216, 222, 233), rgb(136, 192, 208), "Nord"),
        document_from("Gruvbox Dark", false, rgb(40, 40, 40), rgb(235, 219, 178), rgb(254, 128, 25), "gruvbox-dark"),
        document_from("Dracula", false, rgb(40, 42, 54), rgb(248, 248, 242), rgb(189, 147, 249), "Dracula"),
        document_from("Monokai", false, rgb(39, 40, 34), rgb(248, 248, 242), rgb(230, 219, 116), "Monokai Extended"),
        document_from("GitHub Light", true, rgb(255, 255, 255), rgb(36, 41, 47), rgb(9, 105, 218), "GitHub"),
        document_from("High contrast", false, rgb(0, 0, 0), rgb(255, 255, 255), rgb(255, 230, 0), "Monokai Extended Bright"),
        // Older machines: their screens' own colours, made readable for a day's work.
        document_from("Commodore 64", false, rgb(53, 40, 121), rgb(176, 166, 236), rgb(154, 210, 132), "None"),
        document_from("Atari 8-bit", false, rgb(16, 58, 140), rgb(160, 196, 255), rgb(255, 214, 102), "None"),
        document_from("ZX Spectrum", true, rgb(215, 215, 215), rgb(0, 0, 0), rgb(0, 0, 215), "None"),
        document_from("Amiga Workbench", false, rgb(0, 85, 170), rgb(255, 255, 255), rgb(255, 136, 0), "None"),
        document_from("BBC Micro and teletext", false, rgb(0, 0, 0), rgb(255, 255, 255), rgb(255, 255, 0), "None"),
        document_from("DOS", false, rgb(0, 0, 170), rgb(200, 200, 200), rgb(255, 255, 85), "None"),
        document_from("Green screen", false, rgb(8, 18, 10), rgb(80, 255, 120), rgb(180, 255, 190), "None"),
        document_from("Amber screen", false, rgb(18, 11, 0), rgb(255, 176, 0), rgb(255, 214, 140), "None"),
    ]
}

thread_local! {
    /// Everything is drawn on one thread, so these are only ever read there.
    static WINDOW: RefCell<Rc<WindowTheme>> = RefCell::new(Rc::new(limoniter_dark_window()));
    static DOCUMENT: RefCell<Rc<DocumentTheme>> = RefCell::new(Rc::new(limoniter_dark_document()));
}

pub fn set_window(theme: Rc<WindowTheme>) {
    WINDOW.with(|cell| *cell.borrow_mut() = theme);
}

/// The document theme drawn with from now on, until another is set: set for each document as it's drawn.
pub fn set_document(theme: Rc<DocumentTheme>) {
    DOCUMENT.with(|cell| {
        if *cell.borrow() != theme {
            *cell.borrow_mut() = theme;
        }
    });
}

pub fn window_theme() -> Rc<WindowTheme> {
    WINDOW.with(|cell| cell.borrow().clone())
}

#[cfg(test)]
pub fn document_theme() -> Rc<DocumentTheme> {
    DOCUMENT.with(|cell| cell.borrow().clone())
}

/// The colours tags are shown in, by name.
pub const TAG_NAMES: [&str; 6] = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple"];

pub fn tag_colour(colour: u8) -> Color32 {
    DOCUMENT.with(|cell| {
        let t = cell.borrow();
        [t.tag_1, t.tag_2, t.tag_3, t.tag_4, t.tag_5, t.tag_6][colour as usize % 6]
    })
}

/// One colour per nesting level for rainbow brackets.
pub fn rainbow() -> [Color32; 6] {
    DOCUMENT.with(|cell| {
        let t = cell.borrow();
        [t.rainbow_1, t.rainbow_2, t.rainbow_3, t.rainbow_4, t.rainbow_5, t.rainbow_6]
    })
}

/// Whether the window theme is a light one.
pub fn is_light() -> bool {
    WINDOW.with(|cell| cell.borrow().light)
}

/// Whether the document theme is a light one.
pub fn document_is_light() -> bool {
    DOCUMENT.with(|cell| cell.borrow().light)
}

/// The syntax colours the document theme asks for, by name.
pub fn syntax_scheme() -> String {
    DOCUMENT.with(|cell| cell.borrow().syntax.clone())
}

/// Limoniter's own light or dark themes for both, as the tests use.
#[cfg(test)]
pub fn set_light(light: bool) {
    set_window(Rc::new(if light { limoniter_light_window() } else { limoniter_dark_window() }));
    set_document(Rc::new(if light { limoniter_light_document() } else { limoniter_dark_document() }));
}

/// A colour as Windows wants it for its window frame: blue, green, red from the low byte up.
#[cfg(any(windows, test))]
fn windows_colour(colour: Color32) -> u32 {
    u32::from(colour.b()) << 16 | u32::from(colour.g()) << 8 | u32::from(colour.r())
}

/// Windows draws the bar along the top of the window itself. Asking only for dark mode can leave white
/// words on its pale background when Windows is in light mode, so its colours are set outright.
/// Windows 10 ignores the colour settings and keeps the mode's own.
#[cfg(windows)]
pub fn colour_title_bar(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use std::ffi::c_void;

    const USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const BORDER_COLOR: u32 = 34;
    const CAPTION_COLOR: u32 = 35;
    const TEXT_COLOR: u32 = 36;

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmSetWindowAttribute(window: isize, attribute: u32, value: *const c_void, size: u32) -> i32;
    }

    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
    let window = handle.hwnd.get();
    let settings = [
        (USE_IMMERSIVE_DARK_MODE, u32::from(!is_light())),
        (CAPTION_COLOR, windows_colour(self::window_colour())),
        (BORDER_COLOR, windows_colour(border())),
        (TEXT_COLOR, windows_colour(text())),
    ];
    for (attribute, value) in settings {
        // A refusal only means an older Windows, and leaves its own colours in place.
        // SAFETY: the window is alive for this call and each value is the 4 bytes these settings read.
        unsafe {
            DwmSetWindowAttribute(window, attribute, std::ptr::from_ref(&value).cast(), 4);
        }
    }
}

#[cfg(windows)]
fn window_colour() -> Color32 {
    self::window()
}

#[cfg(not(windows))]
pub fn colour_title_bar(_window: &impl raw_window_handle::HasWindowHandle) {}

pub fn apply(ctx: &Context) {
    let (kind, mut visuals) = if is_light() { (egui::Theme::Light, egui::Visuals::light()) } else { (egui::Theme::Dark, egui::Visuals::dark()) };
    ctx.set_theme(kind);
    visuals.panel_fill = window();
    visuals.window_fill = window();
    visuals.faint_bg_color = ribbon();
    visuals.extreme_bg_color = field();
    visuals.override_text_color = Some(text());
    visuals.selection.bg_fill = selected();
    visuals.selection.stroke = Stroke::new(1.0, accent());
    visuals.hyperlink_color = accent();
    visuals.window_stroke = Stroke::new(1.0, border());
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if is_light() { 40 } else { 110 }) };
    visuals.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if is_light() { 34 } else { 96 }) };
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border());
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
    for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        widget.corner_radius = CornerRadius::same(6);
    }
    visuals.widgets.inactive.weak_bg_fill = ribbon();
    visuals.widgets.inactive.bg_fill = ribbon();
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border());
    visuals.widgets.hovered.weak_bg_fill = hover();
    visuals.widgets.hovered.bg_fill = hover();
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, accent());
    visuals.widgets.active.weak_bg_fill = selected();
    visuals.widgets.active.bg_fill = selected();
    visuals.widgets.open.weak_bg_fill = selected();
    ctx.set_visuals_of(kind, visuals);
    ctx.style_mut_of(kind, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.window_margin = egui::Margin::same(14);
        style.spacing.interact_size.y = 22.0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_colours_run_blue_green_red() {
        assert_eq!(windows_colour(Color32::from_rgb(0x12, 0x34, 0x56)), 0x0056_3412);
    }

    fn brightness(colour: Color32) -> f32 {
        let channel = |value: u8| {
            let share = f32::from(value) / 255.0;
            if share <= 0.03928 { share / 12.92 } else { ((share + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
    }

    /// The usual measure of how readable one colour is on another: 1 is none, 21 is black on white.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (brightness(a), brightness(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn words_are_readable_in_every_built_in_theme() {
        for window in built_in_windows() {
            let at_least = if window.name.starts_with("Limoniter") { 7.0 } else { 4.5 };
            for (what, words, back, least) in [
                ("window text", window.text, window.window, at_least),
                ("ribbon text", window.text, window.ribbon, at_least),
                ("dim text", window.text_dim, window.window, 4.5),
                ("faint text", window.text_faint, window.window, 3.0),
                ("text in a box", window.text, window.field, 4.5),
                ("the accent", window.accent, window.window, 3.0),
            ] {
                assert!(contrast(words, back) >= least, "{} {what}: {}", window.name, contrast(words, back));
            }
        }
        for document in built_in_documents() {
            let at_least = if document.name.starts_with("Limoniter") { 7.0 } else { 4.5 };
            for (what, words, back, least) in [
                ("text", document.editor_text, document.editor_back, at_least),
                ("text on the cursor's line", document.editor_text, document.active_line, 4.5),
                ("selected text", document.editor_text, document.selection, 3.0),
                ("line numbers", document.gutter_text, document.gutter_back, 2.5),
                ("ruler", document.ruler_text, document.ruler_back, 2.5),
                ("control code names", document.token_text, document.token_back, 4.5),
                ("the cursor", document.caret, document.editor_back, 3.0),
            ] {
                assert!(contrast(words, back) >= least, "{} {what}: {}", document.name, contrast(words, back));
            }
        }
    }

    #[test]
    fn a_theme_is_written_and_read_back_exactly() {
        for theme in built_in_documents() {
            let back = DocumentTheme::from_text(&theme.to_text()).unwrap();
            assert_eq!(back, theme);
        }
        for theme in built_in_windows() {
            assert_eq!(WindowTheme::from_text(&theme.to_text()).unwrap(), theme);
        }
        // Colours a file leaves out come from Limoniter's own theme, so older theme files still read.
        let short = DocumentTheme::from_text("kind = document\nname = Mine\nlight = no\neditor_back = #102030\n").unwrap();
        assert_eq!(short.editor_back, rgb(0x10, 0x20, 0x30));
        assert_eq!(short.editor_text, limoniter_dark_document().editor_text);
        assert!(WindowTheme::from_text("kind = document\nname = X\n").unwrap_err().contains("document"));
        assert!(DocumentTheme::from_text("kind = document\nname = X\ncaret = blue\n").is_err());
        assert_eq!(from_hex("#11223344"), Some(Color32::from_rgba_unmultiplied(0x11, 0x22, 0x33, 0x44)));
        assert_eq!(to_hex(rgb(1, 2, 3)), "#010203");
    }

    #[test]
    fn the_document_theme_is_its_own() {
        set_light(true);
        set_document(Rc::new(built_in_documents().into_iter().find(|t| t.name == "Commodore 64").unwrap()));
        assert!(is_light(), "the window stays light");
        assert!(!document_is_light());
        assert_eq!(editor_back(), rgb(53, 40, 121));
        assert_eq!(syntax_scheme(), "None", "the machines had no syntax colours");
        assert_eq!(document_theme().name, "Commodore 64");
        set_light(false);
    }
}
