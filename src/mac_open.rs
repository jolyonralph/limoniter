//! Files opened from Finder on a Mac: double-clicked, dropped on the Dock icon, or chosen with Open With.
//!
//! macOS hands them over as an "open documents" Apple event rather than on the command line, and the window
//! library doesn't pass that event on. So a handler is installed for it just before the app finishes launching,
//! as Apple advises, which catches both the files a launch was for and files opened while Limoniter is running.
//! The window picks them up from here each frame.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{ClassType, define_class, msg_send, sel};
use std::ffi::{CStr, c_char};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Apple's four-character codes: the core event class, "open documents", and the event's main parameter.
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WINDOW: OnceLock<eframe::egui::Context> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "LimoniterFileOpener"]
    struct Opener;

    impl Opener {
        /// Just before launching finishes: the moment to take over "open documents" from AppKit.
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &AnyObject) {
            let Some(class) = AnyClass::get(c"NSAppleEventManager") else {
                return;
            };
            // SAFETY: the shared manager is always there, and the selector is this object's own method below.
            unsafe {
                let manager: Retained<AnyObject> = msg_send![class, sharedAppleEventManager];
                let () = msg_send![&*manager, setEventHandler: self, andSelector: sel!(handleOpen:withReply:), forEventClass: CORE_EVENT_CLASS, andEventID: OPEN_DOCUMENTS];
            }
        }

        /// The files to open, each as a file URL.
        #[unsafe(method(handleOpen:withReply:))]
        fn handle_open(&self, event: &AnyObject, _reply: &AnyObject) {
            let mut paths = Vec::new();
            // SAFETY: an "open documents" event's direct object is a list of file references, each of which turns
            // into a file URL; every message here is one NSAppleEventDescriptor and NSURL answer.
            unsafe {
                let list: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
                let Some(list) = list else {
                    return;
                };
                let count: isize = msg_send![&*list, numberOfItems];
                if count == 0 {
                    paths.extend(path_of(&list));
                }
                for i in 1..=count {
                    let item: Option<Retained<AnyObject>> = msg_send![&*list, descriptorAtIndex: i];
                    if let Some(item) = item {
                        paths.extend(path_of(&item));
                    }
                }
            }
            if !paths.is_empty() {
                OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(paths);
                if let Some(window) = WINDOW.get() {
                    window.request_repaint();
                }
            }
        }
    }
);

/// The path a file reference in an Apple event is for.
unsafe fn path_of(descriptor: &AnyObject) -> Option<PathBuf> {
    // SAFETY: fileURLValue is answered by every NSAppleEventDescriptor (nil if it isn't a file), path by every
    // NSURL, and UTF8String by every NSString, whose bytes last as long as the string.
    unsafe {
        let url: Option<Retained<AnyObject>> = msg_send![descriptor, fileURLValue];
        let path: Option<Retained<AnyObject>> = msg_send![&*url?, path];
        let text: *const c_char = msg_send![&*path?, UTF8String];
        (!text.is_null()).then(|| PathBuf::from(CStr::from_ptr(text).to_string_lossy().into_owned()))
    }
}

/// Starts listening for files from Finder. Called once, before the window is made.
pub fn start() {
    // SAFETY: plain Foundation calls: a new object of the class above, kept for the life of the program, observing
    // NSApplicationWillFinishLaunchingNotification, whose name is the same string.
    unsafe {
        let opener: Retained<Opener> = msg_send![Opener::class(), new];
        let (Some(strings), Some(centres)) = (AnyClass::get(c"NSString"), AnyClass::get(c"NSNotificationCenter")) else {
            return;
        };
        let name: Retained<AnyObject> = msg_send![strings, stringWithUTF8String: c"NSApplicationWillFinishLaunchingNotification".as_ptr()];
        let centre: Retained<AnyObject> = msg_send![centres, defaultCenter];
        let () = msg_send![&*centre, addObserver: &*opener, selector: sel!(willFinishLaunching:), name: &*name, object: Option::<&AnyObject>::None];
        std::mem::forget(opener);
    }
}

/// Tells the window to look when files arrive, once it's there.
pub fn set_window(window: eframe::egui::Context) {
    let _ = WINDOW.set(window);
}

/// The files that have arrived since the last look.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}
