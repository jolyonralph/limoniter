//! Every box and menu fits on a small screen: each is opened in a window far smaller than it was laid out for, and
//! it must lie wholly on the screen, with what doesn't fit scrolling inside it (see src/fit.rs).

use super::tests::app_with;
use super::*;

/// A screen smaller than any box was laid out for: most of them are taller than this, and some wider.
const SMALL: egui::Vec2 = egui::vec2(560.0, 340.0);

fn small_input() -> egui::RawInput {
    egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SMALL)), ..Default::default() }
}

fn frame(app: &mut Limoniter, ctx: &Context) {
    let mut output = ctx.run_ui(small_input(), |ui| app.draw_frame(ui));
    output.textures_delta.clear();
}

/// The rectangles of what's shown in `order`: windows (boxes) in the middle, menus in front.
fn shown(ctx: &Context, order: egui::Order) -> Vec<(egui::Id, egui::Rect)> {
    ctx.memory(|memory| memory.areas().visible_layer_ids().into_iter().filter(|layer| layer.order == order).filter_map(|layer| memory.area_rect(layer.id).map(|rect| (layer.id, rect))).collect())
}

fn on_screen(rect: egui::Rect) -> bool {
    egui::Rect::from_min_size(egui::Pos2::ZERO, SMALL).expand(0.5).contains_rect(rect)
}

/// Commands that open a box with nothing more needed (no file picker, no server), by their names for the message.
const BOXES: &[(&str, Command)] = &[
    ("Settings", Command::Settings),
    ("Devices", Command::ManageDevices),
    ("Compilers", Command::Compilers),
    ("Assemblers", Command::ChooseAssembler),
    ("Amiga crunchers and music", Command::AmigaTools),
    ("New Amiga C project", Command::AmigaNewProject),
    ("Amiga NDK", Command::AmigaNdk),
    ("ST music and packers", Command::StTools),
    ("C64 SID music", Command::C64Tools),
    ("Dragon DOS disk", Command::DragonDisk),
    ("About", Command::About),
    ("Themes", Command::Themes),
    ("VS Code extensions", Command::Extensions),
    ("Add-ons", Command::ManageAddOns),
    ("Kinds of new file", Command::EditNewKinds),
    ("Kind of computer for new files", Command::ChoosePreset),
    ("New disk or tape image", Command::NewImage),
    ("Open from the web", Command::OpenUrl),
    ("Disassemble", Command::Disassemble),
    ("Snippets", Command::Snippets),
    ("Run a command", Command::RunTool),
    ("Code as a picture", Command::CodePicture),
    ("Editor font", Command::ChooseFont),
    ("Window font", Command::WindowFont),
    ("Insert colour", Command::InsertColor),
    ("Insert byte", Command::InsertByte),
    ("Insert or fill columns", Command::ColumnFill),
    ("Insert numbers", Command::ColumnNumber),
    ("Paragraph setup", Command::ParagraphSetup),
    ("Wrap at a column", Command::WrapToLineEnds),
    ("Surround with a tag", Command::SurroundTag),
    ("Save this layout", Command::SaveLayout),
    ("Go to line", Command::GoTo),
    ("Go to file", Command::GoToFile),
    ("Find a command", Command::CommandPalette),
    ("Saved comparisons", Command::CompareSessions),
    ("Microcontroller flashing", Command::FlashSettings),
    ("Time spent", Command::TimeSpent),
];

#[test]
fn every_box_fits_on_a_small_screen() {
    let mut problems = Vec::new();
    for (name, command) in BOXES {
        let (mut app, ctx) = app_with("");
        frame(&mut app, &ctx);
        let mut output = ctx.run_ui(small_input(), |ui| app.run(*command, ui));
        output.textures_delta.clear();
        for _ in 0..6 {
            frame(&mut app, &ctx);
        }
        let windows = shown(&ctx, egui::Order::Middle);
        if windows.is_empty() {
            problems.push(format!("{name}: no box opened"));
        }
        for (_, rect) in windows {
            if !on_screen(rect) {
                problems.push(format!("{name}: {rect:?} isn't within {SMALL:?}"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_longest_menus_and_sub_menus_fit_on_a_small_screen() {
    for path in [&["File"][..], &["File", "New as"], &["File", "New as", "Amiga"], &["Tools"], &["Edit"], &["View"]] {
        let (mut app, ctx) = app_with("");
        ctx.data_mut(|data| data.insert_temp(crate::fit::opened_menus_key(), path.iter().map(|name| name.to_string()).collect::<Vec<_>>()));
        for _ in 0..10 {
            frame(&mut app, &ctx);
        }
        let menus = shown(&ctx, egui::Order::Foreground);
        assert!(menus.len() >= path.len(), "{path:?}: the menus didn't open ({} shown)", menus.len());
        for (_, rect) in menus {
            assert!(on_screen(rect), "{path:?}: a menu at {rect:?} isn't within {SMALL:?}");
        }
    }
}

#[test]
fn a_long_menu_drops_below_its_bar_rather_than_over_it() {
    let (mut app, ctx) = app_with("");
    ctx.data_mut(|data| data.insert_temp(crate::fit::opened_menus_key(), vec!["File".to_string()]));
    for _ in 0..10 {
        frame(&mut app, &ctx);
    }
    let menus = shown(&ctx, egui::Order::Foreground);
    let (_, file) = menus.iter().max_by(|a, b| a.1.height().total_cmp(&b.1.height())).expect("the File menu opened");
    assert!(file.top() >= 20.0, "it begins below the menu bar: {file:?}");
    assert!(on_screen(*file), "{file:?}");
}

fn frame_sized(app: &mut Limoniter, ctx: &Context, size: egui::Vec2) {
    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), ..Default::default() };
    let mut output = ctx.run_ui(input, |ui| app.draw_frame(ui));
    output.textures_delta.clear();
}

fn tallest_menu(ctx: &Context) -> egui::Rect {
    shown(ctx, egui::Order::Foreground).into_iter().map(|(_, rect)| rect).max_by(|a, b| a.height().total_cmp(&b.height())).unwrap_or(egui::Rect::NOTHING)
}

/// As the person found it: the window made small, Tools opened (it scrolls), the window made big again, and Tools
/// opened again uses the room there is, both when it was left open and when it was closed and opened again.
#[test]
fn the_tools_menu_uses_the_room_after_the_window_grows() {
    let big = egui::vec2(1400.0, 1000.0);
    for close_between in [false, true] {
        let (mut app, ctx) = app_with("");
        ctx.data_mut(|data| data.insert_temp(crate::fit::opened_menus_key(), vec!["Tools".to_string()]));
        for _ in 0..10 {
            frame_sized(&mut app, &ctx, SMALL);
        }
        let small = tallest_menu(&ctx);
        assert!(on_screen(small), "scrolls on the small window: {small:?}");
        if close_between {
            ctx.data_mut(|data| data.remove_temp::<Vec<String>>(crate::fit::opened_menus_key()));
            egui::Popup::close_all(&ctx);
            for _ in 0..5 {
                frame_sized(&mut app, &ctx, big);
            }
            ctx.data_mut(|data| data.insert_temp(crate::fit::opened_menus_key(), vec!["Tools".to_string()]));
        }
        for _ in 0..10 {
            frame_sized(&mut app, &ctx, big);
        }
        let grown = tallest_menu(&ctx);
        assert!(grown.height() > small.height() + 150.0, "closed between: {close_between}; Tools should use the bigger window: {small:?} then {grown:?}");
    }
}
