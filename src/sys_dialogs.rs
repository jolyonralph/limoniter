//! The system's own dialogs (file pickers and message boxes), opened over Limoniter's window. Given the window
//! as their parent, the system keeps them in front of it and modal: a click on the window behind can't send one
//! to the back, which would leave Limoniter looking frozen while it waits for an answer nobody can see.
//!
//! Every dialog is made here (clippy refuses `rfd::FileDialog::new` and `rfd::MessageDialog::new` elsewhere).

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle};
use std::sync::OnceLock;

/// Limoniter's main window, as the system knows it.
struct Handles {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

// SAFETY: these are only the system's names for the main window, which lasts as long as the program. They're
// handed to the system's dialog code, from whichever thread opens a dialog, and never used to reach memory.
unsafe impl Send for Handles {}
unsafe impl Sync for Handles {}

static MAIN: OnceLock<Handles> = OnceLock::new();

/// Remembers the main window once it exists, so the dialogs opened after can sit over it.
pub fn remember_window(window: &(impl HasWindowHandle + HasDisplayHandle)) {
    if let (Ok(w), Ok(d)) = (window.window_handle(), window.display_handle()) {
        let _ = MAIN.set(Handles { window: w.as_raw(), display: d.as_raw() });
    }
}

/// The main window as a dialog's parent.
struct Parent(&'static Handles);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the main window outlives every dialog (see `Handles`).
        Ok(unsafe { WindowHandle::borrow_raw(self.0.window) })
    }
}

impl HasDisplayHandle for Parent {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: as above.
        Ok(unsafe { DisplayHandle::borrow_raw(self.0.display) })
    }
}

/// A file picker (open, save or choose a folder) over Limoniter's window.
#[allow(clippy::disallowed_methods)] // The one place a file dialog is made.
pub fn file_dialog() -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    match MAIN.get() {
        Some(main) => dialog.set_parent(&Parent(main)),
        None => dialog,
    }
}

/// A message box (a question, a warning) over Limoniter's window.
#[allow(clippy::disallowed_methods)] // The one place a message box is made.
pub fn message_dialog() -> rfd::MessageDialog {
    let dialog = rfd::MessageDialog::new();
    match MAIN.get() {
        Some(main) => dialog.set_parent(&Parent(main)),
        None => dialog,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dialogs_can_be_made_before_and_without_a_window() {
        // Before the window exists (a message at startup, or in tests), a dialog is made with no parent.
        let _ = super::file_dialog().set_title("x");
        let _ = super::message_dialog().set_title("x");
    }
}
