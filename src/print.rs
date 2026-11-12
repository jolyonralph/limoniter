//! Printing: Windows' own print dialog, then the text laid out in pages and sent to the printer on another thread.

use crate::codec::TextFormat;
use crate::text_buffer::{expand_tabs, is_special};

const PAGE_BREAK: char = '\u{C}';

/// Text ready to print: exact bytes shown as what they mean, tabs as spaces, and other controls as a dot.
/// A page break stays, because it starts a new page.
pub fn printable_rows<S: AsRef<str>>(lines: &[S], format: &TextFormat) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let readable = format.readable_text(line.as_ref());
            let plain = expand_tabs(&readable);
            plain.chars().map(|c| if c != PAGE_BREAK && is_special(c) { '\u{B7}' } else { c }).collect()
        })
        .collect()
}

/// Splits rows into pages. Long rows continue on the next row, and a page break starts a new page.
pub fn paginate(rows: &[String], columns: usize, per_page: usize, numbers: bool) -> Vec<Vec<String>> {
    let per_page = per_page.max(1);
    let number_width = if numbers { rows.len().max(1).to_string().len() + 1 } else { 0 };
    let body_width = columns.saturating_sub(number_width).max(1);
    let mut pages = Vec::new();
    let mut page: Vec<String> = Vec::with_capacity(per_page);
    for (index, row) in rows.iter().enumerate() {
        for (part_number, part) in row.split(PAGE_BREAK).enumerate() {
            if part_number > 0 {
                pages.push(std::mem::take(&mut page));
            }
            let chars: Vec<char> = part.chars().collect();
            let mut pieces = chars.chunks(body_width);
            let mut piece = pieces.next().unwrap_or(&[]);
            // A row of nothing but a page break doesn't leave a blank row behind it.
            let skip_blank = part_number > 0 && part.is_empty() && row.split(PAGE_BREAK).count() == part_number + 1;
            let mut first = part_number == 0;
            if !skip_blank {
                loop {
                    if page.len() == per_page {
                        pages.push(std::mem::take(&mut page));
                    }
                    let lead = if !numbers {
                        String::new()
                    } else if first {
                        format!("{:>width$} ", index + 1, width = number_width - 1)
                    } else {
                        " ".repeat(number_width)
                    };
                    page.push(format!("{lead}{}", piece.iter().collect::<String>()));
                    first = false;
                    match pieces.next() {
                        Some(next) => piece = next,
                        None => break,
                    }
                }
            }
        }
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages
}

/// Keeps the pages asked for, counting from 1.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn pick_pages(pages: Vec<Vec<String>>, range: Option<(usize, usize)>) -> Vec<(usize, Vec<String>)> {
    let (from, to) = range.unwrap_or((1, usize::MAX));
    pages.into_iter().enumerate().map(|(i, page)| (i + 1, page)).filter(|(number, _)| *number >= from && *number <= to).collect()
}

/// What to print. The selection is offered in the dialog if there is one.
pub struct Job {
    pub title: String,
    pub rows: Vec<String>,
    pub selection: Option<Vec<String>>,
    pub header: bool,
    pub line_numbers: bool,
}

/// How a job that's been sent came out: the pages sent, or what went wrong.
pub type Outcome = std::sync::Arc<std::sync::Mutex<Option<Result<usize, String>>>>;

#[cfg(windows)]
mod windows {
    use super::{Job, Outcome, paginate, pick_pages};
    use std::ffi::c_void;
    use std::sync::{Arc, Mutex};

    /// The layout Windows expects on 64-bit, where this isn't packed.
    #[repr(C)]
    pub(super) struct PrintDialog {
        pub size: u32,
        pub owner: isize,
        pub dev_mode: isize,
        pub dev_names: isize,
        pub dc: isize,
        pub flags: u32,
        pub from_page: u16,
        pub to_page: u16,
        pub min_page: u16,
        pub max_page: u16,
        pub copies: u16,
        pub instance: isize,
        pub custom: isize,
        pub print_hook: *const c_void,
        pub setup_hook: *const c_void,
        pub print_template: *const u16,
        pub setup_template: *const u16,
        pub print_template_handle: isize,
        pub setup_template_handle: isize,
    }

