//! The ribbon's tests: that every button is the menus' own command, enabled and ticked exactly as its menu item is,
//! whatever's in front; and that Alt's letters, folding and the switch work.

use super::super::tests::{app_with, draw};
use super::*;
use crate::access::tests::{find, listing, nodes};
use eframe::egui::accesskit;

/// A file of its own for a test, in a folder of its own, so tests running at once never share one.
fn temp_file(name: &str, text: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("limoniter-ribbon-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn frame(app: &mut Limoniter, ctx: &Context, modifiers: egui::Modifiers, events: Vec<egui::Event>) -> egui::FullOutput {
    // The keys held are said first, as the window says them when they change.
    let events = std::iter::once(egui::Event::ModifiersChanged(modifiers)).chain(events).collect();
    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0))), events, ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
    output.textures_delta.clear();
    output
}

fn press(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
}

/// What was drawn in the frames since the last call.
fn take_drawn(ctx: &Context) -> Vec<Drawn> {
    ctx.data_mut(|d| d.remove_temp::<Vec<Drawn>>(drawn_key())).unwrap_or_default()
}

/// Every command a ribbon control gives, with its tab: the buttons and ticks, and those in its lists.
fn ribbon_commands() -> Vec<(Tab, Command)> {
    fn walk(entries: &[Entry], out: &mut Vec<Command>) {
        for entry in entries {
            match entry {
                Entry::Item(_, command) | Entry::Keyed(_, command, _) => out.push(*command),
                Entry::Check(_, flag) => out.push(Command::Toggle(*flag)),
                Entry::Sub(_, inner) => walk(inner, out),
                Entry::Live(..) | Entry::Line => {}
            }
        }
    }
    let tabs = Tab::FIXED.into_iter().chain(Kind::ALL.map(Tab::Kind));
    let mut out = Vec::new();
    for tab in tabs {
        for control in tab.groups().iter().flat_map(|g| g.controls.iter()) {
            let mut commands = Vec::new();
            match control.what {
                What::Run(command) => commands.push(command),
                What::Tick(flag) => commands.push(Command::Toggle(flag)),
                What::Drop(entries) => walk(entries, &mut commands),
            }
            out.extend(commands.into_iter().map(|command| (tab, command)));
        }
    }
    out
}

#[test]
fn every_ribbon_command_is_one_of_the_menus() {
    let menus: Vec<Command> = menus::every_command().into_iter().map(|found| found.command).collect();
    for (tab, command) in ribbon_commands() {
        assert!(menus.contains(&command), "a command on the {} tab isn't in the menus", tab.name());
    }
}

#[test]
fn every_ribbon_button_has_a_name_and_every_tab_some_groups() {
    for tab in Tab::FIXED.into_iter().chain(Kind::ALL.map(Tab::Kind)) {
        assert!(!tab.groups().is_empty(), "{tab:?}");
        for group in tab.groups() {
            assert!(!group.name.is_empty() && !group.controls.is_empty(), "{tab:?}");
            for control in group.controls {
                assert!(!control.label.trim().is_empty());
            }
        }
    }
}

/// The ribbon drawn on every tab there is now, and the menus laid open, after which each button's state is
/// compared with its menu item's.
fn check_ribbon_matches_menus(app: &mut Limoniter, ctx: &Context, what: &str) -> usize {
    app.view.ribbon = true;
    app.view.ribbon_folded = false;
    draw(app, ctx, Vec::new());
    take_drawn(ctx);
    for menu in 0..menus::MENUS.len() {
        app.shown_menu = Some((menu, true));
        draw(app, ctx, Vec::new());
    }
    app.shown_menu = None;
    let in_menus: Vec<Drawn> = take_drawn(ctx).into_iter().filter(|d| !d.ribbon).collect();
    let mut checked = 0;
    for tab in app.ribbon_tabs() {
        app.ribbon.tab = tab;
        draw(app, ctx, Vec::new());
        let on_ribbon: Vec<Drawn> = take_drawn(ctx).into_iter().filter(|d| d.ribbon).collect();
        assert!(!on_ribbon.is_empty(), "{what}: the {} tab drew nothing", tab.name());
        for button in on_ribbon {
            let same: Vec<&Drawn> = in_menus.iter().filter(|m| m.command == button.command).collect();
            assert!(!same.is_empty(), "{what}: a button on the {} tab has no menu item", tab.name());
            for item in same {
                assert_eq!((button.enabled, button.ticked), (item.enabled, item.ticked), "{what}: a button on the {} tab differs from its menu item", tab.name());
            }
            checked += 1;
        }
    }
    checked
}

