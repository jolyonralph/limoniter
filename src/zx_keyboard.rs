//! The Spectrum's keyboard, drawn under the built-in Spectrum's screen: each key with everything printed on and
//! around it (its keyword, its Symbol Shift symbol in red, and its Extended mode words above and below), so it can
//! be played with the mouse, and the key combinations it takes can be found and clicked. Under OpticOS, which takes
//! keywords letter by letter, the keys show only what OpticOS gives with them.

use crate::zx_machine::BuiltIn;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Ui, vec2};
use rustzx_core::zx::keys::ZXKey;

/// One key: what's on it and around it, as on a 48K Spectrum.
struct Cap {
    key: ZXKey,
    /// The letter or number on it.
    main: &'static str,
    /// The keyword it gives in K mode.
    keyword: &'static str,
    /// What it gives with Symbol Shift.
    symbol: &'static str,
    /// Above it: what it gives in Extended mode (or with Caps Shift, on the number row).
    above: &'static str,
    /// Below it: Extended mode with Symbol Shift.
    below: &'static str,
}

const fn cap(
    key: ZXKey,
    main: &'static str,
    keyword: &'static str,
    symbol: &'static str,
    above: &'static str,
    below: &'static str,
) -> Cap {
    Cap {
        key,
        main,
        keyword,
        symbol,
        above,
        below,
    }
}

const ROWS: [[Cap; 10]; 3] = [
    [
        cap(ZXKey::N1, "1", "EDIT", "!", "BLUE", "DEF FN"),
        cap(ZXKey::N2, "2", "CAPS LOCK", "@", "RED", "FN"),
        cap(ZXKey::N3, "3", "TRUE VID", "#", "MAGENTA", "LINE"),
        cap(ZXKey::N4, "4", "INV VID", "$", "GREEN", "OPEN #"),
        cap(ZXKey::N5, "5", "\u{2190}", "%", "CYAN", "CLOSE #"),
        cap(ZXKey::N6, "6", "\u{2193}", "&", "YELLOW", "MOVE"),
        cap(ZXKey::N7, "7", "\u{2191}", "'", "WHITE", "ERASE"),
        cap(ZXKey::N8, "8", "\u{2192}", "(", "", "POINT"),
        cap(ZXKey::N9, "9", "GRAPHICS", ")", "", "CAT"),
        cap(ZXKey::N0, "0", "DELETE", "_", "BLACK", "FORMAT"),
    ],
    [
        cap(ZXKey::Q, "Q", "PLOT", "<=", "SIN", "ASN"),
        cap(ZXKey::W, "W", "DRAW", "<>", "COS", "ACS"),
        cap(ZXKey::E, "E", "REM", ">=", "TAN", "ATN"),
        cap(ZXKey::R, "R", "RUN", "<", "INT", "VERIFY"),
        cap(ZXKey::T, "T", "RAND", ">", "RND", "MERGE"),
        cap(ZXKey::Y, "Y", "RETURN", "AND", "STR$", "["),
        cap(ZXKey::U, "U", "IF", "OR", "CHR$", "]"),
        cap(ZXKey::I, "I", "INPUT", "AT", "CODE", "IN"),
        cap(ZXKey::O, "O", "POKE", ";", "PEEK", "OUT"),
        cap(ZXKey::P, "P", "PRINT", "\"", "TAB", "\u{A9}"),
    ],
    [
        cap(ZXKey::A, "A", "NEW", "STOP", "READ", "~"),
        cap(ZXKey::S, "S", "SAVE", "NOT", "RESTORE", "|"),
        cap(ZXKey::D, "D", "DIM", "STEP", "DATA", "\\"),
        cap(ZXKey::F, "F", "FOR", "TO", "SGN", "{"),
        cap(ZXKey::G, "G", "GO TO", "THEN", "ABS", "}"),
        cap(ZXKey::H, "H", "GO SUB", "\u{2191}", "SQR", "CIRCLE"),
        cap(ZXKey::J, "J", "LOAD", "-", "VAL", "VAL$"),
        cap(ZXKey::K, "K", "LIST", "+", "LEN", "SCREEN$"),
        cap(ZXKey::L, "L", "LET", "=", "USR", "ATTR"),
        cap(ZXKey::Enter, "ENTER", "", "", "", ""),
    ],
];

