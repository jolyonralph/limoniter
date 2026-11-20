//! The window's side of the diagram features: saving the preview as a PDF, the call graph panel and the
//! instruction reference panel.

use super::*;

impl Limoniter {
    /// The preview a document has that's laid out as a document, which a PDF can be made of.
    pub(super) fn pdf_preview_kind(doc: &Document) -> Option<preview::Kind> {
        Self::preview_kind(doc).filter(|kind| matches!(kind, preview::Kind::Markdown | preview::Kind::Html | preview::Kind::Document | preview::Kind::Basic))
    }

    /// The preview's blocks, made afresh from the document as it is now.
    fn blocks_for_pdf(&self) -> Result<Vec<preview::Block>, String> {
        let doc = &self.docs[self.active];
        let kind = Self::pdf_preview_kind(doc).ok_or_else(|| crate::i18n::tr("Only a preview laid out as a document can be saved as a PDF: Markdown, a web page, a converted document or a BASIC listing.").to_string())?;
        if !kind.reads_bytes() {
            if doc.hex_mode {
                return Err(crate::i18n::tr("Switch to text mode to preview this file.").to_string());
            }
            if doc.text.is_disk_backed() {
                return Err(crate::i18n::tr("This file is too large to preview. It is being kept on disk.").to_string());
            }
            let lines = doc.text.all_lines();
            let readable = lines.iter().map(|line| doc.text.format.readable_text(line)).collect::<Vec<_>>().join("\n");
            return Ok(preview::render(kind, &readable));
        }
        let bytes = self.document_bytes();
        match preview::render_bytes(kind, &bytes, &doc.text.format, Some(&doc.path_or_name())) {
            preview::FromBytes::Blocks(blocks) => Ok(blocks),
            preview::FromBytes::Nothing(why) => Err(why),
            _ => Err(crate::i18n::tr("Only a preview laid out as a document can be saved as a PDF: Markdown, a web page, a converted document or a BASIC listing.").to_string()),
        }
    }

    /// Saves the preview as a PDF, beside the file unless another place is chosen.
    pub(super) fn save_preview_as_pdf(&mut self) {
        let blocks = match self.blocks_for_pdf() {
            Ok(blocks) => blocks,
            Err(why) => return self.message("Save as PDF", why),
        };
        let doc = &self.docs[self.active];
        let folder = doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let name = doc.path_or_name().file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "preview".into());
        let title = doc.path_or_name().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut picker = rfd::FileDialog::new().set_title(crate::i18n::tr("Save the preview as PDF")).set_file_name(format!("{name}.pdf")).add_filter(crate::i18n::tr("PDF document"), &["pdf"]);
        if let Some(folder) = &folder {
            picker = picker.set_directory(folder);
        }
        let Some(target) = picker.save_file() else { return };
        let bytes = crate::preview_pdf::make(&blocks, folder.as_deref(), &title);
        if let Err(e) = codec::write_file(&target, &bytes) {
            self.message("Save as PDF", crate::i18n::tf!("Couldn't save {}.\n\n{e}", target.display()));
        }
    }

    /// For pictures of the panels: the call graph or instruction reference open, the program assembled now; or the
    /// preview written straight to a PDF at a path, with no box to choose one.
    pub(super) fn diagram_screenshot_step(&mut self, step: &str, value: &str) {
        match step {
            "call-graph" => self.panels.call_graph = true,
            "instructions" => self.panels.instructions = true,
            _ => {
                if let Ok(blocks) = self.blocks_for_pdf() {
                    let folder = self.docs[self.active].path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
                    let _ = codec::write_file(Path::new(value), &crate::preview_pdf::make(&blocks, folder.as_deref(), "preview"));
                }
                return;
            }
        }
        highlight::load_now();
        self.doc().update_syntax();
        self.refresh_intel();
    }

    /// The calls into and out of the function at the cursor, as its language server sees them: those it makes, and
    /// those they make in turn, and those that call it. Drawn in the call graph panel.
    pub(super) fn lsp_call_hierarchy(&mut self) {
        const WHAT: &str = "Call graph";
        let Some((result, _)) = self.lsp_ask_here("textDocument/prepareCallHierarchy", WHAT) else { return };
        let Some(root) = result.as_array().and_then(|items| items.first()).cloned() else {
            return self.message(WHAT, crate::i18n::tr("The language server found no function at the cursor, or can't draw calls. Put the cursor on a function's name.").into());
        };
        let Some((server, _)) = self.lsp_server_for(self.active) else { return };
        let mut gathered = crate::call_graph::ServerGraph::default();
        let Some(start) = gathered.item(&root) else { return };
        let mut ask = |method: &str, item: &serde_json::Value| -> Vec<serde_json::Value> {
            self.lsp_request_wait(server, method, serde_json::json!({ "item": item })).and_then(|r| r.as_array().cloned()).unwrap_or_default()
        };
        // Who calls it.
        for call in ask("callHierarchy/incomingCalls", &root) {
            let places = call.get("fromRanges").and_then(serde_json::Value::as_array).map_or(1, Vec::len);
            if let Some(from) = call.get("from").and_then(|item| gathered.item(item)) {
                gathered.call(from, start, places);
            }
        }
        // What it calls, and what those call, two deep, and no more than so many functions.
        let mut waiting = vec![(root, start, 0)];
        let mut asked = std::collections::HashSet::new();
        while let Some((item, at, depth)) = waiting.pop() {
            if depth >= 2 || !asked.insert(at) || gathered.len() > 80 {
                continue;
            }
            for call in ask("callHierarchy/outgoingCalls", &item) {
                let places = call.get("fromRanges").and_then(serde_json::Value::as_array).map_or(1, Vec::len);
                let Some(to_item) = call.get("to") else { continue };
                if let Some(to) = gathered.item(to_item) {
                    gathered.call(at, to, places);
                    waiting.insert(0, (to_item.clone(), to, depth + 1));
                }
            }
        }
        let (graph, files) = gathered.finish();
        let id = self.docs[self.active].id;
        self.panels.show_server_graph(id, graph, files);
    }
}