#[test]
fn every_ribbon_button_is_enabled_and_ticked_as_its_menu_item_is() {
    let (mut app, ctx) = app_with("");
    let mut checked = 0;
    // A file with text, then a selection, then read-only.
    app.doc().text.set_content(vec!["hello world".into(), "second line".into()]);
    checked += check_ribbon_matches_menus(&mut app, &ctx, "plain text");
    app.doc().text.move_caret(0, 0, false);
    app.doc().text.move_caret(0, 5, true);
    checked += check_ribbon_matches_menus(&mut app, &ctx, "a selection");
    app.view.line_numbers = !app.view.line_numbers;
    app.view.word_wrap = !app.view.word_wrap;
    checked += check_ribbon_matches_menus(&mut app, &ctx, "ticks changed");
    app.doc().set_read_only(true);
    checked += check_ribbon_matches_menus(&mut app, &ctx, "read-only");
    // Assembler source, BASIC and Markdown, made as File, New as makes them, each with a tab of its own.
    highlight::load_now();
    for (name, kind) in [("Z80 assembler", Kind::Assembly), ("Spectrum BASIC", Kind::Basic), ("Markdown", Kind::Markdown)] {
        let index = app.new_kinds.iter().position(|k| k.name == name).unwrap();
        app.new_of_kind(index);
        app.doc().update_syntax();
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.ribbon_kind(), Some(kind), "{name}");
        assert!(app.ribbon_tabs().contains(&Tab::Kind(kind)));
        checked += check_ribbon_matches_menus(&mut app, &ctx, name);
    }
    // A table, opened from a file.
    let path = temp_file("list.csv", "a,b\n1,2\n");
    assert!(app.open_path(&path));
    for _ in 0..50 {
        draw(&mut app, &ctx, Vec::new());
        if !app.docs[app.active].is_busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if app.docs[app.active].table.is_none() {
        app.run_command_for_test(Command::ToggleTable, &ctx);
    }
    for _ in 0..50 {
        draw(&mut app, &ctx, Vec::new());
        if app.ribbon_kind() == Some(Kind::Table) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(app.ribbon_kind(), Some(Kind::Table));
    checked += check_ribbon_matches_menus(&mut app, &ctx, "a table");
    app.run_command_for_test(Command::ToggleTable, &ctx);
    // Hex.
    app.run_command_for_test(Command::ViewHex, &ctx);
    draw(&mut app, &ctx, Vec::new());
    assert_eq!(app.ribbon_kind(), Some(Kind::Hex));
    checked += check_ribbon_matches_menus(&mut app, &ctx, "hex");
    // And with nothing open at all.
    while !app.no_file() {
        app.remove_tab(app.active);
    }
    checked += check_ribbon_matches_menus(&mut app, &ctx, "no file");
    assert!(checked > 500, "only {checked} buttons were compared");
}

#[test]
fn a_files_own_tab_goes_when_the_file_does() {
    let (mut app, ctx) = app_with("");
    app.view.ribbon = true;
    app.run_command_for_test(Command::ViewHex, &ctx);
    app.ribbon.tab = Tab::Kind(Kind::Hex);
    assert_eq!(app.ribbon_tab(), Tab::Kind(Kind::Hex));
    app.run_command_for_test(Command::ViewRegular, &ctx);
    assert_eq!(app.ribbon_kind(), None);
    assert_eq!(app.ribbon_tab(), Tab::Home, "back to Home once the hex is gone");
    assert!(!app.ribbon_tabs().iter().any(|tab| matches!(tab, Tab::Kind(_))));
}

#[test]
fn alt_letters_are_different_on_every_tab() {
    let tabs: Vec<char> = Tab::FIXED.iter().map(|tab| tab.key()).chain([FILE_KEY, 'X']).collect();
    let mut unique = tabs.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), tabs.len(), "the tabs' letters are all different");
    for tab in Tab::FIXED.into_iter().chain(Kind::ALL.map(Tab::Kind)) {
        let labels: Vec<String> = tab.groups().iter().flat_map(|g| g.controls.iter()).map(|c| crate::i18n::tr(c.label).to_string()).collect();
        let letters: Vec<char> = key_letters(&labels).into_iter().map(|l| l.expect("every button has a letter")).collect();
        let mut seen = letters.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), letters.len(), "{tab:?}: no two buttons share a letter");
        assert!(!letters.contains(&FILE_KEY), "{tab:?}: F is the File button's");
    }
    assert_eq!(key_letters(&["Paste".into(), "Print".into(), "Cut".into()]), [Some('P'), Some('R'), Some('C')], "the initial if it's free, then another letter");
}

