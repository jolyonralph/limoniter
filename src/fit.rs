//! Keeping boxes and menus on the screen. A box taller or wider than the window it's shown in, or a menu longer
//! than the window, used to run off the edge with no way to reach the rest of it. Everything here keeps them on the
//! screen, and when what's in one doesn't fit there, it scrolls.
//!
//! Every box is made with [`Dialog::new`] in place of `egui::Window::new`, and every menu with [`menu_button`],
//! [`context_menu`] or [`popup_menu`] in place of egui's own. `clippy.toml` refuses egui's plain ones, so a new box or
//! menu can't miss out.
//!
//! How it works: egui already keeps a window's size within the screen, but what's in it isn't kept within the
//! window, so it spills past the bottom. Here, what's in a box or menu is drawn as it always was while it fits. When
//! it's measured as taller (or wider) than the room the screen has for it, the next frame draws it in a scroll area
//! of at most that size, which egui then fits on the screen. It goes back to being drawn plainly once it fits again.
//! Drawing it the plain way while it fits means a box sized to what's in it, or one whose list fills the space it's
//! given, looks just as it did.

use eframe::egui;
use egui::containers::menu::{self, MenuState, SubMenu};
use egui::{Align2, Context, Id, InnerResponse, IntoAtoms, Pos2, Rect, Response, Ui, UiBuilder, Vec2, Vec2b, pos2, vec2};

/// The least room anything is squeezed to, however small the window, so there's always something to scroll.
const LEAST_ROOM: f32 = 80.0;

/// Which ways something didn't fit when it was last drawn, and so scrolls this time.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct Overflow {
    wide: bool,
    tall: bool,
}

/// Whether something `needed` long scrolls in `room`. Once scrolling, it stays so until it's clearly back within the
/// room, so something exactly as big as the room doesn't flicker between the two.
fn scrolls(needed: f32, room: f32, was: bool) -> bool {
    if was { needed > room - 2.0 } else { needed > room + 0.5 }
}

/// The room on the screen for what's drawn from `ui`'s next place onwards, in the box or menu it's in: `space`
/// (the screen, or the part of it a menu drops into), less what's around it (the title bar and edges, measured from
/// where the box or menu begins), less `below`.
fn room_in(ui: &Ui, space: Rect, below: f32) -> Vec2 {
    // The outermost part of the stack is the box's or menu's own area, at where it is this frame.
    let outer = ui.stack().iter().last().map_or(space, |stack| stack.max_rect);
    let start = pos2(ui.max_rect().left(), ui.cursor().top());
    let above = (start.y - outer.top()).max(0.0);
    let side = (start.x - outer.left()).max(0.0);
    // The bottom and right edges are taken to be as wide as the left one.
    vec2(space.width() - 2.0 * side, space.height() - above - side - below).max(Vec2::splat(LEAST_ROOM))
}

fn room(ui: &Ui, below: f32) -> Vec2 {
    room_in(ui, ui.ctx().content_rect(), below)
}

/// Draws `add` as it is while it fits in `room`, or else in a scroll area no bigger than `room`.
fn fitted<R>(ui: &mut Ui, room: Vec2, add: impl FnOnce(&mut Ui) -> R) -> R {
    let body = ui.scope_id().with("fit-body");
    let key = body.with("overflow");
    let was: Overflow = ui.data(|data| data.get_temp(key)).unwrap_or_default();
    // The same id either way, so what's typed or chosen inside isn't lost when it starts or stops scrolling. The room
    // goes with it, for [`height_left`].
    let inside = UiBuilder::new().scope_id(body).ui_stack_info(egui::UiStackInfo::default().with_tag_value(ROOM_TAG, room));
    let (inner, needed) = if was.wide || was.tall {
        let shown = egui::ScrollArea::new([was.wide, was.tall]).id_salt("fit-scroll").auto_shrink([true, true]).max_width(room.x).max_height(room.y).show(ui, |ui| ui.scope_builder(inside, add).inner);
        (shown.inner, shown.content_size)
    } else {
        let shown = ui.scope_builder(inside, add);
        (shown.inner, shown.response.rect.size())
    };
    let now = Overflow { wide: scrolls(needed.x, room.x, was.wide), tall: scrolls(needed.y, room.y, was.tall) };
    if now != was {
        ui.data_mut(|data| data.insert_temp(key, now));
        // Drawn again straight away the new way, so it never shows cut off for a frame.
        ui.ctx().request_discard("a box or menu stopped or started fitting on the screen");
    }
    inner
}

/// The tag on what [`fitted`] draws in, holding the room it has.
const ROOM_TAG: &str = "fit-room";