const BOTTOM: [Cap; 10] = [
    cap(ZXKey::Shift, "CAPS SHIFT", "", "", "", ""),
    cap(ZXKey::Z, "Z", "COPY", ":", "LN", "BEEP"),
    cap(ZXKey::X, "X", "CLEAR", "\u{A3}", "EXP", "INK"),
    cap(ZXKey::C, "C", "CONT", "?", "LPRINT", "PAPER"),
    cap(ZXKey::V, "V", "CLS", "/", "LLIST", "FLASH"),
    cap(ZXKey::B, "B", "BORDER", "*", "BIN", "BRIGHT"),
    cap(ZXKey::N, "N", "NEXT", ",", "INKEY$", "OVER"),
    cap(ZXKey::M, "M", "PAUSE", ".", "PI", "INVERSE"),
    cap(ZXKey::SymShift, "SYMBOL SHIFT", "", "", "", ""),
    cap(ZXKey::Space, "BREAK SPACE", "", "", "", ""),
];

/// OpticOS's keyboard (its README's and specification's key table, and the key tables in its ROM): letters,
/// digits and Symbol Shift's symbols, with no keywords. Caps Shift on the number row gives EDIT, CAPS LOCK, the
/// cursor keys and DELETE; its Extended mode, once, gives [ ] { } \ | ~ on Y U F G D S A, drawn above them.
const OPTICOS_ROWS: [[Cap; 10]; 3] = [
    [
        cap(ZXKey::N1, "1", "EDIT", "!", "", ""),
        cap(ZXKey::N2, "2", "CAPS LOCK", "@", "", ""),
        cap(ZXKey::N3, "3", "", "#", "", ""),
        cap(ZXKey::N4, "4", "", "$", "", ""),
        cap(ZXKey::N5, "5", "\u{2190}", "%", "", ""),
        cap(ZXKey::N6, "6", "\u{2193}", "&", "", ""),
        cap(ZXKey::N7, "7", "\u{2191}", "'", "", ""),
        cap(ZXKey::N8, "8", "\u{2192}", "(", "", ""),
        cap(ZXKey::N9, "9", "", ")", "", ""),
        cap(ZXKey::N0, "0", "DELETE", "_", "", ""),
    ],
    [
        cap(ZXKey::Q, "Q", "", "", "", ""),
        cap(ZXKey::W, "W", "", "", "", ""),
        cap(ZXKey::E, "E", "", "", "", ""),
        cap(ZXKey::R, "R", "", "<", "", ""),
        cap(ZXKey::T, "T", "", ">", "", ""),
        cap(ZXKey::Y, "Y", "", "", "[", ""),
        cap(ZXKey::U, "U", "", "", "]", ""),
        cap(ZXKey::I, "I", "", "", "", ""),
        cap(ZXKey::O, "O", "", ";", "", ""),
        cap(ZXKey::P, "P", "", "\"", "", ""),
    ],
    [
        cap(ZXKey::A, "A", "", "", "~", ""),
        cap(ZXKey::S, "S", "", "", "|", ""),
        cap(ZXKey::D, "D", "", "", "\\", ""),
        cap(ZXKey::F, "F", "", "", "{", ""),
        cap(ZXKey::G, "G", "", "", "}", ""),
        cap(ZXKey::H, "H", "", "^", "", ""),
        cap(ZXKey::J, "J", "", "-", "", ""),
        cap(ZXKey::K, "K", "", "+", "", ""),
        cap(ZXKey::L, "L", "", "=", "", ""),
        cap(ZXKey::Enter, "ENTER", "", "", "", ""),
    ],
];

