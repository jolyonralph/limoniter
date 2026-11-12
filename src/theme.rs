//! The colours used across the window, in one place: a dark set and a light set.

use eframe::egui::{self, Color32, Context, CornerRadius, Stroke};
use std::cell::Cell;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

macro_rules! palette {
    ($($name:ident: $dark:expr, $light:expr;)*) => {
        struct Palette {
            rainbow: [Color32; 6],
            is_light: bool,
            $($name: Color32,)*
        }
        static DARK: Palette = Palette { rainbow: DARK_RAINBOW, is_light: false, $($name: $dark,)* };
        static LIGHT: Palette = Palette { rainbow: LIGHT_RAINBOW, is_light: true, $($name: $light,)* };
        $(pub fn $name() -> Color32 { current().$name })*
    };
}

palette! {
    // Limoniter's colours: weathered iron browns with a rust-orange accent at night; ochre-tinted parchment with a
    // deep rust accent by day.
    window: rgb(28, 22, 18), rgb(245, 238, 227);
    ribbon: rgb(36, 28, 22), rgb(236, 225, 208);
    border: rgb(70, 54, 42), rgb(214, 194, 166);
    hover: rgb(54, 41, 31), rgb(229, 212, 187);
    selected: rgb(84, 48, 22), rgb(245, 212, 172);
    accent: rgb(230, 132, 52), rgb(160, 70, 12);
    text: rgb(238, 229, 218), rgb(40, 28, 20);
    strong_text: rgb(255, 255, 255), rgb(0, 0, 0);
    text_dim: rgb(186, 170, 152), rgb(94, 74, 58);
    text_faint: rgb(152, 134, 116), rgb(112, 94, 78);
    text_disabled: rgb(102, 88, 74), rgb(176, 160, 140);

    editor_back: rgb(22, 17, 14), rgb(254, 251, 245);
    editor_text: rgb(234, 225, 214), rgb(36, 26, 18);
    active_line: rgb(33, 26, 20), rgb(248, 240, 226);
    gutter_back: rgb(22, 17, 14), rgb(254, 251, 245);
    gutter_text: rgb(132, 112, 92), rgb(132, 108, 84);
    ruler_back: rgb(30, 23, 18), rgb(246, 238, 225);
    ruler_text: rgb(156, 124, 96), rgb(122, 88, 60);
    ruler_mark: rgb(230, 132, 52), rgb(160, 70, 12);
    selection: rgb(96, 56, 26), rgb(248, 210, 166);
    highlight_all: rgb(68, 62, 30), rgb(255, 236, 170);
    other_pane: rgb(86, 70, 30), rgb(248, 224, 170);
    caret: rgb(255, 176, 96), rgb(120, 46, 6);
    overwrite_caret: Color32::from_rgba_premultiplied(110, 60, 20, 90), Color32::from_rgba_premultiplied(70, 30, 4, 80);
    zero_byte: rgb(124, 108, 92), rgb(172, 156, 136);
    strong_line: rgb(70, 54, 42), rgb(214, 194, 166);
    whitespace: rgb(96, 78, 62), rgb(196, 180, 160);
    rainbow_mistake: rgb(255, 96, 96), rgb(200, 30, 30);
    token_back: rgb(62, 45, 32), rgb(242, 226, 204);
    token_undefined: rgb(96, 46, 48), rgb(246, 210, 206);
    token_state: rgb(74, 64, 30), rgb(240, 226, 176);
    token_text: rgb(236, 224, 210), rgb(54, 34, 20);
    sequence: rgb(110, 200, 160), rgb(16, 116, 80);
    map_back: rgb(26, 20, 16), rgb(248, 241, 230);
    map_view: rgb(62, 47, 35), rgb(228, 212, 188);
    map_text: rgb(144, 126, 108), rgb(136, 116, 96);
    compare_changed: rgb(70, 62, 30), rgb(255, 244, 196);
    compare_removed: rgb(84, 40, 44), rgb(252, 216, 214);
    compare_added: rgb(30, 70, 54), rgb(210, 242, 226);
    change_unsaved: rgb(240, 170, 60), rgb(196, 118, 0);
    change_saved: rgb(80, 190, 130), rgb(30, 140, 80);
}

/// One colour per nesting level for rainbow parentheses, readable on the editor's background.
const DARK_RAINBOW: [Color32; 6] = [rgb(255, 215, 0), rgb(218, 112, 214), rgb(23, 159, 255), rgb(139, 209, 124), rgb(255, 166, 87), rgb(86, 212, 221)];
const LIGHT_RAINBOW: [Color32; 6] = [rgb(166, 124, 0), rgb(160, 40, 160), rgb(0, 100, 200), rgb(30, 130, 50), rgb(190, 90, 0), rgb(0, 130, 140)];

thread_local! {
    /// Everything is drawn on one thread, so this is only ever read there.
    static CURRENT: Cell<&'static Palette> = const { Cell::new(&DARK) };
}

fn current() -> &'static Palette {
    CURRENT.with(Cell::get)
}

/// The colours sections of a file can be tagged with, by name.
pub const TAG_NAMES: [&str; 6] = ["Red", "Orange", "Yellow", "Green", "Blue", "Purple"];
const DARK_TAGS: [Color32; 6] = [rgb(232, 92, 92), rgb(238, 150, 62), rgb(226, 200, 72), rgb(92, 192, 112), rgb(84, 152, 242), rgb(178, 122, 232)];
const LIGHT_TAGS: [Color32; 6] = [rgb(204, 52, 52), rgb(214, 116, 20), rgb(184, 150, 0), rgb(36, 146, 70), rgb(30, 104, 210), rgb(132, 72, 196)];

pub fn tag_colour(colour: u8) -> Color32 {
    let set = if is_light() { LIGHT_TAGS } else { DARK_TAGS };
    set[colour as usize % set.len()]
}

pub fn rainbow() -> [Color32; 6] {
    current().rainbow
}

pub fn is_light() -> bool {
    current().is_light
}

pub fn set_light(light: bool) {
    CURRENT.with(|cell| cell.set(if light { &LIGHT } else { &DARK }));
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
    visuals.extreme_bg_color = editor_back();
    visuals.override_text_color = Some(text());
    visuals.selection.bg_fill = selection();
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
    fn words_are_readable_in_both_themes() {
        for light in [false, true] {
            set_light(light);
            for (name, words, back, least) in [
                ("window text", text(), window(), 7.0),
                ("ribbon text", text(), ribbon(), 7.0),
                ("editor text", editor_text(), editor_back(), 7.0),
                ("editor text on the active line", editor_text(), active_line(), 7.0),
                ("selected editor text", editor_text(), selection(), 4.5),
                ("dim text", text_dim(), window(), 4.5),
                ("line numbers", gutter_text(), gutter_back(), 3.0),
                ("ruler", ruler_text(), ruler_back(), 3.0),
                ("tokens", token_text(), token_back(), 4.5),
                ("the caret", caret(), editor_back(), 7.0),
            ] {
                assert!(contrast(words, back) >= least, "{name} with light={light}: {}", contrast(words, back));
            }
            for colour in rainbow() {
                assert!(contrast(colour, editor_back()) >= 3.0, "rainbow {colour:?} with light={light}");
            }
        }
        set_light(false);
    }

    #[test]
    fn the_accent_shows_on_every_background() {
        for light in [false, true] {
            set_light(light);
            for back in [window(), ribbon(), editor_back()] {
                assert!(contrast(accent(), back) >= 3.0, "with light={light}");
            }
        }
        set_light(false);
    }
}