/// How much of the screen's room is left below where `ui` has got to, in the box or menu it's in (infinite outside
/// one). Something that would otherwise take a fixed height, such as a list, can take no more than this, so the box
/// fits on a small screen without having to scroll as a whole.
pub fn height_left(ui: &Ui) -> f32 {
    ui.stack().iter().find_map(|stack| stack.info.tags.get_downcast::<Vec2>(ROOM_TAG).map(|room| room.y - (ui.cursor().top() - stack.max_rect.top()))).map_or(f32::INFINITY, |left| left.max(LEAST_ROOM))
}

// ---------- Boxes ----------

/// Which part of a box [`Dialog::show_with_buttons`] wants drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    /// What the box is for, which scrolls when it doesn't fit.
    Body,
    /// The row of buttons along its foot (OK, Cancel and such), which stays in sight below.
    Buttons,
}

/// A box (an egui window) that's kept on the screen, with what's in it scrolling when it doesn't fit there. It's
/// made as `egui::Window` is, and has the same settings.
#[must_use = "a box is only seen when it's shown"]
pub struct Dialog<'a> {
    window: egui::Window<'a>,
    fixed_size: Option<Vec2>,
}

impl<'a> Dialog<'a> {
    #[allow(clippy::disallowed_methods, reason = "this is the one place a box is made")]
    pub fn new(title: impl IntoAtoms<'a>) -> Self {
        Self { window: egui::Window::new(title), fixed_size: None }
    }

    pub fn id(self, id: Id) -> Self {
        Self { window: self.window.id(id), ..self }
    }

    pub fn open(self, open: &'a mut bool) -> Self {
        Self { window: self.window.open(open), ..self }
    }

    pub fn collapsible(self, collapsible: bool) -> Self {
        Self { window: self.window.collapsible(collapsible), ..self }
    }

    pub fn resizable(self, resizable: impl Into<Vec2b>) -> Self {
        Self { window: self.window.resizable(resizable), ..self }
    }

    pub fn title_bar(self, title_bar: bool) -> Self {
        Self { window: self.window.title_bar(title_bar), ..self }
    }

    pub fn anchor(self, align: Align2, offset: impl Into<Vec2>) -> Self {
        Self { window: self.window.anchor(align, offset), ..self }
    }

    pub fn pivot(self, pivot: Align2) -> Self {
        Self { window: self.window.pivot(pivot), ..self }
    }

    pub fn default_pos(self, pos: impl Into<Pos2>) -> Self {
        Self { window: self.window.default_pos(pos), ..self }
    }

    pub fn default_size(self, size: impl Into<Vec2>) -> Self {
        Self { window: self.window.default_size(size), ..self }
    }

    pub fn default_width(self, width: f32) -> Self {
        Self { window: self.window.default_width(width), ..self }
    }

    pub fn default_height(self, height: f32) -> Self {
        Self { window: self.window.default_height(height), ..self }
    }

    pub fn min_width(self, width: f32) -> Self {
        Self { window: self.window.min_width(width), ..self }
    }

    pub fn max_width(self, width: f32) -> Self {
        Self { window: self.window.max_width(width), ..self }
    }

    pub fn max_height(self, height: f32) -> Self {
        Self { window: self.window.max_height(height), ..self }
    }

    /// A size the box keeps, made smaller on a screen it wouldn't fit.
    pub fn fixed_size(self, size: impl Into<Vec2>) -> Self {
        Self { fixed_size: Some(size.into()), ..self }
    }

    /// The window as egui shows it, with a fixed size brought within the screen.
    fn window(self, ctx: &Context) -> egui::Window<'a> {
        match self.fixed_size {
            Some(size) => self.window.fixed_size(size.min(ctx.content_rect().size())),
            None => self.window,
        }
    }

    /// Shows the box, with everything in it scrolling when it doesn't fit on the screen.
    pub fn show<R>(self, ctx: &Context, add: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<Option<R>>> {
        self.window(ctx).show(ctx, |ui| {
            let room = room(ui, 0.0);
            fitted(ui, room, add)
        })
    }

    /// Shows the box in two parts: `add` is called for its [`Part::Body`], which scrolls when it doesn't fit on the
    /// screen, and then for its [`Part::Buttons`], which stay in sight below it.
    pub fn show_with_buttons(self, ctx: &Context, mut add: impl FnMut(&mut Ui, Part)) -> Option<InnerResponse<Option<()>>> {
        self.window(ctx).show(ctx, |ui| {
            // How tall the buttons were last time; a row of them until they've been measured.
            let key = ui.scope_id().with("fit-buttons");
            let guess = ui.spacing().interact_size.y + 2.0 * ui.spacing().item_spacing.y;
            let below = ui.data(|data| data.get_temp::<f32>(key)).unwrap_or(guess);
            let room = room(ui, below);
            fitted(ui, room, |ui| add(ui, Part::Body));
            let top = ui.min_rect().bottom();
            add(ui, Part::Buttons);
            let height = ui.min_rect().bottom() - top;
            if (height - below).abs() > 0.5 {
                ui.data_mut(|data| data.insert_temp(key, height));
                ui.ctx().request_repaint();
            }
        })
    }
}