#[test]
fn alt_shows_the_letters_and_letters_choose_a_tab_then_a_button() {
    let (mut app, ctx) = app_with("ribbon=true\n");
    app.doc().text.set_content(vec!["pear".into(), "apple".into()]);
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    assert_eq!(app.ribbon.tips, Tips::Off);
    // Alt pressed and let go alone.
    frame(&mut app, &ctx, egui::Modifiers::ALT, Vec::new());
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    assert_eq!(app.ribbon.tips, Tips::Tabs);
    // E for Edit.
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(egui::Key::E)]);
    assert_eq!(app.ribbon.tips, Tips::Buttons);
    assert_eq!(app.ribbon.tab, Tab::Edit);
    // Then Select all's letter: the text's all selected, and the letter typed nothing.
    let labels: Vec<String> = Tab::Edit.groups().iter().flat_map(|g| g.controls.iter()).map(|c| crate::i18n::tr(c.label).to_string()).collect();
    let at = labels.iter().position(|l| l == "Select all").unwrap();
    let letter = key_letters(&labels)[at].unwrap();
    let key = egui::Key::from_name(&letter.to_string()).unwrap();
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(key), egui::Event::Text(letter.to_string())]);
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    assert_eq!(app.ribbon.tips, Tips::Off);
    assert_eq!(app.doc().text.selection_text().replace("\r\n", "\n"), "pear\napple");
    assert_eq!(app.doc().text.line(0).as_ref(), "pear");
    // Alt with another key is a shortcut, not the letters; and Escape goes back a step at a time.
    frame(&mut app, &ctx, egui::Modifiers::ALT, vec![press(egui::Key::U)]);
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    assert_eq!(app.ribbon.tips, Tips::Off);
    frame(&mut app, &ctx, egui::Modifiers::ALT, Vec::new());
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![]);
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(egui::Key::S)]);
    assert_eq!((app.ribbon.tips, app.ribbon.tab), (Tips::Buttons, Tab::Search));
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(egui::Key::Escape)]);
    assert_eq!(app.ribbon.tips, Tips::Tabs);
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![press(egui::Key::Escape)]);
    assert_eq!(app.ribbon.tips, Tips::Off);
}

#[test]
fn with_the_ribbon_off_alt_does_nothing_of_the_ribbons() {
    let (mut app, ctx) = app_with("");
    frame(&mut app, &ctx, egui::Modifiers::ALT, Vec::new());
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    assert_eq!(app.ribbon.tips, Tips::Off);
}

