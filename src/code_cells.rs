//! Cells in a script, as VS Code and Spyder have them in Python (`# %%`, and `# In[ ]:` from a notebook saved as a
//! script) and RStudio in R (`## ----` and `# %%`): each marker line starts a cell that runs to the next. Also what
//! Ctrl+Enter sends to the console when nothing is selected: the line, or the whole statement it begins (a
//! `def` with its body, a call over several lines), as RStudio and VS Code do.

/// Which language's markers and statements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    Python,
    R,
}

impl Style {
    /// By the language the highlighter names.
    pub fn for_language(language: &str) -> Option<Style> {
        match language {
            "Python" | "Cython" => Some(Style::Python),
            "R" => Some(Style::R),
            _ => None,
        }
    }
}

/// Whether a line starts a cell.
pub fn is_marker(line: &str, style: Style) -> bool {
    let line = line.trim_start();
    if line.starts_with("# %%") || line.starts_with("#%%") {
        return true;
    }
    match style {
        Style::Python => line.strip_prefix("# In[").is_some_and(|rest| rest.trim_end().ends_with("]:")),
        // knitr's spin chunks ("## ---- label"), and RStudio's sections: a comment ending in four dashes.
        Style::R => line.starts_with("## ----") || (line.starts_with('#') && line.trim_end().ends_with("----")),
    }
}

/// The cells of a file, each as (first line of its code, the line after its last), with the marker itself left
/// out. Lines before the first marker are a cell of their own if any are code.
pub fn cells(lines: &[&str], style: Style) -> Vec<(usize, usize)> {
    let markers: Vec<usize> = (0..lines.len()).filter(|&n| is_marker(lines[n], style)).collect();
    if markers.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    if lines[..markers[0]].iter().any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#')) {
        found.push((0, markers[0]));
    }
    for (i, &marker) in markers.iter().enumerate() {
        let end = markers.get(i + 1).copied().unwrap_or(lines.len());
        found.push((marker + 1, end));
    }
    found
}

/// The code of lines `start..end`, joined, without trailing blank lines.
pub fn code_of(lines: &[&str], start: usize, end: usize) -> String {
    let end = end.min(lines.len());
    let start = start.min(end);
    lines[start..end].join("\n").trim_end().to_string()
}