    #[repr(C)]
    struct DocInfo {
        size: i32,
        name: *const u16,
        output: *const u16,
        datatype: *const u16,
        kind: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Size {
        x: i32,
        y: i32,
    }

    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn PrintDlgW(dialog: *mut PrintDialog) -> i32;
        fn CommDlgExtendedError() -> u32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetDeviceCaps(dc: isize, what: i32) -> i32;
        fn CreateFontW(height: i32, width: i32, escapement: i32, orientation: i32, weight: i32, italic: u32, underline: u32, strike: u32, charset: u32, precision: u32, clip: u32, quality: u32, pitch: u32, face: *const u16) -> isize;
        fn SelectObject(dc: isize, object: isize) -> isize;
        fn DeleteObject(object: isize) -> i32;
        fn DeleteDC(dc: isize) -> i32;
        fn GetTextExtentPoint32W(dc: isize, text: *const u16, count: i32, size: *mut Size) -> i32;
        fn SetBkMode(dc: isize, mode: i32) -> i32;
        fn TextOutW(dc: isize, x: i32, y: i32, text: *const u16, count: i32) -> i32;
        fn StartDocW(dc: isize, info: *const DocInfo) -> i32;
        fn StartPage(dc: isize) -> i32;
        fn EndPage(dc: isize) -> i32;
        fn EndDoc(dc: isize) -> i32;
        fn AbortDoc(dc: isize) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalFree(memory: isize) -> isize;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetActiveWindow() -> isize;
    }

    const PD_SELECTION: u32 = 0x1;
    const PD_PAGENUMS: u32 = 0x2;
    const PD_NOSELECTION: u32 = 0x4;
    const PD_RETURNDC: u32 = 0x100;
    const PD_USEDEVMODECOPIESANDCOLLATE: u32 = 0x4_0000;
    const HORZRES: i32 = 8;
    const VERTRES: i32 = 10;
    const LOGPIXELSX: i32 = 88;
    const LOGPIXELSY: i32 = 90;
    const PHYSICALOFFSETX: i32 = 112;
    const PHYSICALOFFSETY: i32 = 113;
    const TRANSPARENT: i32 = 1;
    /// The most page numbers the dialog's boxes can hold.
    const LAST_PAGE: u16 = 9999;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Where things go on a page, in the printer's own dots.
    pub(super) struct Layout {
        pub left: i32,
        pub top: i32,
        pub line_height: i32,
        pub columns: usize,
        pub per_page: usize,
        pub header_rows: i32,
    }

    /// Sets the font on the device and works out the layout. The margins are three quarters of an inch.
    pub(super) fn set_up(dc: isize, header: bool) -> Result<(Layout, isize), String> {
        // SAFETY: the device context is valid, and these only read its details.
        let (width, height, dpi_x, dpi_y, offset_x, offset_y) = unsafe {
            (GetDeviceCaps(dc, HORZRES), GetDeviceCaps(dc, VERTRES), GetDeviceCaps(dc, LOGPIXELSX), GetDeviceCaps(dc, LOGPIXELSY), GetDeviceCaps(dc, PHYSICALOFFSETX), GetDeviceCaps(dc, PHYSICALOFFSETY))
        };
        if width <= 0 || height <= 0 || dpi_y <= 0 {
            return Err("That printer didn't say how big its page is.".to_string());
        }
        const FF_MODERN: u32 = 0x30;
        const FIXED_PITCH: u32 = 1;
        const DEFAULT_CHARSET: u32 = 1;
        let face = wide("Consolas");
        let font_height = -(10 * dpi_y / 72);
        // SAFETY: the face name is terminated and outlives the call.
        let font = unsafe { CreateFontW(font_height, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, 0, 0, 0, FF_MODERN | FIXED_PITCH, face.as_ptr()) };
        if font == 0 {
            return Err("Windows couldn't make a font for printing.".to_string());
        }
        let letter = wide("M");
        let mut cell = Size::default();
        // SAFETY: the device, font and text are all valid, and the text is one character long.
        unsafe {
            SelectObject(dc, font);
            SetBkMode(dc, TRANSPARENT);
            GetTextExtentPoint32W(dc, letter.as_ptr(), 1, &raw mut cell);
        }
        if cell.x <= 0 || cell.y <= 0 {
            // SAFETY: the font was made above, and the device can do without it.
            unsafe {
                DeleteObject(font);
            }
            return Err("That printer couldn't measure the text.".to_string());
        }
        let left = (dpi_x * 3 / 4 - offset_x).max(0);
        let top = (dpi_y * 3 / 4 - offset_y).max(0);
        let header_rows = if header { 2 } else { 0 };
        let columns = ((width - 2 * left) / cell.x).max(1) as usize;
        let per_page = (((height - 2 * top) / cell.y) - header_rows).max(1) as usize;
        Ok((Layout { left, top, line_height: cell.y, columns, per_page, header_rows }, font))
    }