#[test]
fn ctrl_f1_and_a_double_click_fold_the_ribbon_and_a_click_shows_a_tab_over_the_text() {
    let (mut app, ctx) = app_with("ribbon=true\n");
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    let ctrl_f1 = egui::Event::Key { key: egui::Key::F1, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::CTRL | egui::Modifiers::COMMAND };
    frame(&mut app, &ctx, egui::Modifiers::CTRL | egui::Modifiers::COMMAND, vec![ctrl_f1.clone()]);
    assert!(app.view.ribbon_folded);
    assert!(matches!(app.dialog, Dialog::None), "Ctrl+F1 isn't F1's command box");
    frame(&mut app, &ctx, egui::Modifiers::CTRL | egui::Modifiers::COMMAND, vec![ctrl_f1]);
    assert!(!app.view.ribbon_folded);
    // Folded, the menu's Fold command can do it too, and kept in the settings.
    app.run_command_for_test(Command::FoldRibbon, &ctx);
    assert!(app.view.ribbon_folded);
    assert!(app.view.to_settings().contains("ribbon_folded=true"));
    // With the ribbon off, Ctrl+F1 leaves it alone.
    app.view.ribbon = false;
    assert!(!app.can(Command::FoldRibbon));
    app.run_command_for_test(Command::FoldRibbon, &ctx);
    assert!(app.view.ribbon_folded);
}

#[test]
fn the_switch_beside_find_a_command_turns_the_ribbon_on_and_off() {
    let (mut app, ctx) = app_with("");
    ctx.enable_accesskit();
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    let output = nodes(&frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new()));
    let switch = find(&output, accesskit::Role::Switch, "Ribbon").unwrap_or_else(|| panic!("{}", listing(&output)));
    assert_eq!(switch.toggled(), Some(accesskit::Toggled::False));
    assert!(find(&output, accesskit::Role::Tab, "Home").is_none(), "with it off, the menus are as they were");
    app.run_command_for_test(Command::Toggle(Flag::Ribbon), &ctx);
    assert!(app.view.ribbon);
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    let output = nodes(&frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new()));
    assert_eq!(find(&output, accesskit::Role::Switch, "Ribbon").and_then(|n| n.toggled()), Some(accesskit::Toggled::True));
    for tab in Tab::FIXED {
        assert!(find(&output, accesskit::Role::Tab, tab.name()).is_some(), "{tab:?}: {}", listing(&output));
    }
    // Buttons are read out by name, greyed when they can't act, and ticks with their tick.
    let undo = find(&output, accesskit::Role::Button, "Undo").expect("Undo on Home");
    assert!(undo.is_disabled(), "nothing to undo yet");
    let wrap = find(&output, accesskit::Role::CheckBox, "Word wrap").expect("Word wrap on Home");
    assert_eq!(wrap.toggled(), Some(if app.view.word_wrap { accesskit::Toggled::True } else { accesskit::Toggled::False }));
    assert!(ViewOptions::from_settings(&app.view.to_settings()).ribbon, "kept in the settings");
}

#[test]
fn the_ribbon_draws_on_every_tab_in_both_themes_at_any_width_folded_or_not() {
    for light in [true, false] {
        theme::set_light(light);
        let (mut app, ctx) = app_with("ribbon=true\n");
        app.doc().text.set_content(vec!["text".into()]);
        for folded in [false, true] {
            app.view.ribbon_folded = folded;
            app.ribbon.popped = folded;
            for tab in Tab::FIXED {
                app.ribbon.tab = tab;
                for width in [1600.0, 1024.0, 700.0, 480.0] {
                    let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 480.0))), ..Default::default() };
                    let mut output = ctx.run_ui(raw, |ui| app.draw_frame(ui));
                    output.textures_delta.clear();
                }
            }
        }
    }
    theme::set_light(false);
}

#[test]
fn large_names_break_into_two_lines_near_their_middle() {
    assert_eq!(two_lines("Paste"), ("Paste", ""));
    assert_eq!(two_lines("Character and sprite editor"), ("Character and", "sprite editor"));
    assert_eq!(two_lines("Unbreakablewordhere"), ("Unbreakablewordhere", ""));
}

#[test]
fn alt_tab_to_another_window_and_back_shows_no_letters() {
    let (mut app, ctx) = app_with("ribbon=true\n");
    frame(&mut app, &ctx, egui::Modifiers::NONE, Vec::new());
    frame(&mut app, &ctx, egui::Modifiers::ALT, Vec::new());
    frame(&mut app, &ctx, egui::Modifiers::ALT, vec![egui::Event::WindowFocused(false)]);
    frame(&mut app, &ctx, egui::Modifiers::NONE, vec![egui::Event::WindowFocused(true)]);
    assert_eq!(app.ribbon.tips, Tips::Off);
}