const OPTICOS_BOTTOM: [Cap; 10] = [
    cap(ZXKey::Shift, "CAPS SHIFT", "", "", "", ""),
    cap(ZXKey::Z, "Z", "", ":", "", ""),
    cap(ZXKey::X, "X", "", "`", "", ""),
    cap(ZXKey::C, "C", "", "?", "", ""),
    cap(ZXKey::V, "V", "", "/", "", ""),
    cap(ZXKey::B, "B", "", "*", "", ""),
    cap(ZXKey::N, "N", "", ",", "", ""),
    cap(ZXKey::M, "M", "", ".", "", ""),
    cap(ZXKey::SymShift, "SYMBOL SHIFT", "", "", "", ""),
    cap(ZXKey::Space, "BREAK SPACE", "", "", "", ""),
];

/// What a button under the keys does: presses keys together, or types text a key at a time.
enum Action {
    Keys(&'static [ZXKey]),
    Type(&'static str),
}

/// Combinations with names, as buttons under the keys: what they are, and what they press or type.
const COMBINATIONS: [(&str, &str, Action); 9] = [
    (
        "Break",
        "Caps Shift and Space: stops a program, or a LOAD",
        Action::Keys(&[ZXKey::Shift, ZXKey::Space]),
    ),
    ("Delete", "Caps Shift and 0", Action::Keys(&[ZXKey::Shift, ZXKey::N0])),
    (
        "Edit",
        "Caps Shift and 1: brings the current line down to change",
        Action::Keys(&[ZXKey::Shift, ZXKey::N1]),
    ),
    ("Caps Lock", "Caps Shift and 2", Action::Keys(&[ZXKey::Shift, ZXKey::N2])),
    (
        "Graphics",
        "Caps Shift and 9: block graphics, and user-defined graphics on A to U",
        Action::Keys(&[ZXKey::Shift, ZXKey::N9]),
    ),
    (
        "Extended",
        "Caps Shift and Symbol Shift together: the next key gives the green word above it, or with Symbol Shift the red word below",
        Action::Keys(&[ZXKey::Shift, ZXKey::SymShift]),
    ),
    ("True video", "Caps Shift and 3", Action::Keys(&[ZXKey::Shift, ZXKey::N3])),
    (
        "Inverse video",
        "Caps Shift and 4",
        Action::Keys(&[ZXKey::Shift, ZXKey::N4]),
    ),
    (
        "LOAD \"\"",
        "J, then Symbol Shift and P twice, then Enter",
        Action::Type("J\"\"\n"),
    ),
];

/// OpticOS's: its keys, and LOAD "" typed out, since it takes keywords letter by letter.
const OPTICOS_COMBINATIONS: [(&str, &str, Action); 7] = [
    (
        "Break",
        "Caps Shift and Space: stops a program",
        Action::Keys(&[ZXKey::Shift, ZXKey::Space]),
    ),
    (
        "Delete",
        "Caps Shift and 0: deletes the character left of the cursor",
        Action::Keys(&[ZXKey::Shift, ZXKey::N0]),
    ),
    (
        "Edit",
        "Caps Shift and 1: copies the current line into the input line to change; again, empties it",
        Action::Keys(&[ZXKey::Shift, ZXKey::N1]),
    ),
    ("Caps Lock", "Caps Shift and 2", Action::Keys(&[ZXKey::Shift, ZXKey::N2])),
    (
        "Line up",
        "Caps Shift and 7: moves the current-line marker up, and lists around it",
        Action::Keys(&[ZXKey::Shift, ZXKey::N7]),
    ),
    (
        "Line down",
        "Caps Shift and 6: moves the current-line marker down, and lists around it",
        Action::Keys(&[ZXKey::Shift, ZXKey::N6]),
    ),
    ("LOAD \"\"", "Types LOAD \"\" letter by letter, then Enter", Action::Type("load \"\"\n")),
];

const KEY_FILL: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3e);
const KEY_DOWN: Color32 = Color32::from_rgb(0x6a, 0x6a, 0x72);
const LATCHED: Color32 = Color32::from_rgb(0x2f, 0x5f, 0x9f);
const BODY: Color32 = Color32::from_rgb(0x16, 0x16, 0x18);
const WHITE: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);
const RED: Color32 = Color32::from_rgb(0xff, 0x55, 0x55);
const GREEN: Color32 = Color32::from_rgb(0x55, 0xdd, 0x55);