    fn put_text(dc: isize, x: i32, y: i32, text: &str) {
        let units: Vec<u16> = text.encode_utf16().collect();
        if !units.is_empty() {
            // SAFETY: the count is exactly the length of the text, which outlives the call.
            unsafe {
                TextOutW(dc, x, y, units.as_ptr(), units.len() as i32);
            }
        }
    }

    pub(super) fn draw_page(dc: isize, layout: &Layout, header: Option<&str>, rows: &[String]) {
        if let Some(text) = header {
            put_text(dc, layout.left, layout.top, text);
        }
        let mut y = layout.top + layout.header_rows * layout.line_height;
        for row in rows {
            put_text(dc, layout.left, y, row);
            y += layout.line_height;
        }
    }

    fn send(dc: isize, job: &Job, rows: &[String], range: Option<(usize, usize)>) -> Result<usize, String> {
        let (layout, font) = set_up(dc, job.header)?;
        let pages = paginate(rows, layout.columns, layout.per_page, job.line_numbers);
        let total = pages.len();
        let chosen = pick_pages(pages, range);
        if chosen.is_empty() {
            // SAFETY: the font was made by set_up and isn't needed now.
            unsafe {
                DeleteObject(font);
            }
            return Err(format!("It has only {total} page{}, so those pages don't exist.", if total == 1 { "" } else { "s" }));
        }
        let name = wide(&job.title);
        let info = DocInfo { size: std::mem::size_of::<DocInfo>() as i32, name: name.as_ptr(), output: std::ptr::null(), datatype: std::ptr::null(), kind: 0 };
        // SAFETY: the name outlives the document, and the rest of the structure may be null.
        if unsafe { StartDocW(dc, &raw const info) } <= 0 {
            // SAFETY: as above.
            unsafe {
                DeleteObject(font);
            }
            return Err("The printer wouldn't start the document.".to_string());
        }
        let mut sent = 0;
        let mut problem = None;
        for (number, page) in &chosen {
            // SAFETY: the document was started above and is still open.
            if unsafe { StartPage(dc) } <= 0 {
                problem = Some("The printer wouldn't start a page.");
                break;
            }
            // Some printers forget the font between pages.
            // SAFETY: the font and device are both still valid.
            unsafe {
                SelectObject(dc, font);
                SetBkMode(dc, TRANSPARENT);
            }
            let header = job.header.then(|| format!("{}    Page {number} of {total}", job.title));
            draw_page(dc, &layout, header.as_deref(), page);
            // SAFETY: the page was started above.
            if unsafe { EndPage(dc) } <= 0 {
                problem = Some("The printer stopped partway through.");
                break;
            }
            sent += 1;
        }
        // SAFETY: the document is open, and is ended once, one way or the other.
        unsafe {
            if problem.is_some() {
                AbortDoc(dc);
            } else {
                EndDoc(dc);
            }
            DeleteObject(font);
        }
        match problem {
            Some(text) => Err(text.to_string()),
            None => Ok(sent),
        }
    }

