//! The AutoDoc reader's shelf: every page of the Amiga NDK's autodocs, and of MUI's when the person has pointed
//! to them, kept by library (a "book") so they can be browsed as MinAD browses them, filtered as you type, and
//! found from a name in C or 68000 source (AllocMem, _LVOAllocMem, OpenWindowTags, MUIA_Window_Title).
//!
//! An autodoc is plain text: a table of contents, then a page per function, each starting with a form feed and a
//! title such as `exec.library/AllocMem`. Some copies have lost their form feeds on the way (MUI's on old CDs),
//! so then a page starts where a line is one of the titles the table of contents lists.
//!
//! MUI (the Magic User Interface) is Stefan Stuntz's, and its developer files are free to download but not under
//! a licence that lets anyone else hand them out, so Limoniter doesn't carry them: the person points to the
//! Autodocs folder of the MUI SDK they have, and the choice is remembered beside the settings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Where a book of pages came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Ndk,
    Mui,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Ndk => "NDK",
            Source::Mui => "MUI",
        }
    }
}

/// One function's (or attribute's, method's or background) page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub text: String,
}

/// One library's, device's or class's pages, as its autodoc gives them.
#[derive(Clone, Debug)]
pub struct Book {
    pub name: String,
    pub source: Source,
    pub pages: Vec<Page>,
}

/// A page's place on the shelf, by name, so it still means the same page once the shelf is read again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub book: String,
    pub page: String,
}

/// Every book, and every page by its name.
#[derive(Debug, Default)]
pub struct Shelf {
    pub books: Vec<Book>,
    /// Pages by their names in small letters: (book, page).
    names: HashMap<String, Vec<(usize, usize)>>,
}

/// Tabs as the autodocs were written for them: every eight columns.
fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let to = (column / 8 + 1) * 8;
            out.extend(std::iter::repeat_n(' ', to - column));
            column = to;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

/// A page's title (`exec.library/AllocMem`) split into its book and name, if a line starts with one.
fn title_of(line: &str) -> Option<(&str, &str)> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let title = line.split_whitespace().next()?;
    let (book, name) = title.rsplit_once('/')?;
    (!book.is_empty() && !name.is_empty() && !book.contains(['(', ')', ',']) && !name.contains(['(', ')', ','])).then_some((book, name))
}

/// The lines of a page, without the blank lines around them.
fn page_text(lines: &[&str]) -> String {
    let first = lines.iter().position(|l| !l.trim().is_empty()).unwrap_or(lines.len());
    let last = lines.iter().rposition(|l| !l.trim().is_empty()).map_or(first, |i| i + 1);
    lines[first..last.max(first)].iter().map(|l| expand_tabs(l.trim_end())).collect::<Vec<_>>().join("\n")
}

/// Reads one autodoc file's pages: (book, page name, its text).
pub fn read_pages(text: &str) -> Vec<(String, String, String)> {
    let mut pages = Vec::new();
    if text.contains('\x0c') {
        for chunk in text.split('\x0c') {
            let lines: Vec<&str> = chunk.lines().skip_while(|l| l.trim().is_empty()).collect();
            let Some((book, name)) = lines.first().and_then(|l| title_of(l)) else { continue };
            pages.push((book.to_string(), name.to_string(), page_text(&lines[1..])));
        }
        return pages;
    }
    // No form feeds: the table of contents says which lines start pages.
    let lines: Vec<&str> = text.lines().collect();
    let Some(contents) = lines.iter().position(|l| l.trim().eq_ignore_ascii_case("TABLE OF CONTENTS")) else {
        return pages;
    };
    let mut titles = std::collections::HashSet::new();
    let mut at = contents + 1;
    while at < lines.len() && lines[at].trim().is_empty() {
        at += 1;
    }
    while at < lines.len() && title_of(lines[at]).is_some() && lines[at].split_whitespace().count() == 1 {
        titles.insert(lines[at].trim());
        at += 1;
    }
    let mut current: Option<(usize, &str, &str)> = None;
    for (index, line) in lines.iter().enumerate().skip(at) {
        let Some((book, name)) = title_of(line).filter(|_| line.split_whitespace().next().is_some_and(|t| titles.contains(t))) else { continue };
        if let Some((start, b, n)) = current {
            pages.push((b.to_string(), n.to_string(), page_text(&lines[start..index])));
        }
        current = Some((index + 1, book, name));
    }
    if let Some((start, b, n)) = current {
        pages.push((b.to_string(), n.to_string(), page_text(&lines[start..])));
    }
    pages
}