// ---------- Menus ----------

/// The key under which a picture or a test asks for menus to be opened, by the names shown on them (as
/// `["File", "New as", "Amiga"]`), so a menu can be seen opened as a click opens it.
pub fn opened_menus_key() -> Id {
    Id::unique("fit-opened-menus")
}

/// Where the button a menu dropped from was, kept under the menu's id, so the menu can tell how much room there is
/// below the button (or above it, if that's where it opened).
fn anchor_key(menu: Id) -> Id {
    menu.with("fit-anchor")
}

/// Draws what's in a menu, scrolling when the menu would be longer (or wider) than the screen. A menu dropping from a
/// button has the room on the side of the button it opened on; any other (a sub-menu, or one opened with a right
/// click) has the screen's whole height, as it's moved up to fit.
pub fn menu_body<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let screen = ui.ctx().content_rect();
    let space = match ui.data(|data| data.get_temp::<Rect>(anchor_key(ui.layer_id().id))) {
        Some(button) if screen.bottom() - button.bottom() >= button.top() - screen.top() => Rect::from_min_max(pos2(screen.left(), button.bottom()), screen.max),
        Some(button) => Rect::from_min_max(screen.min, pos2(screen.right(), button.top())),
        None => screen,
    };
    // A little is left for the gap between the button and the menu.
    let room = room_in(ui, space, ui.spacing().item_spacing.y);
    fitted(ui, room, add)
}

/// Notes where the button a menu drops from is, for [`menu_body`].
fn note_anchor(button: &Response) {
    button.ctx.data_mut(|data| data.insert_temp(anchor_key(egui::Popup::default_response_id(button)), button.rect));
}

/// A menu's button, as `ui.menu_button` (a sub-menu's when it's in a menu), whose menu scrolls when it's too long.
pub fn menu_button<'a, R>(ui: &mut Ui, title: impl IntoAtoms<'a>, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
    let title = title.into_atoms();
    let name = title.text().map(|text| text.into_owned());
    let in_menu = menu::is_in_menu(ui);
    #[allow(clippy::disallowed_methods, reason = "this is the one place a menu button is made")]
    let shown = ui.menu_button(title, |ui| menu_body(ui, add));
    if !in_menu {
        note_anchor(&shown.response);
    }
    // Opened by name for a picture or a test, as though it had been clicked.
    let asked = ui.data(|data| data.get_temp::<Vec<String>>(opened_menus_key())).unwrap_or_default();
    if let Some(name) = name
        && asked.contains(&name)
        && shown.inner.is_none()
    {
        if in_menu {
            let sub = SubMenu::id_from_widget_id(shown.response.id);
            MenuState::from_ui(ui, |state, _| state.open_item = Some(sub));
        } else {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&shown.response));
        }
        ui.ctx().request_repaint();
    }
    shown
}

/// A menu that opens on a right click on `response`, as `response.context_menu`, which scrolls when it's too long.
pub fn context_menu(response: &Response, add: impl FnOnce(&mut Ui)) -> Option<InnerResponse<()>> {
    #[allow(clippy::disallowed_methods, reason = "this is the one place a context menu is made")]
    response.context_menu(|ui| menu_body(ui, add))
}

