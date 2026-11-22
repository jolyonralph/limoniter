//! The AutoDoc reader (Tools, Amiga, AutoDoc reader, or Jump to its explanation on a function's name): the NDK's
//! autodocs, and MUI's when the person has pointed to them, in a window that stays open beside the source, as
//! MinAD shows them. On the left, every library's pages, filtered as you type; on the right, the page in a
//! fixed-width font, its calls and SEE ALSO names followed with a click, and Back and Next.

use super::*;
use crate::autodocs::{History, Shelf};

const TITLE: &str = "AutoDoc reader";

/// The reader's window.
pub struct Reader {
    shelf: Arc<Shelf>,
    pub filter: String,
    history: History,
    /// The page just gone to, to open its book in the list and scroll to it: for a few frames, as the book takes
    /// a frame or two to open.
    reveal: u8,
    /// What came of choosing MUI's autodocs.
    said: String,
}

impl Reader {
    fn new() -> Self {
        Reader { shelf: crate::autodocs::shelf(), filter: String::new(), history: History::default(), reveal: 0, said: String::new() }
    }

    fn go(&mut self, at: (usize, usize)) {
        if let Some(place) = self.shelf.place(at) {
            self.history.go(place);
            self.reveal = 3;
        }
    }

    /// Goes to the page explaining a word, if there is one. Whether there was.
    pub fn go_to_word(&mut self, word: &str) -> bool {
        let ndk = crate::amiga_ndk::current();
        match self.shelf.find(word, ndk.as_deref()) {
            Some(at) => {
                self.go(at);
                true
            }
            None => false,
        }
    }

    fn current(&self) -> Option<(usize, usize)> {
        self.history.at.as_ref().and_then(|place| self.shelf.locate(place))
    }

    /// Reads the shelf again (after MUI's folder or the NDK has changed), keeping the page if it's still there.
    fn reread(&mut self) {
        self.shelf = crate::autodocs::shelf();
        if self.current().is_none() {
            self.history = History::default();
        }
    }

