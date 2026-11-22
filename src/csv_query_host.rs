//! The window's side of querying the rows of a delimited file: the box to type the query in, and the result
//! opening in the data viewer. The query itself is in `csv_query`.

use super::*;
use crate::csv_query;

/// The box asking for a query.
pub(super) struct QueryBox {
    pub query: String,
    pub delimiter: char,
    /// The first row names the columns.
    pub header: bool,
    /// The columns' names as the file is first read, to remind the person what they can write.
    pub names: Vec<String>,
    /// The first row's fields, to name the columns again if the headings box is changed.
    pub first: Vec<String>,
    pub error: Option<String>,
}

impl Limoniter {
    /// Opens the box for the file in front, if it holds delimited data.
    pub(super) fn open_query_rows(&mut self) {
        let doc = &self.docs[self.active];
        if doc.placeholder || doc.hex_mode || doc.data.is_some() {
            return;
        }
        let ending = doc.path_or_name().extension().map(|e| e.to_string_lossy().to_lowercase());
        let sample: Vec<Arc<str>> = (0..doc.text.line_count().min(200)).map(|i| doc.text.line(i)).collect();
        let dialect = doc.table.as_deref().map(|t| t.dialect).or_else(|| crate::table::sniff(&sample, ending.as_deref())).unwrap_or(crate::table::Dialect { delimiter: if ending.as_deref() == Some("tsv") { '\t' } else { ',' }, header: true });
        let first: Vec<String> = crate::table::records(sample.as_slice(), dialect.delimiter).first().map(|r| crate::table::fields(&crate::table::record_text(sample.as_slice(), *r), dialect.delimiter).into_iter().map(|f| f.value).collect()).unwrap_or_default();
        let names = csv_query::column_names(if dialect.header { Some(&first) } else { None }, first.len());
        self.dialog = Dialog::QueryRows(Box::new(QueryBox { query: "SELECT *".to_string(), delimiter: dialect.delimiter, header: dialect.header, names, first, error: None }));
    }

