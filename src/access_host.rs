//! Getting about the window with the keyboard alone: F10 goes to the menus, Ctrl+F6 leaves the editor (which keeps
//! Tab and the arrows for itself) for the rest of the window, and a box that opens takes the keyboard at once.

use super::*;
use eframe::egui::{FocusDirection, Id, Order};

/// Where the menu bar notes its first menu, for F10 to go to.
pub(super) fn first_menu_key() -> Id {
    Id::unique("limoniter-first-menu")
}

/// Where the last box open is noted, with how many more frames to try putting the keyboard in it.
fn dialog_key() -> Id {
    Id::unique("limoniter-dialog-focus")
}

impl Limoniter {
    /// F10 puts the keyboard on the File menu, where the arrows and Enter open the menus. Ctrl+F6 moves it from
    /// wherever it is to the next thing that takes it, Ctrl+Shift+F6 to the one before: out of the editor, to the
    /// menus, tabs, panels and status bar, and Tab and Shift+Tab go on from there.
    pub(super) fn keyboard_routes(&mut self, ctx: &Context) {
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F10))
            && let Some(menu) = ctx.data(|d| d.get_temp::<Id>(first_menu_key()))
        {
            ctx.memory_mut(|m| m.request_focus(menu));
        }
        let back = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::F6));
        let next = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F6));
        if back || next {
            ctx.memory_mut(|m| m.move_focus(if back { FocusDirection::Previous } else { FocusDirection::Next }));
        }
    }

    /// A box that's just opened takes the keyboard: unless something in it already has, its first field or button
    /// does, so Tab goes on through it and Escape and Enter reach it. Tried for a few frames, as a box may take a
    /// frame to be laid out.
    pub(super) fn focus_into_dialog(&mut self, ctx: &Context) {
        use std::hash::{Hash, Hasher};
        let now = (!matches!(self.dialog, Dialog::None)).then(|| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::mem::discriminant(&self.dialog).hash(&mut hasher);
            hasher.finish()
        });
        let (last, tries) = ctx.data(|d| d.get_temp::<(Option<u64>, u8)>(dialog_key())).unwrap_or_default();
        let mut tries = match now {
            Some(_) if now != last => 3,
            Some(_) => tries,
            None => 0,
        };
        if tries > 0 {
            tries -= 1;
            let in_window = ctx.memory(|m| m.focused()).and_then(|id| ctx.viewport(|v| v.this_pass.widgets.get(id).map(|w| w.layer_id.order))).is_some_and(|order| order != Order::Background);
            if in_window {
                tries = 0;
            } else if let Some(layer) = ctx.memory(|m| m.areas().top_layer_id(Order::Middle))
                && let Some(first) = ctx.viewport(|v| v.this_pass.widgets.get_layer(layer).find(|w| w.enabled && w.sense.is_focusable()).map(|w| w.id))
            {
                ctx.memory_mut(|m| m.request_focus(first));
                tries = 0;
            }
        }
        ctx.data_mut(|d| d.insert_temp(dialog_key(), (now, tries)));
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;
    use crate::access::tests::{find, listing};
    use eframe::egui::accesskit::Role;

    fn press(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn focused_role(output: &egui::FullOutput) -> Option<(Role, String)> {
        let update = output.platform_output.accesskit_update.as_ref()?;
        let nodes = &update.nodes;
        nodes.iter().find(|(id, _)| *id == update.focus).map(|(_, node)| (node.role(), node.label().unwrap_or_default().to_string()))
    }

    /// The editor is a text field to a screen reader, with the line the cursor is on and where it is.
    #[test]
    fn the_editor_tells_a_screen_reader_its_line_and_place() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        app.docs[0].text.type_text("first line\nsecond line");
        draw(&mut app, &ctx, Vec::new());
        let output = draw(&mut app, &ctx, Vec::new());
        let nodes = crate::access::tests::nodes(&output);
        let editor = find(&nodes, Role::MultilineTextInput, "Text").unwrap_or_else(|| panic!("{}", listing(&nodes)));
        assert_eq!(editor.value(), Some("second line"));
        assert!(editor.description().is_some_and(|d| d.contains("Ln 2") || d.contains('2')), "{:?}", editor.description());
        assert_eq!(focused_role(&output).map(|f| f.0), Some(Role::MultilineTextInput), "the editor has the keyboard");
        // In hex, the byte at the cursor.
        app.docs[0].hex_mode = true;
        app.docs[0].hex.load(b"AB".to_vec());
        draw(&mut app, &ctx, Vec::new());
        let output = draw(&mut app, &ctx, Vec::new());
        let nodes = crate::access::tests::nodes(&output);
        let hex = find(&nodes, Role::MultilineTextInput, "Hex").unwrap_or_else(|| panic!("{}", listing(&nodes)));
        assert!(hex.value().is_some_and(|v| v.contains("0x41")), "{:?}", hex.value());
    }

    /// The status bar's clickable words are buttons and a check box with names, and the command box is a button.
    #[test]
    fn the_status_bar_and_command_box_have_names_and_roles() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        draw(&mut app, &ctx, Vec::new());
        let output = draw(&mut app, &ctx, Vec::new());
        let nodes = crate::access::tests::nodes(&output);
        let lock = find(&nodes, Role::CheckBox, "Read-only").unwrap_or_else(|| panic!("{}", listing(&nodes)));
        assert_eq!(lock.toggled(), Some(egui::accesskit::Toggled::False));
        assert!(find(&nodes, Role::Button, "UTF-8").is_some(), "the encoding: {}", listing(&nodes));
        assert!(find(&nodes, Role::Button, "Find a command").is_some());
        assert!(find(&nodes, Role::Tab, "Edit").is_some(), "the file's tab: {}", listing(&nodes));
        assert!(find(&nodes, Role::Button, "Close Edit").is_some(), "its close button");
    }

    /// F10 reaches the menus; Ctrl+F6 leaves the editor, and Tab then moves on.
    #[test]
    fn the_keyboard_gets_out_of_the_editor_and_to_the_menus() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        assert!(ctx.memory(|m| m.focused()).is_some_and(views::is_editor_id));
        draw(&mut app, &ctx, vec![press(Key::F10, Modifiers::NONE)]);
        let output = draw(&mut app, &ctx, Vec::new());
        assert_eq!(focused_role(&output), Some((Role::Button, "File".into())), "F10 is on the File menu");
        // Enter opens it, and Down goes into it.
        draw(&mut app, &ctx, vec![press(Key::Enter, Modifiers::NONE)]);
        draw(&mut app, &ctx, Vec::new());
        assert!(egui::Popup::is_any_open(&ctx), "Enter opens the menu");
        draw(&mut app, &ctx, vec![press(Key::ArrowDown, Modifiers::NONE)]);
        draw(&mut app, &ctx, Vec::new());
        let output = draw(&mut app, &ctx, Vec::new());
        let focused = focused_role(&output);
        assert!(focused.as_ref().is_some_and(|(_, name)| name != "File"), "the keyboard is in the menu: {focused:?}");
        draw(&mut app, &ctx, vec![press(Key::Escape, Modifiers::NONE)]);
        draw(&mut app, &ctx, vec![press(Key::Escape, Modifiers::NONE)]);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        assert!(ctx.memory(|m| m.focused()).is_some_and(views::is_editor_id), "Escape goes back to the text");
        // Ctrl+F6 leaves the editor, which keeps Tab for itself.
        draw(&mut app, &ctx, vec![press(Key::F6, Modifiers::CTRL | Modifiers::COMMAND)]);
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, Vec::new());
        let left = ctx.memory(|m| m.focused());
        assert!(left.is_some_and(|id| !views::is_editor_id(id)), "out of the editor");
        draw(&mut app, &ctx, vec![press(Key::Tab, Modifiers::NONE)]);
        draw(&mut app, &ctx, Vec::new());
        assert_ne!(ctx.memory(|m| m.focused()), left, "Tab moves on");
    }

    /// A box opened from the keyboard has the keyboard: Go to line's field has it, and Escape closes it.
    #[test]
    fn a_box_takes_the_keyboard_and_escape_closes_it() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        draw(&mut app, &ctx, Vec::new());
        draw(&mut app, &ctx, vec![press(Key::F1, Modifiers::NONE)]);
        let output = draw(&mut app, &ctx, Vec::new());
        assert!(matches!(app.dialog, Dialog::Palette(_)));
        assert_eq!(focused_role(&output).map(|f| f.0), Some(Role::TextInput), "the command box's field has the keyboard");
        // The options are read out, the chosen one as the field's active one.
        let nodes = crate::access::tests::nodes(&output);
        let update = output.platform_output.accesskit_update.as_ref().unwrap();
        let field = nodes.iter().find(|(id, _)| *id == update.focus).map(|(_, n)| n).unwrap();
        let active = field.active_descendant().expect("an option is active");
        let option = nodes.iter().find(|(id, _)| *id == active).map(|(_, n)| n).unwrap();
        assert_eq!(option.role(), Role::ListBoxOption);
        assert!(option.is_selected() == Some(true) && option.label().is_some_and(|l| !l.is_empty()), "{}", listing(&nodes));
        draw(&mut app, &ctx, vec![press(Key::Escape, Modifiers::NONE)]);
        draw(&mut app, &ctx, Vec::new());
        assert!(matches!(app.dialog, Dialog::None));
        // Every box opened by a command: something in it takes the keyboard, and Escape closes it.
        for (name, command) in [("Go to line", Command::GoTo), ("About", Command::About), ("Devices", Command::ManageDevices), ("Themes", Command::Themes), ("Settings", Command::Settings)] {
            let _ = ctx.run_ui(Default::default(), |ui| app.run(command, ui));
            for _ in 0..3 {
                draw(&mut app, &ctx, Vec::new());
            }
            if matches!(app.dialog, Dialog::None) {
                continue;
            }
            let focused = ctx.memory(|m| m.focused());
            assert!(focused.is_some_and(|id| !views::is_editor_id(id)), "{name}: the box has the keyboard");
            draw(&mut app, &ctx, vec![press(Key::Escape, Modifiers::NONE)]);
            draw(&mut app, &ctx, Vec::new());
            assert!(matches!(app.dialog, Dialog::None), "{name}: Escape closes it");
        }
    }
}