    /// Draws the window's inside. Gives what was asked for that the window can't do itself.
    fn draw(&mut self, ui: &mut Ui) -> Asked {
        let mut asked = Asked::Nothing;
        ui.horizontal(|ui| {
            if ui.add_enabled(self.history.can_go_back(), Button::new(crate::i18n::tr("\u{2190} Back"))).clicked() {
                self.history.back();
                self.reveal = 3;
            }
            if ui.add_enabled(self.history.can_go_forward(), Button::new(crate::i18n::tr("Next \u{2192}"))).clicked() {
                self.history.forward();
                self.reveal = 3;
            }
            ui.separator();
            ui.label(crate::i18n::tr("Filter:"));
            let typed = ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(200.0).hint_text(crate::i18n::tr("AllocMem, MUIA_, exec")));
            if typed.lost_focus()
                && ui.input(|i| i.key_pressed(Key::Enter))
                && let Some(at) = self.shelf.best(&self.filter)
            {
                self.go(at);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::autodocs::mui_folder().is_some() {
                    if ui.button(crate::i18n::tr("Forget MUI's")).on_hover_text(crate::i18n::tr("Stops reading MUI's autodocs (the folder is left alone)")).clicked() {
                        asked = Asked::ForgetMui;
                    }
                } else if ui.button(crate::i18n::tr("MUI's autodocs\u{2026}")).on_hover_text(crate::i18n::tr("The Autodocs folder of the MUI SDK, from muidev or an MUI release: MUI's developer files aren't Limoniter's to include, so they're read from where you keep them")).clicked() {
                    asked = Asked::ChooseMui;
                }
                if ui.button(crate::i18n::tr("NDK\u{2026}")).on_hover_text(crate::i18n::tr("The Amiga NDK, whose autodocs these are")).clicked() {
                    asked = Asked::Ndk;
                }
            });
        });
        if !self.said.is_empty() {
            ui.colored_label(theme::text_dim(), &self.said);
        }
        ui.separator();
        if self.shelf.is_empty() {
            ui.add_space(8.0);
            ui.label(crate::i18n::tr("There are no autodocs to read yet. The Amiga's come with Hyperion's NDK: download it, or use one you have, with the NDK button. MUI's come with the MUI SDK: point to its Autodocs folder with MUI's autodocs."));
            return asked;
        }
        let current = self.current();
        let mut go_to = None;
        egui::Panel::left("autodocs-tree").resizable(true).default_size(240.0).size_range(160.0..=320.0).show(ui, |ui| {
            let shown = self.shelf.filtered(&self.filter);
            let filtering = !self.filter.trim().is_empty();
            let pages: usize = shown.iter().map(|(_, p)| p.len()).sum();
            ui.label(RichText::new(crate::i18n::tf!("{} pages in {} books", pages, shown.len())).color(theme::text_dim()).size(12.0));
            egui::ScrollArea::vertical().auto_shrink([true, false]).show(ui, |ui| {
                // A long list of every page is drawn only for the books opened, or a few hundred when filtering.
                let mut left = 600usize;
                for (b, pages) in &shown {
                    let book = &self.shelf.books[*b];
                    let holds_current = current.is_some_and(|(cb, _)| cb == *b);
                    let open = ((filtering && left > 0) || (self.reveal > 0 && holds_current)).then_some(true);
                    let mut header = egui::CollapsingHeader::new(RichText::new(&book.name).monospace()).id_salt(("autodoc-book", *b));
                    if let Some(open) = open {
                        header = header.open(Some(open));
                    }
                    let response = header.show(ui, |ui| {
                        for p in pages {
                            if left == 0 {
                                break;
                            }
                            left -= 1;
                            let here = current == Some((*b, *p));
                            let label = ui.selectable_label(here, RichText::new(&book.pages[*p].name).monospace());
                            if here && self.reveal > 0 {
                                label.scroll_to_me(Some(egui::Align::Center));
                            }
                            if label.clicked() {
                                go_to = Some((*b, *p));
                            }
                        }
                    });
                    if book.source == crate::autodocs::Source::Mui {
                        response.header_response.on_hover_text(crate::i18n::tr("From MUI's autodocs"));
                    }
                }
                if left == 0 {
                    ui.label(RichText::new(crate::i18n::tr("\u{2026} and more: type more of the name to see them.")).color(theme::text_dim()).size(12.0));
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| match current.and_then(|at| self.shelf.page(at)) {
            None => {
                ui.label(crate::i18n::tf!("{} pages. Choose one on the left, or type a name and press Enter.", self.shelf.page_count()));
            }
            Some((book, page)) => {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new(format!("{}/{}", book.name, page.name)).monospace());
                    ui.label(RichText::new(book.source.name()).color(theme::text_dim()));
                });
                ui.separator();
                let scroll = egui::ScrollArea::both().id_salt(("autodoc-page", &book.name, &page.name)).auto_shrink([false, false]);
                scroll.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    let mut see_also = false;
                    for line in page.text.lines() {
                        // SEE ALSO runs until the next heading, which is written in capitals at the left.
                        let trimmed = line.trim();
                        if !line.starts_with("        ") && !trimmed.is_empty() && trimmed.chars().all(|c| !c.is_lowercase()) {
                            see_also = trimmed == "SEE ALSO";
                        }
                        let links = self.shelf.links(line, see_also);
                        if links.is_empty() {
                            // Lines are kept as they were written, not wrapped: the window scrolls sideways instead.
                            ui.add(egui::Label::new(RichText::new(if line.is_empty() { " " } else { line }).monospace()).extend());
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let chars: Vec<char> = line.chars().collect();
                            let mut at = 0;
                            for (start, end, page) in links {
                                if start > at {
                                    ui.add(egui::Label::new(RichText::new(chars[at..start].iter().collect::<String>()).monospace()).extend());
                                }
                                let word: String = chars[start..end].iter().collect();
                                if ui.link(RichText::new(word).monospace()).clicked() {
                                    go_to = Some(page);
                                }
                                at = end;
                            }
                            if at < chars.len() {
                                ui.add(egui::Label::new(RichText::new(chars[at..].iter().collect::<String>()).monospace()).extend());
                            }
                        });
                    }
                });
            }
        });
        self.reveal = self.reveal.saturating_sub(1);
        if let Some(at) = go_to {
            self.go(at);
        }
        asked
    }
}

