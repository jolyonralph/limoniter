//! What screen readers are told about the parts of the window Limoniter draws itself. egui gives its own buttons,
//! labels and fields a role and a name through AccessKit; these give the same to clickable labels, painted
//! controls and the editors, so each can be found, named and used without seeing it.

use eframe::egui::{Response, Role, WidgetInfo};

/// A painted control, or a label that's clicked, read out as a button with this name.
pub fn button(response: &Response, name: &str) {
    named(response, Role::Button, name);
}

/// A painted control read out with this role and name.
pub fn named(response: &Response, role: Role, name: &str) {
    response.widget_info(|| WidgetInfo::labeled(role, response.enabled(), name));
    // A label's words are its value; once it's a button they're its name alone.
    response.ctx.accesskit_node_builder(response.id, |node| node.clear_value());
}

/// Something that's on or off, such as an option in a list or a tab, read out with its state.
pub fn chosen(response: &Response, role: Role, on: bool, name: &str) {
    response.widget_info(|| WidgetInfo::selected(role, response.enabled(), on, name));
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.clear_value();
        // Tabs and options in a list are selected; check boxes keep egui's ticked or not.
        if !matches!(role, Role::CheckBox | Role::Switch | Role::RadioButton) {
            node.clear_toggled();
            node.set_selected(on);
        }
    });
}

/// Gives a control the words a screen reader says after its name, such as what its tooltip says.
pub fn describe(response: &Response, words: &str) {
    if !words.is_empty() {
        response.ctx.accesskit_node_builder(response.id, |node| node.set_description(words.to_string()));
    }
}

/// Renames a control egui has already named, such as a button showing only a symbol.
pub fn rename(response: &Response, name: &str) {
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_label(name.to_string());
        if node.role() == Role::Label {
            node.clear_value();
        }
    });
}

/// A label that's clicked, read out as a button named by its words.
pub fn clickable(response: Response) -> Response {
    response.ctx.accesskit_node_builder(response.id, |node| {
        if let Some(words) = node.value().map(str::to_string) {
            node.set_label(words);
            node.clear_value();
        }
        node.set_role(Role::Button);
    });
    response
}

/// A button showing only a symbol, such as ×, given a name to be read out by.
pub fn called(response: Response, name: &str) -> Response {
    rename(&response, name);
    response
}

/// Whether a screen reader is being told about the window (AccessKit is on), so what it would be told is worth
/// working out.
pub fn wanted(response: &Response) -> bool {
    response.ctx.accesskit_node_builder(response.id, |_| ()).is_some()
}

/// An editor Limoniter draws itself: a text field whose value is what's at the cursor (the line, or the byte), and
/// whose description says where the cursor is. The whole of a file can be far too big to hand over each frame.
pub fn editor(response: &Response, name: &str, at_cursor: &str, place: &str, read_only: bool) {
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_role(Role::MultilineTextInput);
        node.set_label(name.to_string());
        node.set_value(at_cursor.to_string());
        node.set_description(place.to_string());
        if read_only {
            node.set_read_only();
        }
    });
}

#[cfg(test)]
pub mod tests {
    use eframe::egui::{self, Context, accesskit};

    /// The nodes of a frame's AccessKit tree, by their ids.
    pub fn nodes(output: &egui::FullOutput) -> Vec<(accesskit::NodeId, accesskit::Node)> {
        output.platform_output.accesskit_update.as_ref().expect("AccessKit is on").nodes.clone()
    }

    /// The node with this role whose name (or a label's value) starts with these words.
    pub fn find<'a>(nodes: &'a [(accesskit::NodeId, accesskit::Node)], role: accesskit::Role, name: &str) -> Option<&'a accesskit::Node> {
        nodes.iter().map(|(_, node)| node).find(|node| node.role() == role && node.label().or(node.value()).is_some_and(|l| l.starts_with(name)))
    }

    /// Every node's role and name, for a failing test to show.
    pub fn listing(nodes: &[(accesskit::NodeId, accesskit::Node)]) -> String {
        nodes.iter().map(|(_, node)| format!("{:?} {:?} {:?}", node.role(), node.label(), node.value())).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn painted_controls_get_a_role_and_a_name() {
        let ctx = Context::default();
        ctx.enable_accesskit();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let painted = ui.allocate_response(egui::vec2(20.0, 20.0), egui::Sense::click());
            super::button(&painted, "Close");
            let label = ui.add(egui::Label::new("Read-only").sense(egui::Sense::click()));
            super::chosen(&label, accesskit::Role::CheckBox, true, "Read-only");
            let symbol = ui.button("\u{D7}");
            super::rename(&symbol, "Close the panel");
        });
        output.textures_delta.clear();
        let nodes = nodes(&output);
        assert!(find(&nodes, accesskit::Role::Button, "Close").is_some(), "{}", listing(&nodes));
        assert!(find(&nodes, accesskit::Role::CheckBox, "Read-only").is_some_and(|n| n.toggled() == Some(accesskit::Toggled::True)));
        assert!(find(&nodes, accesskit::Role::Button, "Close the panel").is_some());
    }
}