/// A menu dropping from `button` (such as a drop-down list), as `egui::Popup::menu(button).show`, which scrolls when
/// it's too long for the screen.
pub fn popup_menu<R>(button: &Response, add: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
    note_anchor(button);
    #[allow(clippy::disallowed_methods, reason = "this is the one place a drop-down menu is shown")]
    egui::Popup::menu(button).show(|ui| menu_body(ui, add))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(width: f32, height: f32) -> egui::RawInput {
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(width, height))), ..Default::default() }
    }

    /// The rectangle of the box called `title` after a few frames of it drawn with `rows` rows of `row_text`.
    fn window_after(ctx: &Context, size: Vec2, rows: usize, row_text: &str, buttons: bool) -> (egui::Rect, Vec<bool>) {
        let mut rect = egui::Rect::NOTHING;
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen = vec![false; 2];
            let _ = ctx.run_ui(screen(size.x, size.y), |ui| {
                let ctx = ui.ctx().clone();
                let dialog = Dialog::new("Tall").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]);
                let shown = if buttons {
                    dialog.show_with_buttons(&ctx, |ui, part| match part {
                        Part::Body => {
                            for row in 0..rows {
                                ui.label(format!("{row_text} {row}"));
                            }
                        }
                        Part::Buttons => {
                            let button = ui.button("OK");
                            seen[1] = ui.clip_rect().intersect(button.rect).height() >= button.rect.height() - 0.5 && ctx.content_rect().contains_rect(button.rect);
                        }
                    })
                } else {
                    dialog.show(&ctx, |ui| {
                        for row in 0..rows {
                            let label = ui.label(format!("{row_text} {row}"));
                            if row == 0 {
                                seen[0] = ctx.content_rect().contains_rect(label.rect);
                            }
                        }
                    })
                };
                rect = shown.map_or(egui::Rect::NOTHING, |shown| shown.response.rect);
            });
        }
        (rect, seen)
    }

    #[test]
    fn a_short_box_is_drawn_as_it_always_was() {
        let ctx = Context::default();
        let (rect, seen) = window_after(&ctx, vec2(800.0, 600.0), 3, "Row", false);
        assert!(ctx.content_rect().contains_rect(rect), "{rect:?}");
        assert!(rect.height() < 200.0, "a short box isn't stretched: {rect:?}");
        assert!(seen[0]);
    }

    #[test]
    fn a_box_taller_than_the_screen_is_kept_on_it_and_scrolls() {
        let ctx = Context::default();
        let (rect, seen) = window_after(&ctx, vec2(800.0, 300.0), 60, "Row", false);
        assert!(ctx.content_rect().expand(0.5).contains_rect(rect), "kept within the screen: {rect:?}");
        assert!(rect.height() > 250.0, "and uses the height there is: {rect:?}");
        assert!(seen[0], "its top row is on the screen");
    }

    #[test]
    fn a_box_wider_than_the_screen_is_kept_on_it() {
        let ctx = Context::default();
        let long = "word ".repeat(10).replace(' ', "\u{A0}");
        let (rect, _) = window_after(&ctx, vec2(300.0, 400.0), 4, &long, false);
        assert!(ctx.content_rect().expand(0.5).contains_rect(rect), "kept within the screen: {rect:?}");
    }

    #[test]
    fn a_box_with_buttons_keeps_them_in_sight() {
        let ctx = Context::default();
        let (rect, seen) = window_after(&ctx, vec2(800.0, 300.0), 60, "Row", true);
        assert!(ctx.content_rect().expand(0.5).contains_rect(rect), "{rect:?}");
        assert!(seen[1], "the OK button is on the screen, not scrolled away");
    }

    #[test]
    fn a_box_goes_back_to_drawing_plainly_when_the_screen_grows() {
        let ctx = Context::default();
        let (small, _) = window_after(&ctx, vec2(800.0, 300.0), 20, "Row", false);
        let (big, _) = window_after(&ctx, vec2(800.0, 1200.0), 20, "Row", false);
        assert!(small.height() <= 300.5);
        assert!(big.height() > small.height() + 50.0, "it grows to show everything: {small:?} {big:?}");
    }

    #[test]
    fn scrolling_starts_past_the_room_and_stops_only_clearly_within_it() {
        assert!(!scrolls(100.0, 100.0, false));
        assert!(scrolls(101.0, 100.0, false));
        assert!(scrolls(100.0, 100.0, true));
        assert!(!scrolls(97.0, 100.0, true));
    }

    #[test]
    fn a_long_menu_scrolls_within_the_screen() {
        let ctx = Context::default();
        let mut menu_rect = egui::Rect::NOTHING;
        for frame in 0..8 {
            let _ = ctx.run_ui(screen(600.0, 300.0), |ui| {
                ui.data_mut(|data| data.insert_temp(opened_menus_key(), vec!["Long".to_string()]));
                let shown = menu_button(ui, "Long", |ui| {
                    for row in 0..80 {
                        let _ = ui.button(format!("Item {row}"));
                    }
                });
                if frame > 0 {
                    assert!(egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&shown.response)), "opened by its name");
                }
            });
            menu_rect = ctx.memory(|memory| {
                memory.areas().visible_layer_ids().into_iter().filter(|layer| layer.order == egui::Order::Foreground).filter_map(|layer| memory.area_rect(layer.id)).fold(egui::Rect::NOTHING, |all, rect| all.union(rect))
            });
        }
        assert!(menu_rect.is_positive(), "the menu was shown");
        assert!(ctx.content_rect().expand(0.5).contains_rect(menu_rect), "{menu_rect:?}");
    }
}
