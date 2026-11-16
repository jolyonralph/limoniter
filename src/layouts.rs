//! Window, Layouts: named arrangements of the panels, such as one for writing, one for debugging and one for
//! comparing, kept in the settings and put back in one go.

/// One saved layout: its name, and what's open, one "name=value" a line, in the settings file's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub open: String,
}

/// The most layouts kept.
pub const MOST: usize = 20;

/// The layouts as settings lines: a name, then its switches joined by \u{1}.
pub fn to_settings(layouts: &[Layout]) -> String {
    layouts.iter().map(|layout| format!("layout={}\u{1}{}\n", layout.name, layout.open.lines().collect::<Vec<_>>().join("\u{1}"))).collect()
}

pub fn from_settings(settings: &str) -> Vec<Layout> {
    settings
        .lines()
        .filter_map(|line| line.strip_prefix("layout="))
        .filter_map(|value| {
            let mut parts = value.split('\u{1}');
            let name = parts.next()?.trim().to_string();
            let open: String = parts.filter(|part| part.contains('=')).map(|part| format!("{part}\n")).collect();
            (!name.is_empty()).then_some(Layout { name, open })
        })
        .take(MOST)
        .collect()
}

/// Keeps a layout under its name, in place of one of the same name, or first among the others.
pub fn keep(layouts: &mut Vec<Layout>, layout: Layout) {
    if let Some(existing) = layouts.iter_mut().find(|l| l.name.eq_ignore_ascii_case(&layout.name)) {
        *existing = layout;
        return;
    }
    layouts.insert(0, layout);
    layouts.truncate(MOST);
}

/// One switch from a layout, if it has it.
pub fn switch(open: &str, name: &str) -> Option<bool> {
    open.lines().filter_map(|line| line.split_once('=')).find(|(key, _)| key.trim() == name).and_then(|(_, value)| value.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_kept_by_name_and_read_back() {
        let mut layouts = Vec::new();
        keep(&mut layouts, Layout { name: "Writing".into(), open: "functions=false\noutput_window=false\n".into() });
        keep(&mut layouts, Layout { name: "Debugging".into(), open: "functions=true\noutput_window=true\n".into() });
        keep(&mut layouts, Layout { name: "writing".into(), open: "functions=true\n".into() });
        assert_eq!(layouts.len(), 2, "the same name, whatever its case, is replaced");
        assert_eq!(from_settings(&to_settings(&layouts)), layouts);
        assert_eq!(switch(&layouts[1].open, "functions"), Some(true));
        assert_eq!(switch(&layouts[0].open, "nothing"), None);
        assert!(from_settings("layout=\u{1}a=true\nlayout=Only a name\n").iter().map(|l| l.name.as_str()).eq(["Only a name"]));
    }
}
