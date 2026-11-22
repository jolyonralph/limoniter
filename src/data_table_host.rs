//! Showing a `DataTable` in a tab: the data viewer, in `data_view_host`.

use super::*;
use crate::data_table::DataTable;

impl Limoniter {
    /// Opens a table of data in a tab of its own, named `title`.
    pub(super) fn open_data_table(&mut self, title: &str, table: DataTable) {
        self.open_data_sheets(title, vec![(title.to_string(), table)], None);
    }
}