/// The widest it's drawn.
const MOST_WIDTH: f32 = 720.0;

/// For a width: how far apart the keys are, the size of the words around them, how tall a key is, and a row.
fn sizes(width: f32) -> (f32, f32, f32, f32) {
    let pitch = width / 10.0;
    let small = (pitch * 0.15).clamp(6.5, 10.0);
    let key_height = (pitch * 0.62).clamp(22.0, 40.0);
    // Each row: a line for the words above, the key, a line for the words below, and a gap.
    (
        pitch,
        small,
        key_height,
        small * 1.25 * 2.0 + key_height + 10.0,
    )
}

/// How tall the keyboard is at a width, with its buttons for combinations.
pub fn height(width: f32) -> f32 {
    sizes(width.min(MOST_WIDTH)).3 * 4.0 + 6.0 + 4.0 + 52.0
}

/// Draws the keyboard and plays what's clicked on it. Says whether it was used, so the screen can take the
/// keyboard back for typing.
pub fn draw(ui: &mut Ui, machine: &mut BuiltIn) -> bool {
    let mut used = false;
    let width = ui.available_width().min(MOST_WIDTH);
    let (pitch, small, key_height, row_height) = sizes(width);
    let (whole, _) = ui.allocate_exact_size(vec2(width, row_height * 4.0 + 6.0), Sense::hover());
    ui.painter().rect_filled(whole, 4.0, BODY);
    let down_now = ui.input(|i| i.pointer.primary_down());
    let mut pressed: Option<ZXKey> = None;
    let opticos = machine.is_opticos();
    let (rows, bottom, combinations) = if opticos {
        (&OPTICOS_ROWS, &OPTICOS_BOTTOM, &OPTICOS_COMBINATIONS[..])
    } else {
        (&ROWS, &BOTTOM, &COMBINATIONS[..])
    };
    for (row, caps) in rows.iter().chain(std::iter::once(bottom)).enumerate() {
        let top = whole.top() + 3.0 + row as f32 * row_height;
        for (column, cap) in caps.iter().enumerate() {
            let left = whole.left() + column as f32 * pitch;
            let key_rect = Rect::from_min_size(
                Pos2::new(left + 3.0, top + small * 1.25),
                vec2(pitch - 6.0, key_height),
            );
            let id = ui.scope_id().with(("zx-key", row, column));
            let response = ui.interact(
                key_rect.expand2(vec2(2.0, small * 1.25)),
                id,
                Sense::click_and_drag(),
            );
            let held = response.is_pointer_button_down_on() && down_now;
            if response.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
                pressed = Some(cap.key);
            }
            let latched = (matches!(cap.key, ZXKey::Shift) && machine.caps_latched)
                || (matches!(cap.key, ZXKey::SymShift) && machine.symbol_latched);
            let fill = if held {
                KEY_DOWN
            } else if latched {
                LATCHED
            } else {
                KEY_FILL
            };
            let painter = ui.painter();
            painter.rect_filled(key_rect, 3.0, fill);
            let font = |size: f32| FontId::proportional(size);
            if !cap.above.is_empty() {
                let colour = if row == 0 && !opticos && cap.above != "BLACK" && column < 7 {
                    colour_of(column)
                } else {
                    GREEN
                };
                painter.text(
                    Pos2::new(key_rect.center().x, key_rect.top() - 1.0),
                    egui::Align2::CENTER_BOTTOM,
                    cap.above,
                    font(small),
                    colour,
                );
            }
            if !cap.below.is_empty() {
                painter.text(
                    Pos2::new(key_rect.center().x, key_rect.bottom() + 1.0),
                    egui::Align2::CENTER_TOP,
                    cap.below,
                    font(small),
                    RED,
                );
            }
            if named(cap.key) {
                // Enter, the shifts and Space: their names, in the middle.
                let colour = if matches!(cap.key, ZXKey::SymShift) {
                    RED
                } else {
                    WHITE
                };
                let words: Vec<&str> = cap.main.split(' ').collect();
                let line = small * 1.15;
                let first = key_rect.center().y - line * (words.len() as f32 - 1.0) / 2.0;
                for (n, word) in words.iter().enumerate() {
                    painter.text(
                        Pos2::new(key_rect.center().x, first + n as f32 * line),
                        egui::Align2::CENTER_CENTER,
                        *word,
                        font(small),
                        colour,
                    );
                }
            } else {
                painter.text(
                    key_rect.left_top() + vec2(4.0, 2.0),
                    egui::Align2::LEFT_TOP,
                    cap.main,
                    font((key_height * 0.42).min(15.0)),
                    WHITE,
                );
                painter.text(
                    key_rect.right_top() + vec2(-3.0, 2.0),
                    egui::Align2::RIGHT_TOP,
                    cap.symbol,
                    font(small * 1.05),
                    RED,
                );
                painter.text(
                    Pos2::new(key_rect.center().x, key_rect.bottom() - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    cap.keyword,
                    font(small),
                    WHITE,
                );
            }
            let tip = describe(cap, opticos);
            let label = format!("Spectrum key {}", tip.lines().next().unwrap_or(cap.main));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
            response.on_hover_text(tip);
        }
    }
    if let Some(key) = pressed {
        used = true;
        click(machine, key);
    }
    if machine.board_held() && !down_now {
        machine.board_up();
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (name, how, action) in combinations {
            if ui.small_button(*name).on_hover_text(*how).clicked() {
                used = true;
                press(machine, action);
            }
        }
    });
    used
}