/// An autodoc file's text: they're Latin-1, as the Amiga wrote them.
fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| bytes.iter().map(|&b| b as char).collect())
}

/// The autodoc files in a folder and the folders in it, a few deep: the person may point to the SDK's top.
pub fn doc_files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![(folder.to_path_buf(), 0)];
    while let Some((dir, depth)) = todo.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                if depth < 3 {
                    todo.push((path, depth + 1));
                }
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("doc")) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

impl Shelf {
    /// Reads the autodoc files given, in order, each with where it came from.
    pub fn read(files: &[(Source, PathBuf)]) -> Shelf {
        let mut shelf = Shelf::default();
        for (source, path) in files {
            let Some(text) = read_text(path) else { continue };
            shelf.add(*source, &text);
        }
        shelf.books.sort_by_key(|b| (b.source != Source::Ndk, b.name.to_lowercase()));
        shelf.index();
        shelf
    }

    /// Adds one file's pages, each to its book (a file usually has one, but may have more).
    pub fn add(&mut self, source: Source, text: &str) {
        for (book, name, text) in read_pages(text) {
            let place = match self.books.iter().position(|b| b.name == book && b.source == source) {
                Some(place) => place,
                None => {
                    self.books.push(Book { name: book, source, pages: Vec::new() });
                    self.books.len() - 1
                }
            };
            self.books[place].pages.push(Page { name, text });
        }
    }