    /// Asks with Windows' print dialog, then prints on another thread. None if the person cancelled.
    pub fn ask_and_print(job: Job) -> Result<Option<Outcome>, String> {
        let mut dialog = PrintDialog {
            size: std::mem::size_of::<PrintDialog>() as u32,
            // SAFETY: this takes nothing and only reads which window is active.
            owner: unsafe { GetActiveWindow() },
            dev_mode: 0,
            dev_names: 0,
            dc: 0,
            flags: PD_RETURNDC | PD_USEDEVMODECOPIESANDCOLLATE | if job.selection.is_some() { PD_SELECTION } else { PD_NOSELECTION },
            from_page: 1,
            to_page: 1,
            min_page: 1,
            max_page: LAST_PAGE,
            copies: 1,
            instance: 0,
            custom: 0,
            print_hook: std::ptr::null(),
            setup_hook: std::ptr::null(),
            print_template: std::ptr::null(),
            setup_template: std::ptr::null(),
            print_template_handle: 0,
            setup_template_handle: 0,
        };
        // SAFETY: the structure is the size it says it is, and lives for the whole call.
        let accepted = unsafe { PrintDlgW(&raw mut dialog) };
        // SAFETY: these two came from the dialog, are this program's to free, and aren't used after this.
        unsafe {
            if dialog.dev_mode != 0 {
                GlobalFree(dialog.dev_mode);
            }
            if dialog.dev_names != 0 {
                GlobalFree(dialog.dev_names);
            }
        }
        if accepted == 0 {
            // SAFETY: this takes nothing, and reports on the call just made.
            let code = unsafe { CommDlgExtendedError() };
            return if code == 0 { Ok(None) } else { Err(format!("Windows couldn't show the print window (error {code:#x}). Is a printer installed?")) };
        }
        let dc = dialog.dc;
        let selection_only = dialog.flags & PD_SELECTION != 0;
        let range = (dialog.flags & PD_PAGENUMS != 0).then_some((usize::from(dialog.from_page), usize::from(dialog.to_page)));
        let outcome: Outcome = Arc::new(Mutex::new(None));
        let report = outcome.clone();
        std::thread::spawn(move || {
            let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let rows = if selection_only { job.selection.as_deref().unwrap_or(&job.rows) } else { &job.rows };
                send(dc, &job, rows, range)
            }));
            // SAFETY: the dialog gave this device to this program, and only this thread has used it since.
            unsafe {
                DeleteDC(dc);
            }
            *report.lock().unwrap() = Some(attempt.unwrap_or_else(|_| Err("Printing failed unexpectedly.".to_string())));
        });
        Ok(Some(outcome))
    }

    #[cfg(test)]
    mod tests {

        use super::*;

        #[link(name = "gdi32")]
        unsafe extern "system" {
            fn CreateCompatibleDC(dc: isize) -> isize;
            fn CreateBitmap(width: i32, height: i32, planes: u32, bits: u32, data: *const c_void) -> isize;
            fn PatBlt(dc: isize, x: i32, y: i32, width: i32, height: i32, operation: u32) -> i32;
            fn GetPixel(dc: isize, x: i32, y: i32) -> u32;
        }

        #[test]
        #[cfg(target_pointer_width = "64")]
        fn the_dialog_structure_is_the_size_windows_expects() {
            assert_eq!(std::mem::size_of::<PrintDialog>(), 120);
        }

        #[test]
        fn a_page_really_gets_drawn() {
            const WHITENESS: u32 = 0x00FF_0062;
            const WHITE: u32 = 0x00FF_FFFF;
            // A picture in memory stands in for the paper, so nothing goes near a printer.
            // SAFETY: each of these is checked before use, and all are released at the end.
            unsafe {
                let dc = CreateCompatibleDC(0);
                assert_ne!(dc, 0);
                let picture = CreateBitmap(800, 800, 1, 32, std::ptr::null());
                assert_ne!(picture, 0);
                let earlier = SelectObject(dc, picture);
                PatBlt(dc, 0, 0, 800, 800, WHITENESS);
                let (layout, font) = set_up(dc, true).unwrap();
                assert!(layout.columns > 1 && layout.per_page >= 1);
                let dark_dots = |from_y: i32, to_y: i32| {
                    let mut dark = 0;
                    for y in from_y..to_y {
                        for x in 0..800 {
                            if GetPixel(dc, x, y) != WHITE {
                                dark += 1;
                            }
                        }
                    }
                    dark
                };
                assert_eq!(dark_dots(0, 800), 0, "blank to begin with");
                let rows = ["MMMM".to_string()];
                draw_page(dc, &Layout { left: 10, top: 10, line_height: layout.line_height, columns: 80, per_page: 10, header_rows: 2 }, Some("HEADER"), &rows);
                let height = layout.line_height;
                assert!(dark_dots(10, 10 + height) > 0, "the header is on its row");
                assert_eq!(dark_dots(10 + height, 10 + 2 * height), 0, "with a blank row after it");
                assert!(dark_dots(10 + 2 * height, 10 + 3 * height) > 0, "then the text");
                SelectObject(dc, earlier);
                DeleteObject(picture);
                DeleteObject(font);
                DeleteDC(dc);
            }
        }
    }
}

#[cfg(windows)]
pub use windows::ask_and_print;

/// Plain text pages for a printer: each page's rows, with the title and page number above when asked, and a form
/// feed between pages.
#[cfg_attr(windows, allow(dead_code))]
pub fn text_pages(job: &Job, columns: usize, per_page: usize) -> String {
    let rows = job.selection.as_ref().unwrap_or(&job.rows);
    let body = if job.header { per_page.saturating_sub(2) } else { per_page };
    let pages = paginate(rows, columns, body, job.line_numbers);
    let count = pages.len();
    let mut out = String::new();
    for (number, page) in pages.into_iter().enumerate() {
        if number > 0 {
            out.push('\u{c}');
        }
        if job.header {
            let place = format!("Page {} of {count}", number + 1);
            let title: String = job.title.chars().take(columns.saturating_sub(place.len() + 2)).collect();
            out += &format!("{title}{}{place}\n\n", " ".repeat(columns.saturating_sub(title.chars().count() + place.len())));
        }
        for row in page {
            out += &row;
            out.push('\n');
        }
    }
    out
}