/// How far a line opens brackets: positive while some are left open. Strings and comments are passed over.
fn bracket_depth(line: &str, style: Style) -> i32 {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '#' => break,
            '"' | '\'' => quote = Some(c),
            '`' if style == Style::R => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// Whether a line of R carries on onto the next: it ends with an operator or a comma.
fn r_continues(line: &str) -> bool {
    let code = line.split(" #").next().unwrap_or(line).trim_end();
    ["%>%", "|>", "+", "-", "*", "/", ",", "<-", "=", "&", "|", "~"].iter().any(|op| code.ends_with(op))
}

/// The statement that starts at a line: (its first line, the line after its last). A Python line ending in a colon
/// takes the indented block under it (and its `elif`, `else`, `except` and `finally`); one leaving brackets open
/// takes the lines that close them; a decorator takes what it decorates. An R line takes the lines that close its
/// brackets, and those after an operator at its end.
pub fn statement_at(lines: &[&str], line: usize, style: Style) -> (usize, usize) {
    if line >= lines.len() {
        return (line, line);
    }
    let first = lines[line];
    let mut end = line + 1;
    let mut depth = bracket_depth(first, style);
    // Lines that close brackets left open, or carry on after an operator.
    let mut carries = style == Style::R && r_continues(first);
    while end < lines.len() && (depth > 0 || carries || (style == Style::Python && first.trim_end().ends_with('\\') && end == line + 1)) {
        depth += bracket_depth(lines[end], style);
        carries = style == Style::R && r_continues(lines[end]);
        end += 1;
    }
    if style == Style::Python {
        let opener = lines[end - 1].split(" #").next().unwrap_or("").trim_end();
        let decorated = first.trim_start().starts_with('@');
        if opener.ends_with(':') || decorated {
            let base = indent_of(first);
            let continuing = ["elif", "else", "except", "finally", "case"];
            while end < lines.len() {
                let next = lines[end];
                let word = next.trim_start().split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
                let more = next.trim().is_empty() || indent_of(next) > base || (indent_of(next) == base && (continuing.contains(&word) || (decorated && (word == "def" || word == "class" || next.trim_start().starts_with('@') || word == "async"))));
                if !more {
                    break;
                }
                end += 1;
            }
            while end > line + 1 && lines[end - 1].trim().is_empty() {
                end -= 1;
            }
        }
    }
    (line, end)
}

/// The line the cursor moves on to after sending `..end`: the next one with something on it.
pub fn next_code_line(lines: &[&str], end: usize) -> usize {
    (end..lines.len()).find(|&n| !lines[n].trim().is_empty()).unwrap_or(end.min(lines.len().saturating_sub(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_of_each_kind_are_found() {
        assert!(is_marker("# %%", Style::Python));
        assert!(is_marker("# %% Load the data", Style::Python));
        assert!(is_marker("#%%", Style::Python));
        assert!(is_marker("# In[3]:", Style::Python));
        assert!(is_marker("# In[ ]:", Style::Python));
        assert!(!is_marker("# Into the loop", Style::Python));
        assert!(!is_marker("## ---- not in python", Style::Python));
        assert!(is_marker("## ---- setup", Style::R));
        assert!(is_marker("# %% in R too", Style::R));
        assert!(!is_marker("x <- 1 # %% after code isn't one", Style::R));
        assert!(is_marker("# Load the data ----", Style::R), "RStudio's sections");
    }

    #[test]
    fn a_script_is_cut_into_cells() {
        let text = "import numpy as np\n# %% First\nx = 1\ny = 2\n\n# %% Second\nprint(x + y)\n";
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(cells(&lines, Style::Python), vec![(0, 1), (2, 5), (6, 7)]);
        assert_eq!(code_of(&lines, 2, 5), "x = 1\ny = 2");
        let only_comments: Vec<&str> = "# A heading comment\n# %%\nx = 1".lines().collect();
        assert_eq!(cells(&only_comments, Style::Python), vec![(2, 3)], "comments alone before the first marker aren't a cell");
        assert!(cells(&["x = 1"], Style::Python).is_empty(), "no markers, no cells");
    }

    #[test]
    fn a_python_statement_takes_its_block() {
        let text = "def f(x):\n    if x:\n        return 1\n    else:\n        return 2\n\nprint(f(1))\n@dec\ndef g():\n    pass\nvalues = [\n    1,\n    2,\n]\nfor i in range(3):\n    print(i)\n\n\nnext_one = 1";
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(statement_at(&lines, 0, Style::Python), (0, 5));
        assert_eq!(next_code_line(&lines, 5), 6);
        assert_eq!(statement_at(&lines, 6, Style::Python), (6, 7));
        assert_eq!(statement_at(&lines, 7, Style::Python), (7, 10), "a decorator and what it decorates");
        assert_eq!(statement_at(&lines, 10, Style::Python), (10, 14), "brackets left open");
        assert_eq!(statement_at(&lines, 14, Style::Python), (14, 16), "blank lines after the block aren't taken");
        let tries: Vec<&str> = "try:\n    x()\nexcept ValueError:\n    pass\nfinally:\n    done()\nafter()".lines().collect();
        assert_eq!(statement_at(&tries, 0, Style::Python), (0, 6));
        assert_eq!(statement_at(&["s = 'a(b'", "t = 1"], 0, Style::Python), (0, 1), "brackets in strings don't count");
    }

    #[test]
    fn an_r_statement_takes_its_pipes_and_brackets() {
        let text = "library(dplyr)\nmtcars %>%\n  filter(cyl == 4) %>%\n  summarise(n = n())\nf <- function(x) {\n  x + 1\n}\nggplot(df, aes(x, y)) +\n  geom_point()\nz <- 3";
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(statement_at(&lines, 0, Style::R), (0, 1));
        assert_eq!(statement_at(&lines, 1, Style::R), (1, 4));
        assert_eq!(statement_at(&lines, 4, Style::R), (4, 7));
        assert_eq!(statement_at(&lines, 7, Style::R), (7, 9));
        assert_eq!(statement_at(&lines, 9, Style::R), (9, 10));
    }
}
