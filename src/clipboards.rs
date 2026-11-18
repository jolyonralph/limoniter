//! Several clipboards: clipboard 0 is the computer's own, shared with every other program; 1 to 9 are
//! Limoniter's, kept while it runs. Copy, Cut and Paste in the editor (text and hex) use whichever is chosen
//! (Edit, Clipboards, or Ctrl+0 to Ctrl+9), so several things can be held at once and pasted in any order.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

/// How many there are, the computer's included.
pub const COUNT: u8 = 10;

#[cfg(not(test))]
static ACTIVE: AtomicU8 = AtomicU8::new(0);
#[cfg(not(test))]
static HELD: Mutex<[Option<String>; COUNT as usize]> = Mutex::new([const { None }; COUNT as usize]);

// Tests run side by side, each on its own thread, so each has clipboards of its own.
#[cfg(test)]
thread_local! {
    static ACTIVE: AtomicU8 = const { AtomicU8::new(0) };
    static HELD: Mutex<[Option<String>; COUNT as usize]> = const { Mutex::new([const { None }; COUNT as usize]) };
}

fn with_held<T>(f: impl FnOnce(&mut [Option<String>; COUNT as usize]) -> T) -> T {
    #[cfg(not(test))]
    return f(&mut HELD.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    #[cfg(test)]
    return HELD.with(|held| f(&mut held.lock().unwrap_or_else(std::sync::PoisonError::into_inner)));
}

/// The clipboard in use: 0 for the computer's.
pub fn active() -> u8 {
    #[cfg(not(test))]
    return ACTIVE.load(Ordering::Relaxed);
    #[cfg(test)]
    return ACTIVE.with(|a| a.load(Ordering::Relaxed));
}

pub fn choose(number: u8) {
    let number = number.min(COUNT - 1);
    #[cfg(not(test))]
    ACTIVE.store(number, Ordering::Relaxed);
    #[cfg(test)]
    ACTIVE.with(|a| a.store(number, Ordering::Relaxed));
}

/// Its name, as the menu and status bar give it.
pub fn name(number: u8) -> String {
    if number == 0 { crate::i18n::tr("The computer's clipboard").to_string() } else { crate::i18n::tf!("Clipboard {number}") }
}

/// Puts copied text on the clipboard in use.
pub fn put(ctx: &eframe::egui::Context, text: String) {
    match active() {
        0 => ctx.copy_text(text),
        n => with_held(|held| held[usize::from(n)] = Some(text)),
    }
}

/// What a paste brings: what the computer's clipboard gave, or what the one in use holds (nothing, if it's empty).
pub fn pasted(from_the_computer: &str) -> String {
    match active() {
        0 => from_the_computer.to_string(),
        n => with_held(|held| held[usize::from(n)].clone().unwrap_or_default()),
    }
}

/// What one of Limoniter's own clipboards holds.
pub fn holding(number: u8) -> Option<String> {
    with_held(|held| held.get(usize::from(number)).cloned().flatten())
}

/// Empties one of Limoniter's own.
pub fn clear(number: u8) {
    with_held(|held| {
        if let Some(slot) = held.get_mut(usize::from(number)) {
            *slot = None;
        }
    });
}

#[cfg(test)]
pub fn reset() {
    choose(0);
    for n in 1..COUNT {
        clear(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_clipboard_keeps_its_own_and_the_computer_s_is_left_alone() {
        reset();
        let ctx = eframe::egui::Context::default();
        choose(3);
        put(&ctx, "three".into());
        choose(7);
        put(&ctx, "seven".into());
        assert_eq!(pasted("from the computer"), "seven");
        choose(3);
        assert_eq!(pasted("from the computer"), "three");
        choose(5);
        assert_eq!(pasted("from the computer"), "", "an empty one pastes nothing");
        choose(0);
        assert_eq!(pasted("from the computer"), "from the computer");
        assert_eq!((holding(3).as_deref(), holding(7).as_deref()), (Some("three"), Some("seven")));
        clear(3);
        assert_eq!(holding(3), None);
        choose(42);
        assert_eq!(active(), 9);
        reset();
    }
}