/// On macOS and Linux, pages go to the default printer through CUPS, as `lp` sends them.
#[cfg(not(windows))]
pub fn ask_and_print(job: Job) -> Result<Option<Outcome>, String> {
    use std::io::Write;
    let text = text_pages(&job, 80, 62);
    let pages = text.matches('\u{c}').count() + 1;
    let mut child = std::process::Command::new("lp")
        .args(["-t", &job.title])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Printing uses the system's lp command, which couldn't be started.\n\n{e}"))?;
    child.stdin.take().map(|mut input| input.write_all(text.as_bytes())).transpose().map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!("The printer system said: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(Some(std::sync::Arc::new(std::sync::Mutex::new(Some(Ok(pages))))))
}

#[cfg(test)]
mod tests {
    #[test]
    fn text_pages_have_headers_and_breaks() {
        let job = super::Job { title: "notes.txt".into(), rows: (1..=7).map(|n| format!("row {n}")).collect(), selection: None, header: true, line_numbers: false };
        let text = super::text_pages(&job, 30, 5);
        let pages: Vec<&str> = text.split('\u{c}').collect();
        assert_eq!(pages.len(), 3);
        assert!(pages[0].starts_with("notes.txt") && pages[0].lines().next().unwrap().ends_with("Page 1 of 3"));
        assert_eq!(pages[0].lines().next().unwrap().len(), 30);
        assert!(pages[2].contains("row 7"));
    }

    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn rows_fill_pages_in_order() {
        let pages = paginate(&rows(&["a", "b", "c", "d", "e"]), 10, 2, false);
        assert_eq!(pages, vec![rows(&["a", "b"]), rows(&["c", "d"]), rows(&["e"])]);
    }

    #[test]
    fn long_rows_continue_and_nothing_is_lost() {
        let pages = paginate(&rows(&["abcdefgh"]), 3, 10, false);
        assert_eq!(pages, vec![rows(&["abc", "def", "gh"])]);
        let across = paginate(&rows(&["abcdefgh"]), 3, 2, false);
        assert_eq!(across, vec![rows(&["abc", "def"]), rows(&["gh"])]);
    }

    #[test]
    fn empty_rows_are_kept_and_an_empty_file_is_one_blank_page() {
        assert_eq!(paginate(&rows(&["a", "", "b"]), 5, 5, false), vec![rows(&["a", "", "b"])]);
        assert_eq!(paginate(&rows(&[""]), 5, 5, false), vec![rows(&[""])]);
        assert_eq!(paginate(&[], 5, 5, false).len(), 1);
    }

    #[test]
    fn a_page_break_starts_a_new_page() {
        assert_eq!(paginate(&rows(&["a", "\u{C}", "b"]), 5, 5, false), vec![rows(&["a", ""]), rows(&["b"])]);
        assert_eq!(paginate(&rows(&["a\u{C}b"]), 5, 5, false), vec![rows(&["a"]), rows(&["b"])]);
    }

    #[test]
    fn line_numbers_line_up_and_only_go_on_a_rows_first_part() {
        let many: Vec<String> = (0..12).map(|_| "abcdef".to_string()).collect();
        let pages = paginate(&many, 6, 100, true);
        assert_eq!(pages[0][0], " 1 abc");
        assert_eq!(pages[0][1], "   def");
        assert_eq!(pages[0][18], "10 abc");
    }

    #[test]
    fn narrow_or_nonsense_sizes_still_make_progress() {
        assert_eq!(paginate(&rows(&["abc"]), 0, 0, false).len(), 3);
        assert_eq!(paginate(&rows(&["abc"]), 1, 100, true)[0].len(), 3);
    }

    #[test]
    fn page_ranges_keep_their_numbers() {
        let pages = paginate(&rows(&["a", "b", "c"]), 5, 1, false);
        assert_eq!(pick_pages(pages.clone(), Some((2, 3))).iter().map(|(number, _)| *number).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(pick_pages(pages.clone(), None).len(), 3);
        assert!(pick_pages(pages, Some((9, 9))).is_empty());
    }

    #[test]
    fn printable_text_shows_tabs_as_spaces_and_controls_as_dots() {
        crate::text_buffer::set_tab_size(4);
        let format = TextFormat::default();
        assert_eq!(printable_rows(&["a\tb\u{1}c\u{C}"], &format), ["a   b\u{B7}c\u{C}"]);
        crate::text_buffer::set_tab_size(crate::text_buffer::DEFAULT_TAB_SIZE);
    }
}