/// A button under the keys clicked: its keys pressed together, or its text typed.
fn press(machine: &mut BuiltIn, action: &Action) {
    match action {
        Action::Type(text) => {
            for c in text.chars() {
                machine.type_char(c);
            }
        }
        Action::Keys(keys) => {
            machine.caps_latched = false;
            machine.symbol_latched = false;
            machine.board_down(keys.to_vec());
            machine.board_up();
        }
    }
}

/// The keys with their names written across them, rather than a letter or digit with words around it.
fn named(key: ZXKey) -> bool {
    matches!(key, ZXKey::Enter | ZXKey::Shift | ZXKey::SymShift | ZXKey::Space)
}

/// A key clicked: Caps Shift and Symbol Shift stay down for the next key, as on a phone's keyboard; both at once
/// is Extended mode. Any other key goes down with the shifts waiting, and the shifts are let go.
fn click(machine: &mut BuiltIn, key: ZXKey) {
    match key {
        ZXKey::Shift if machine.symbol_latched => extended(machine),
        ZXKey::SymShift if machine.caps_latched => extended(machine),
        ZXKey::Shift => machine.caps_latched = !machine.caps_latched,
        ZXKey::SymShift => machine.symbol_latched = !machine.symbol_latched,
        _ => {
            let mut keys = vec![key];
            if machine.caps_latched {
                keys.insert(0, ZXKey::Shift);
            }
            if machine.symbol_latched {
                keys.insert(0, ZXKey::SymShift);
            }
            machine.caps_latched = false;
            machine.symbol_latched = false;
            machine.board_down(keys);
        }
    }
}

fn extended(machine: &mut BuiltIn) {
    machine.caps_latched = false;
    machine.symbol_latched = false;
    machine.board_down(vec![ZXKey::Shift, ZXKey::SymShift]);
    machine.board_up();
}

/// The number row's Extended mode colours, drawn in the colour they name.
fn colour_of(column: usize) -> Color32 {
    [
        Color32::from_rgb(0x55, 0x77, 0xff),
        RED,
        Color32::from_rgb(0xff, 0x55, 0xff),
        GREEN,
        Color32::from_rgb(0x55, 0xff, 0xff),
        Color32::from_rgb(0xff, 0xff, 0x55),
        WHITE,
    ][column]
}

