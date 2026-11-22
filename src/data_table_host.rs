//! Showing a `DataTable` in a tab. THE STAND-IN: until the data viewer is joined on, a table opens as
//! tab-separated text in table view. The data viewer replaces this file's body with its own window.

// Used once the data viewer and database browser join (0.16 batch 2).
#![allow(dead_code)]

use super::*;
use crate::data_table::DataTable;

impl Limoniter {
    /// Opens a table of data in a tab of its own, named `title`.
    pub(super) fn open_data_table(&mut self, title: &str, table: DataTable) {
        let _ = title;
        self.new_document();
        let text = table.to_tsv();
        let doc = self.doc();
        doc.text.type_text(&text);
        doc.table = Some(Box::new(table_view::TableState::new(crate::table::Dialect { delimiter: '\t', header: true })));
    }
}