    /// The box itself. True while it's showing.
    pub(super) fn query_rows_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::QueryRows(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut run = false;
        crate::fit::Dialog::new(crate::i18n::tr("Query the rows")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.set_max_width(520.0);
                ui.label(crate::i18n::tr("Ask for some rows and columns of the file in SQL's words, and the answer opens as a table. For example:"));
                ui.label(egui::RichText::new("SELECT region, COUNT(*), SUM(sales)\nWHERE sales > 10\nGROUP BY region\nORDER BY 3 DESC LIMIT 20").monospace().color(theme::text_dim()));
                ui.label(crate::i18n::tr("Columns are named by their headings (in backticks if they have spaces), or a1, a2 and so on."));
                let field = ui.add(egui::TextEdit::multiline(&mut state.query).font(egui::TextStyle::Monospace).desired_rows(4).desired_width(f32::INFINITY));
                field.request_focus();
                if field.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(Key::Enter)) {
                    run = true;
                }
                ui.checkbox(&mut state.header, crate::i18n::tr("First row is headings"));
                let shown: Vec<String> = state.names.iter().take(12).cloned().collect();
                let more = if state.names.len() > 12 { ", \u{2026}" } else { "" };
                ui.label(egui::RichText::new(crate::i18n::tf!("Columns: {list}{more}", list = shown.join(", "), more = more)).color(theme::text_dim()));
                if let Some(message) = &state.error {
                    ui.colored_label(theme::change_unsaved(), message);
                }
            }
            crate::fit::Part::Buttons => {
                ui.horizontal(|ui| {
                    run |= ui.button(crate::i18n::tr("Run the query")).on_hover_text(crate::i18n::tr("Ctrl+Enter")).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
        });
        if run {
            self.run_query_rows();
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// Starts the query over the file in front, in the background, so the window keeps working; the answer opens
    /// in the data viewer when it's done (see `finish_query`), or the box comes back saying what's wrong.
    fn run_query_rows(&mut self) {
        let Dialog::QueryRows(state) = &mut self.dialog else {
            return;
        };
        let (query, delimiter, header) = (state.query.clone(), state.delimiter, state.header);
        // A mistake in the query itself is said at once, before the whole file is read.
        let names = csv_query::column_names(if header { Some(&state.first) } else { None }, state.first.len());
        if let Err(error) = csv_query::parse(&query, &names) {
            state.error = Some(error.describe(&query));
            return;
        }
        if self.big_job.is_some() {
            return self.message("Query the rows", crate::i18n::tr("Please wait for the file being worked through to finish.").into());
        }
        let doc = &self.docs[self.active];
        self.big_job = Some(crate::big_jobs::query(doc.text.store().clone(), query, delimiter, header, doc.id));
        self.dialog = Dialog::None;
    }

    /// A query that's finished: its answer in the data viewer, or the box again with the mistake.
    pub(super) fn finish_query(&mut self, doc_id: u64, table: Result<Box<crate::data_table::DataTable>, csv_query::QueryError>, query: String, delimiter: char, header: bool) {
        let Some(index) = self.docs.iter().position(|d| d.id == doc_id) else {
            return;
        };
        self.active = index;
        match table {
            Ok(mut table) => {
                table.note = table.rows.is_empty().then(|| crate::i18n::tr("No rows matched the query.").to_string());
                let title = self.docs[index].name();
                self.open_data_table(&crate::i18n::tf!("Query on {name}", name = title), *table);
            }
            Err(error) => {
                self.open_query_rows();
                if let Dialog::QueryRows(state) = &mut self.dialog {
                    state.error = Some(error.describe(&query));
                    (state.query, state.delimiter, state.header) = (query, delimiter, header);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_table::Cell;

    fn app() -> (Limoniter, Context) {
        (Limoniter::build(String::new(), None, Some(PathBuf::from("never-written.png"))), Context::default())
    }

    fn give(app: &mut Limoniter, ctx: &Context, command: Command) {
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run(command, ui));
    }

    fn frame(app: &mut Limoniter, ctx: &Context) {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        let _ = ctx.run_ui(input, |ui| app.draw_frame(ui));
    }

    /// Draws frames until the background query has finished, giving how many it took.
    fn finish_counting(app: &mut Limoniter) -> usize {
        let ctx = Context::default();
        let mut frames = 0;
        while app.big_job.is_some() {
            frame(app, &ctx);
            frames += 1;
            assert!(frames < 20_000, "the query never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        frames
    }

    fn finish(app: &mut Limoniter) {
        finish_counting(app);
    }

    fn with_sales(app: &mut Limoniter) {
        app.new_document();
        app.doc().text.type_text("region,sales\nnorth,10\nsouth,25\nnorth,7\n");
        app.doc().untitled = "sales.csv".into();
    }

    #[test]
    fn the_query_box_opens_for_delimited_text_only() {
        let (mut app, ctx) = app();
        app.new_document();
        app.doc().text.type_text("just some words\n");
        app.doc().untitled = "notes.txt".into();
        assert!(!app.can(Command::QueryRows));
        with_sales(&mut app);
        assert!(app.can(Command::QueryRows));
        give(&mut app, &ctx, Command::QueryRows);
        let Dialog::QueryRows(state) = &app.dialog else { panic!("the box should be open") };
        assert_eq!((state.delimiter, state.header), (',', true));
        assert_eq!(state.names, ["region", "sales"]);
    }

    #[test]
    fn a_query_opens_its_answer_in_the_data_viewer() {
        let (mut app, ctx) = app();
        with_sales(&mut app);
        give(&mut app, &ctx, Command::QueryRows);
        if let Dialog::QueryRows(state) = &mut app.dialog {
            state.query = "SELECT region, SUM(sales) GROUP BY region ORDER BY 2 DESC".into();
        }
        app.run_query_rows();
        assert!(matches!(app.dialog, Dialog::None));
        finish(&mut app);
        let sheet = &app.docs[app.active].data.as_deref().expect("in the data viewer").sheets[0];
        assert_eq!(sheet.table.rows[0], vec![Cell::Text("south".into()), Cell::Int(25)]);
        assert_eq!(sheet.table.rows[1], vec![Cell::Text("north".into()), Cell::Int(17)]);
    }

    #[test]
    fn a_mistaken_query_leaves_the_box_open_saying_where() {
        let (mut app, ctx) = app();
        with_sales(&mut app);
        give(&mut app, &ctx, Command::QueryRows);
        if let Dialog::QueryRows(state) = &mut app.dialog {
            state.query = "SELECT region WHERE amount > 3".into();
        }
        app.run_query_rows();
        assert!(app.big_job.is_none(), "a mistake in the query is said before the file is read");
        finish(&mut app);
        let Dialog::QueryRows(state) = &app.dialog else { panic!("the box should stay") };
        let message = state.error.clone().expect("an error");
        assert!(message.contains("amount") && message.contains("character 21"), "{message}");
        assert!(app.docs[app.active].data.is_none());
    }

    #[test]
    fn without_a_header_row_the_columns_are_a1_a2() {
        let (mut app, ctx) = app();
        with_sales(&mut app);
        give(&mut app, &ctx, Command::QueryRows);
        if let Dialog::QueryRows(state) = &mut app.dialog {
            state.header = false;
            state.query = "SELECT a1 WHERE a2 = 'sales'".into();
        }
        app.run_query_rows();
        finish(&mut app);
        assert_eq!(app.docs[app.active].data.as_deref().unwrap().sheets[0].table.rows[0], vec![Cell::Text("region".into())]);
    }

    #[test]
    fn opening_a_csv_notes_its_delimiter_for_the_text_view() {
        let (mut app, _ctx) = app();
        with_sales(&mut app);
        app.docs[app.active].table_checked = false;
        app.check_for_table();
        assert_eq!(app.docs[app.active].text.delimited, Some(','));
    }

    #[test]
    fn the_window_keeps_drawing_while_a_big_query_runs() {
        let (mut app, ctx) = app();
        app.new_document();
        let lines: Vec<String> = std::iter::once("id,group,score".to_string()).chain((0..400_000u64).map(|i| format!("{i},g{},{}", i % 1000, (i * 7919) % 10007))).collect();
        app.doc().text.set_content(lines);
        app.doc().untitled = "big.csv".into();
        give(&mut app, &ctx, Command::QueryRows);
        if let Dialog::QueryRows(state) = &mut app.dialog {
            state.query = "SELECT `group`, COUNT(*), SUM(score), AVG(score) GROUP BY `group` ORDER BY 3 DESC".into();
        }
        app.run_query_rows();
        assert!(app.big_job.is_some(), "it runs in the background and comes straight back");
        let share = app.big_job.as_ref().map(|j| j.share());
        assert!(share.is_some());
        let frames = finish_counting(&mut app);
        assert!(frames > 3, "the window drew {frames} frames while it ran");
        let sheet = &app.docs[app.active].data.as_deref().expect("the answer opened").sheets[0];
        assert_eq!(sheet.table.rows.len(), 1000);
    }

    #[test]
    fn a_query_can_be_stopped() {
        let (mut app, ctx) = app();
        app.new_document();
        let lines: Vec<String> = (0..300_000u64).map(|i| format!("{i},{}", i % 13)).collect();
        app.doc().text.set_content(lines);
        app.doc().untitled = "big.csv".into();
        give(&mut app, &ctx, Command::QueryRows);
        if let Dialog::QueryRows(state) = &mut app.dialog {
            state.header = false;
            state.query = "SELECT a1 ORDER BY a2".into();
        }
        app.run_query_rows();
        app.big_job.as_ref().expect("running").cancel();
        finish(&mut app);
        assert!(app.docs.iter().all(|d| d.data.is_none()), "no answer opens");
    }
}