/// What a key gives, every way, for its tooltip.
fn describe(cap: &Cap, opticos: bool) -> String {
    if opticos {
        return describe_opticos(cap);
    }
    match cap.key {
        ZXKey::Shift => "Caps Shift: click, then a key, to press them together. With Symbol Shift, Extended mode.".into(),
        ZXKey::SymShift => "Symbol Shift: click, then a key, for the red symbol or word on it. With Caps Shift, Extended mode.".into(),
        ZXKey::Space => "Space; with Caps Shift, Break.".into(),
        ZXKey::Enter => "Enter".into(),
        _ => {
            let mut lines = vec![format!("{}: {}", cap.main, if cap.keyword.is_empty() { "-" } else { cap.keyword })];
            let caps_shift = matches!(cap.key, ZXKey::N0 | ZXKey::N1 | ZXKey::N2 | ZXKey::N3 | ZXKey::N4 | ZXKey::N5 | ZXKey::N6 | ZXKey::N7 | ZXKey::N8 | ZXKey::N9);
            if caps_shift {
                lines[0] = format!("{0}; with Caps Shift, {1}", cap.main, cap.keyword);
            } else {
                lines[0] = format!("{} (the keyword {} at the start of a line)", cap.main, cap.keyword);
            }
            lines.push(format!("Symbol Shift: {}", cap.symbol));
            if !cap.above.is_empty() {
                lines.push(format!("Extended mode: {}", cap.above));
            }
            if !cap.below.is_empty() {
                lines.push(format!("Extended mode, then Symbol Shift: {}", cap.below));
            }
            lines.join("\n")
        }
    }
}

/// What a key gives under OpticOS, for its tooltip.
fn describe_opticos(cap: &Cap) -> String {
    match cap.key {
        ZXKey::Shift => "Caps Shift: click, then a key, to press them together: a capital, or the word on a number key. With Symbol Shift, Extended mode.".into(),
        ZXKey::SymShift => "Symbol Shift: click, then a key, for the red symbol on it. With Caps Shift, Extended mode, for [ ] { } \\ | ~ on Y U F G D S A.".into(),
        ZXKey::Space => "Space; with Caps Shift, Break.".into(),
        ZXKey::Enter => "Enter".into(),
        _ => {
            let mut lines = vec![if cap.keyword.is_empty() {
                cap.main.to_string()
            } else {
                format!("{}; with Caps Shift, {}", cap.main, cap.keyword)
            }];
            if !cap.symbol.is_empty() {
                lines.push(format!("Symbol Shift: {}", cap.symbol));
            }
            if !cap.above.is_empty() {
                lines.push(format!("Extended mode: {}", cap.above));
            }
            lines.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// OpticOS's keys show no Sinclair keywords: only letters, digits, symbols and its own key functions.
    #[test]
    fn opticos_keys_have_no_sinclair_keywords() {
        let words = ["LOAD", "PRINT", "GO TO", "RUN", "SAVE", "BEEP", "TRUE VID", "GRAPHICS", "STOP", "AND"];
        for cap in OPTICOS_ROWS.iter().flatten().chain(OPTICOS_BOTTOM.iter()) {
            for text in [cap.keyword, cap.symbol, cap.above, cap.below] {
                assert!(!words.contains(&text), "{} shows {text}", cap.main);
            }
            assert!(cap.below.is_empty());
        }
        assert_eq!(describe(&OPTICOS_ROWS[2][6], true), "J\nSymbol Shift: -");
        assert_eq!(describe(&OPTICOS_ROWS[0][0], true), "1; with Caps Shift, EDIT\nSymbol Shift: !");
        // Every key's place matches between the two keyboards.
        for (a, b) in ROWS.iter().flatten().chain(BOTTOM.iter()).zip(OPTICOS_ROWS.iter().flatten().chain(OPTICOS_BOTTOM.iter())) {
            assert_eq!(a.main, b.main);
        }
        // The quick buttons type OpticOS's LOAD "" letter by letter.
        assert!(OPTICOS_COMBINATIONS.iter().any(|(_, _, action)| matches!(action, Action::Type("load \"\"\n"))));
    }
}