    /// Makes the index of pages by name; done once the books are in their order.
    pub fn index(&mut self) {
        self.names.clear();
        for (b, book) in self.books.iter().enumerate() {
            for (p, page) in book.pages.iter().enumerate() {
                self.names.entry(page.name.to_lowercase()).or_default().push((b, p));
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    pub fn page_count(&self) -> usize {
        self.books.iter().map(|b| b.pages.len()).sum()
    }

    pub fn page(&self, at: (usize, usize)) -> Option<(&Book, &Page)> {
        let book = self.books.get(at.0)?;
        Some((book, book.pages.get(at.1)?))
    }

    pub fn place(&self, at: (usize, usize)) -> Option<Place> {
        self.page(at).map(|(book, page)| Place { book: book.name.clone(), page: page.name.clone() })
    }

    pub fn locate(&self, place: &Place) -> Option<(usize, usize)> {
        let b = self.books.iter().position(|b| b.name == place.book)?;
        Some((b, self.books[b].pages.iter().position(|p| p.name == place.page)?))
    }

    /// A page by its very name, written as it is first, then in any case.
    fn named(&self, name: &str) -> Option<(usize, usize)> {
        if let Some((book, page)) = name.rsplit_once('/') {
            let b = self.books.iter().position(|b| b.name.eq_ignore_ascii_case(book))?;
            return Some((b, self.books[b].pages.iter().position(|p| p.name.eq_ignore_ascii_case(page))?));
        }
        let all = self.names.get(&name.to_lowercase())?;
        all.iter().find(|(b, p)| self.books[*b].pages[*p].name == name).or(all.first()).copied()
    }

    /// The page explaining a word from source: a function (AllocMem, _LVOAllocMem, or a stub's _AllocMem), its
    /// tag-list form (OpenWindowTags is explained under OpenWindowTagList, MUI_NewObject under MUI_NewObjectA),
    /// another name in the same slot of the library's jump table, an MUI attribute or method (MUIA_, MUIM_), or a
    /// class (MUIC_Window, whose first page is about the class).
    pub fn find(&self, word: &str, ndk: Option<&crate::amiga_ndk::Index>) -> Option<(usize, usize)> {
        let word = word.trim_matches(|c: char| !(c.is_alphanumeric() || c == '_'));
        if word.len() < 3 {
            return None;
        }
        let name = word.strip_prefix("_LVO").or_else(|| word.strip_prefix('_')).unwrap_or(word);
        if let Some(class) = name.strip_prefix("MUIC_") {
            let book = format!("{class}.mui");
            let b = self.books.iter().position(|b| b.name.eq_ignore_ascii_case(&book))?;
            let pages = &self.books[b].pages;
            return (!pages.is_empty()).then(|| (b, pages.iter().position(|p| p.name.eq_ignore_ascii_case(&book)).unwrap_or(0)));
        }
        let mut tries = vec![name.to_string(), format!("{name}A")];
        if let Some(base) = name.strip_suffix("Tags") {
            tries.extend([format!("{base}TagList"), format!("{base}A"), base.to_string()]);
        }
        if let Some(found) = tries.iter().find_map(|t| self.named(t)) {
            return Some(found);
        }
        // An alias or varargs form the NDK puts in the same slot as a function with a page.
        let ndk = ndk?;
        let function = ndk.functions.get(name)?;
        ndk.functions.values().filter(|f| f.library == function.library && f.offset == function.offset && f.name != function.name).find_map(|f| self.named(&f.name))
    }

    /// The books and pages whose names hold the filter (in any case); a book whose own name holds it, with all its
    /// pages. With no filter, everything.
    pub fn filtered(&self, filter: &str) -> Vec<(usize, Vec<usize>)> {
        let filter = filter.trim().to_lowercase();
        self.books
            .iter()
            .enumerate()
            .filter_map(|(b, book)| {
                if filter.is_empty() || book.name.to_lowercase().contains(&filter) {
                    return Some((b, (0..book.pages.len()).collect()));
                }
                let pages: Vec<usize> = book.pages.iter().enumerate().filter(|(_, p)| p.name.to_lowercase().contains(&filter)).map(|(i, _)| i).collect();
                (!pages.is_empty()).then_some((b, pages))
            })
            .collect()
    }

    /// The page Enter goes to for a filter: one of that very name, else the first whose name starts with it, else
    /// the first shown.
    pub fn best(&self, filter: &str) -> Option<(usize, usize)> {
        let filter = filter.trim();
        if filter.is_empty() {
            return None;
        }
        if let Some(found) = self.named(filter) {
            return Some(found);
        }
        let lower = filter.to_lowercase();
        let shown = self.filtered(filter);
        shown
            .iter()
            .flat_map(|(b, pages)| pages.iter().map(move |p| (*b, *p)))
            .find(|(b, p)| self.books[*b].pages[*p].name.to_lowercase().starts_with(&lower))
            .or_else(|| shown.first().and_then(|(b, pages)| pages.first().map(|p| (*b, *p))))
    }

    /// The words on a page's line that are other pages, to follow: those called, as `FreeMem()`, and under SEE ALSO
    /// every name (`exec.library/FreeMem` too). Each is where it starts and ends, in characters, and its page.
    pub fn links(&self, line: &str, see_also: bool) -> Vec<(usize, usize, (usize, usize))> {
        let chars: Vec<char> = line.chars().collect();
        let part = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-');
        let mut found = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if !part(chars[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < chars.len() && part(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect::<String>().trim_end_matches(['.', '/']).to_string();
            let end = start + word.chars().count();
            let called = chars[end..].iter().collect::<String>().starts_with("()");
            if word.chars().count() < 3 || !(see_also || called) || word.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Some(page) = self.named(&word) {
                found.push((start, end, page));
            }
        }
        found
    }
}

/// The MUI autodocs folder the person chose, once read: `None` inside when there's none.
static MUI: RwLock<Option<Option<PathBuf>>> = RwLock::new(None);
/// The shelf as last read, and the folders it was read from.
type Kept = (Option<PathBuf>, Option<PathBuf>, Arc<Shelf>);
static SHELF: RwLock<Option<Kept>> = RwLock::new(None);

/// The small file that names MUI's autodocs folder.
fn mui_choice_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("mui-autodocs-folder.txt"))
}

/// The MUI autodocs folder chosen, if there is one. None in tests, unless one is set.
pub fn mui_folder() -> Option<PathBuf> {
    if let Some(known) = MUI.read().ok().and_then(|m| m.clone()) {
        return known;
    }
    let folder = if cfg!(test) { None } else { mui_choice_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| PathBuf::from(t.trim())).filter(|p| p.is_dir()) };
    if let Ok(mut kept) = MUI.write() {
        *kept = Some(folder.clone());
    }
    folder
}

/// How many MUI autodoc pages a folder holds.
pub fn mui_pages_in(folder: &Path) -> usize {
    let files: Vec<(Source, PathBuf)> = doc_files(folder).into_iter().map(|f| (Source::Mui, f)).collect();
    Shelf::read(&files).page_count()
}

/// Remembers MUI's autodocs folder (or forgets it), and uses it from now on. Gives the pages found in it.
pub fn choose_mui(folder: Option<&Path>) -> Result<usize, String> {
    let count = match folder {
        Some(folder) => match mui_pages_in(folder) {
            0 => return Err(crate::i18n::tr("There are no autodocs in that folder: no .doc files with pages such as Area.mui/MUIA_Background. Choose the MUI SDK's Autodocs folder.").into()),
            count => count,
        },
        None => 0,
    };
    if !cfg!(test)
        && let Some(file) = mui_choice_file()
    {
        match folder {
            Some(folder) => {
                let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
                std::fs::write(&file, folder.display().to_string()).map_err(|e| crate::i18n::tf!("The choice couldn't be saved: {e}"))?;
            }
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    if let Ok(mut kept) = MUI.write() {
        *kept = Some(folder.map(Path::to_path_buf));
    }
    Ok(count)
}

/// The shelf for the NDK in use and MUI's autodocs, read again only when either has changed.
pub fn shelf() -> Arc<Shelf> {
    let ndk = crate::amiga_ndk::current().map(|index| index.folder.join("Autodocs"));
    let mui = mui_folder();
    if let Some((n, m, shelf)) = SHELF.read().ok().and_then(|s| s.clone())
        && n == ndk
        && m == mui
    {
        return shelf;
    }
    let mut files: Vec<(Source, PathBuf)> = ndk.iter().flat_map(|f| doc_files(f)).map(|f| (Source::Ndk, f)).collect();
    files.extend(mui.iter().flat_map(|f| doc_files(f)).map(|f| (Source::Mui, f)));
    let shelf = Arc::new(Shelf::read(&files));
    if let Ok(mut kept) = SHELF.write() {
        *kept = Some((ndk, mui, shelf.clone()));
    }
    shelf
}

/// The way back and forward through the pages read, as a browser has.
#[derive(Clone, Debug, Default)]
pub struct History {
    pub at: Option<Place>,
    back: Vec<Place>,
    forward: Vec<Place>,
}

impl History {
    /// Goes to a page; going on from one reached by Back forgets the way forward.
    pub fn go(&mut self, place: Place) {
        if self.at.as_ref() == Some(&place) {
            return;
        }
        if let Some(here) = self.at.take() {
            self.back.push(here);
        }
        self.forward.clear();
        self.at = Some(place);
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn back(&mut self) {
        if let Some(place) = self.back.pop() {
            if let Some(here) = self.at.take() {
                self.forward.push(here);
            }
            self.at = Some(place);
        }
    }

    pub fn forward(&mut self) {
        if let Some(place) = self.forward.pop() {
            if let Some(here) = self.at.take() {
                self.back.push(here);
            }
            self.at = Some(place);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXEC: &str = "TABLE OF CONTENTS\n\nexec.library/--background--\nexec.library/AllocMem\nexec.library/FreeMem\n\x0cexec.library/--background--     exec.library/--background--\n\n\tExec is the kernel.\n\x0cexec.library/AllocMem                                   exec.library/AllocMem\n\n   NAME\n\tAllocMem -- allocate memory given certain requirements\n\n   SEE ALSO\n\tFreeMem(), exec/memory.h, exec.library/AvailMem\n\x0cexec.library/FreeMem                     exec.library/FreeMem\n\n   NAME\n\tFreeMem -- deallocate with knowledge\n\n   SEE ALSO\n\tAllocMem()\n";
    const INTUITION: &str = "TABLE OF CONTENTS\n\nintuition.library/OpenWindowTagList\n\x0cintuition.library/OpenWindowTagList          intuition.library/OpenWindowTagList\n\n   NAME\n\tOpenWindowTagList -- open a window\n";
    // MUI's Area class as old CDs have it, without its form feeds.
    const AREA: &str = "TABLE OF CONTENTS\n\nArea.mui/Area.mui\nArea.mui/MUIA_Background\nArea.mui/MUIM_AskMinMax\n\nArea.mui/Area.mui\n\n\tArea class is a super class for every other MUI class.\n\nArea.mui/MUIA_Background\n\n    NAME\n\tMUIA_Background -- (V4 ) [IS.], LONG\n\nArea.mui/MUIM_AskMinMax\n\n    NAME\n\tMUIM_AskMinMax (V4 )\n";
    const MUIMASTER: &str = "TABLE OF CONTENTS\n\nmuimaster.library/MUI_NewObjectA\n\x0cmuimaster.library/MUI_NewObjectA\n\n    NAME\n\tMUI_NewObjectA -- create an object from a class\n";

    fn shelf() -> Shelf {
        let mut shelf = Shelf::default();
        shelf.add(Source::Ndk, EXEC);
        shelf.add(Source::Ndk, INTUITION);
        shelf.add(Source::Mui, AREA);
        shelf.add(Source::Mui, MUIMASTER);
        shelf.index();
        shelf
    }

    fn name_of(shelf: &Shelf, at: Option<(usize, usize)>) -> Option<String> {
        at.and_then(|at| shelf.page(at)).map(|(b, p)| format!("{}/{}", b.name, p.name))
    }

    #[test]
    fn pages_are_read_with_their_backgrounds_and_tabs_set_out() {
        let pages = read_pages(EXEC);
        let names: Vec<&str> = pages.iter().map(|p| p.1.as_str()).collect();
        assert_eq!(names, ["--background--", "AllocMem", "FreeMem"], "the table of contents isn't a page; backgrounds are");
        assert_eq!(pages[1].0, "exec.library");
        assert!(pages[1].2.starts_with("   NAME\n        AllocMem -- allocate"), "tabs every eight columns: {:?}", pages[1].2);
        assert!(!pages[1].2.ends_with('\n'));
    }

    #[test]
    fn pages_without_form_feeds_are_found_from_the_table_of_contents() {
        let pages = read_pages(AREA);
        let names: Vec<(&str, &str)> = pages.iter().map(|p| (p.0.as_str(), p.1.as_str())).collect();
        assert_eq!(names, [("Area.mui", "Area.mui"), ("Area.mui", "MUIA_Background"), ("Area.mui", "MUIM_AskMinMax")]);
        assert!(pages[1].2.contains("MUIA_Background -- (V4 ) [IS.], LONG"));
        assert!(!pages[1].2.contains("MUIM_AskMinMax"), "a page stops where the next starts");
        assert!(read_pages("Just some text\nwith/slashes in it\n").is_empty());
    }

    #[test]
    fn words_from_source_find_their_pages() {
        let shelf = shelf();
        let find = |w: &str| name_of(&shelf, shelf.find(w, None));
        assert_eq!(find("AllocMem").as_deref(), Some("exec.library/AllocMem"));
        assert_eq!(find("_LVOAllocMem").as_deref(), Some("exec.library/AllocMem"), "the offset's name, in 68000 source");
        assert_eq!(find("_AllocMem").as_deref(), Some("exec.library/AllocMem"), "a C stub's name");
        assert_eq!(find("allocmem").as_deref(), Some("exec.library/AllocMem"), "in any case, when there's no other");
        assert_eq!(find("OpenWindowTags").as_deref(), Some("intuition.library/OpenWindowTagList"));
        assert_eq!(find("MUI_NewObject").as_deref(), Some("muimaster.library/MUI_NewObjectA"));
        assert_eq!(find("MUIA_Background").as_deref(), Some("Area.mui/MUIA_Background"));
        assert_eq!(find("MUIM_AskMinMax,").as_deref(), Some("Area.mui/MUIM_AskMinMax"), "punctuation around it is left");
        assert_eq!(find("MUIC_Area").as_deref(), Some("Area.mui/Area.mui"), "a class, at its own page");
        assert_eq!(find("Nothing"), None);
        assert_eq!(find("a0"), None);
    }

    #[test]
    fn a_name_sharing_a_slot_finds_the_page_of_the_one_explained() {
        let shelf = shelf();
        let mut ndk = crate::amiga_ndk::Index::default();
        let sfd = "==libname intuition.library\n==bias 606\n==public\nstruct Window *OpenWindowTagList(CONST struct NewWindow *newWindow, CONST struct TagItem *tagList) (a0,a1)\n==varargs\nstruct Window *OpenWindowTagItems(CONST struct NewWindow *newWindow, Tag tag1Type, ...) (a0,a1)\n==end\n";
        for f in crate::amiga_ndk::read_sfd(sfd) {
            ndk.functions.insert(f.name.clone(), f);
        }
        assert_eq!(shelf.find("OpenWindowTagItems", None), None);
        assert_eq!(name_of(&shelf, shelf.find("OpenWindowTagItems", Some(&ndk))).as_deref(), Some("intuition.library/OpenWindowTagList"));
    }

    #[test]
    fn the_filter_keeps_matching_pages_and_whole_books() {
        let shelf = shelf();
        let shown = |f: &str| -> Vec<String> { shelf.filtered(f).iter().flat_map(|(b, pages)| pages.iter().map(|p| format!("{}/{}", shelf.books[*b].name, shelf.books[*b].pages[*p].name)).collect::<Vec<_>>()).collect() };
        assert_eq!(shown("mem"), ["exec.library/AllocMem", "exec.library/FreeMem"]);
        assert_eq!(shown("AREA.MUI").len(), 3, "a book's name gives all its pages");
        assert_eq!(shown("").len(), shelf.page_count());
        assert!(shown("zzz").is_empty());
        assert_eq!(name_of(&shelf, shelf.best("freemem")).as_deref(), Some("exec.library/FreeMem"), "the very name");
        assert_eq!(name_of(&shelf, shelf.best("Fre")).as_deref(), Some("exec.library/FreeMem"), "the first it starts");
        assert_eq!(name_of(&shelf, shelf.best("Background")).as_deref(), Some("exec.library/--background--"), "else the first shown");
        assert_eq!(shelf.best(" "), None);
    }

    #[test]
    fn the_ndk_s_books_come_before_mui_s_and_pages_link_to_others() {
        let shelf = shelf();
        let books: Vec<&str> = shelf.books.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(books, ["exec.library", "intuition.library", "Area.mui", "muimaster.library"], "as added; read() sorts them");
        let line = "        FreeMem(), exec/memory.h, exec.library/AvailMem";
        let links: Vec<(String, String)> = shelf.links(line, true).iter().map(|(s, e, at)| (line.chars().skip(*s).take(e - s).collect(), name_of(&shelf, Some(*at)).unwrap())).collect();
        assert_eq!(links, [("FreeMem".to_string(), "exec.library/FreeMem".to_string())], "a header and a page that isn't there aren't links");
        assert_eq!(shelf.links("   the memory from AllocMem() must be freed", false).len(), 1, "a call is a link anywhere");
        assert!(shelf.links("   AllocMem is fussy", false).is_empty(), "a bare name is a link only under SEE ALSO");
    }

    #[test]
    fn history_goes_back_and_forward_as_a_browser_does() {
        let place = |p: &str| Place { book: "exec.library".into(), page: p.into() };
        let mut history = History::default();
        assert!(!history.can_go_back());
        history.go(place("AllocMem"));
        history.go(place("FreeMem"));
        history.go(place("FreeMem"));
        history.back();
        assert_eq!(history.at, Some(place("AllocMem")), "the same page twice is one step");
        assert!(history.can_go_forward());
        history.forward();
        assert_eq!(history.at, Some(place("FreeMem")));
        history.back();
        history.go(place("AvailMem"));
        assert!(!history.can_go_forward(), "a new page forgets the way forward");
        history.back();
        assert_eq!(history.at, Some(place("AllocMem")));
    }

    #[test]
    fn a_folder_s_autodocs_are_read_and_mui_s_chosen() {
        let dir = std::env::temp_dir().join(format!("limoniter-autodocs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Developer").join("Autodocs")).unwrap();
        std::fs::write(dir.join("Developer").join("Autodocs").join("MUI_Area.doc"), AREA).unwrap();
        std::fs::write(dir.join("Developer").join("ReadMe"), "not an autodoc").unwrap();
        assert_eq!(doc_files(&dir).len(), 1, "found a few folders down");
        assert_eq!(mui_pages_in(&dir), 3);
        assert!(choose_mui(Some(&std::env::temp_dir().join("limoniter-no-such-folder"))).is_err());
        assert_eq!(choose_mui(Some(&dir)), Ok(3));
        assert_eq!(mui_folder(), Some(dir.clone()));
        let shelf = super::shelf();
        assert_eq!(shelf.books.len(), 1);
        assert_eq!(shelf.books[0].source, Source::Mui);
        assert!(Arc::ptr_eq(&shelf, &super::shelf()), "read once while nothing changes");
        assert_eq!(choose_mui(None), Ok(0));
        assert!(super::shelf().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Reads a real NDK's autodocs: LIMONITER_NDK=<folder> cargo test -- --ignored
    #[test]
    #[ignore]
    fn a_real_ndk_s_autodocs_are_all_read() {
        let Ok(folder) = std::env::var("LIMONITER_NDK") else { return };
        let files: Vec<(Source, PathBuf)> = doc_files(&Path::new(&folder).join("Autodocs")).into_iter().map(|f| (Source::Ndk, f)).collect();
        let started = std::time::Instant::now();
        let shelf = Shelf::read(&files);
        println!("{} books, {} pages, read in {:?}", shelf.books.len(), shelf.page_count(), started.elapsed());
        assert!(shelf.page_count() > 1500, "NDK 3.2 has some 1,560 pages");
        for word in ["AllocMem", "_LVOOpenLibrary", "OpenWindowTags", "CreateNewProcTags"] {
            assert!(shelf.find(word, None).is_some(), "{word}");
        }
        let (book, page) = shelf.page(shelf.find("AllocMem", None).unwrap()).unwrap();
        assert_eq!(book.name, "exec.library");
        assert!(page.text.contains("SEE ALSO"));
    }
}