/// What the reader asks the window to do.
enum Asked {
    Nothing,
    Ndk,
    ChooseMui,
    ForgetMui,
}

impl Limoniter {
    /// Opens the reader (or brings it forward) at the page explaining a word, if there is one; a word with no page
    /// becomes the filter, to show what's near it.
    pub(super) fn open_autodocs(&mut self, word: Option<String>) {
        let reader = self.autodoc_reader.get_or_insert_with(|| Box::new(Reader::new()));
        if let Some(word) = word.filter(|w| !w.trim().is_empty())
            && !reader.go_to_word(&word)
        {
            reader.filter = word;
        }
    }

    /// Opens the reader at the page for the word at the caret, if the source in front has one there.
    pub(super) fn open_autodocs_at_caret(&mut self) {
        let word = (!self.no_file())
            .then(|| &self.docs[self.active])
            .filter(|doc| !doc.hex_mode)
            .and_then(|doc| crate::code_intel::word_at(&doc.text.line(doc.text.caret_line), doc.text.caret_col))
            .map(|(_, _, word)| word)
            .filter(|word| crate::autodocs::shelf().find(word, crate::amiga_ndk::current().as_deref()).is_some());
        self.open_autodocs(word);
    }

    pub(super) fn autodocs_window(&mut self, ctx: &Context) {
        let Some(mut reader) = self.autodoc_reader.take() else {
            return;
        };
        let mut open = true;
        let mut asked = Asked::Nothing;
        crate::fit::Dialog::new(crate::i18n::tr(TITLE)).open(&mut open).default_pos([180.0, 70.0]).default_size([900.0, 560.0]).resizable(true).show(ctx, |ui| {
            asked = reader.draw(ui);
        });
        match asked {
            Asked::Nothing => {}
            Asked::Ndk => self.open_ndk_box(),
            Asked::ChooseMui => {
                if let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("MUI's Autodocs folder")).pick_folder() {
                    reader.said = match crate::autodocs::choose_mui(Some(&folder)) {
                        Ok(pages) => crate::i18n::tf!("{pages} pages of MUI's autodocs read from {}.", folder.display()),
                        Err(problem) => problem,
                    };
                    reader.reread();
                }
            }
            Asked::ForgetMui => {
                let _ = crate::autodocs::choose_mui(None);
                reader.said.clear();
                reader.reread();
            }
        }
        // The NDK may have been downloaded or changed while the reader was open.
        if !Arc::ptr_eq(&reader.shelf, &crate::autodocs::shelf()) {
            reader.reread();
        }
        if open {
            self.autodoc_reader = Some(reader);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autodocs::Place;

    #[test]
    fn the_reader_opens_at_a_word_s_page_or_filters_for_it() {
        let mut reader = Reader { shelf: Arc::new(Shelf::default()), filter: String::new(), history: History::default(), reveal: 0, said: String::new() };
        let mut shelf = Shelf::default();
        shelf.add(crate::autodocs::Source::Ndk, "\x0cexec.library/AllocMem\n\n   NAME\n\tAllocMem -- allocate memory\n\x0cexec.library/FreeMem\n\n   NAME\n\tFreeMem\n");
        shelf.index();
        reader.shelf = Arc::new(shelf);
        assert!(reader.go_to_word("_LVOAllocMem"));
        assert_eq!(reader.history.at, Some(Place { book: "exec.library".into(), page: "AllocMem".into() }));
        assert!(reader.reveal > 0, "its book is opened in the list");
        assert!(!reader.go_to_word("Nothing"));
        assert_eq!(reader.current(), Some((0, 0)));
    }
}
