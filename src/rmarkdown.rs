//! R Markdown (`.Rmd`) and Quarto (`.qmd`): their code chunks (```` ```{r label, echo=FALSE} ````) named by their
//! language for the Markdown preview to colour, and the command that knits or renders one to HTML.

use std::path::{Path, PathBuf};

/// Whether a file is R Markdown or Quarto, by its name.
pub fn is_rmarkdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "rmd" | "qmd" | "rmarkdown"))
}

/// A chunk's language, from a fenced block's info string: `{r}`, `{r setup, include=FALSE}`, `{python}`. Plain
/// fences (```` ```r ````) aren't chunks: they're shown as code, as Markdown shows them.
pub fn chunk_language(info: &str) -> Option<String> {
    let inner = info.trim().strip_prefix('{')?;
    let inner = inner.strip_suffix('}').unwrap_or(inner);
    let word: String = inner.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if word.is_empty() {
        return None;
    }
    Some(match word.to_ascii_lowercase().as_str() {
        "r" => "R".to_string(),
        "python" => "Python".to_string(),
        "sql" => "SQL".to_string(),
        "julia" => "Julia".to_string(),
        "bash" | "sh" => "Bourne Again Shell (bash)".to_string(),
        "js" | "ojs" | "javascript" => "JavaScript".to_string(),
        "cpp" | "rcpp" => "C++".to_string(),
        _ => word,
    })
}

/// How a file is knitted or rendered: Quarto for a `.qmd` (and for an `.Rmd` when R isn't there), and R's
/// rmarkdown package otherwise. `None` when neither is installed.
pub fn render_command(file: &Path, quarto: bool, rscript: bool) -> Option<String> {
    let quarto_file = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("qmd"));
    let shown = file.display().to_string();
    if quarto && (quarto_file || !rscript) {
        return Some(format!("quarto render {}", crate::py_env::quoted(&shown)));
    }
    if rscript && !quarto_file {
        // R wants forward slashes, and the path in single quotes inside the shell's double ones.
        let for_r = shown.replace('\\', "/").replace('\'', "\\'");
        return Some(format!("Rscript -e \"rmarkdown::render('{for_r}')\""));
    }
    None
}

/// The page knitting or rendering makes, beside the file with its name.
pub fn output_html(file: &Path) -> PathBuf {
    file.with_extension("html")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_named_by_their_language() {
        assert_eq!(chunk_language("{r}"), Some("R".into()));
        assert_eq!(chunk_language("{r setup, include=FALSE}"), Some("R".into()));
        assert_eq!(chunk_language("{python echo=FALSE}"), Some("Python".into()));
        assert_eq!(chunk_language("{bash}"), Some("Bourne Again Shell (bash)".into()));
        assert_eq!(chunk_language("r"), None, "a plain fence isn't a chunk");
        assert_eq!(chunk_language("{}"), None);
        assert!(is_rmarkdown(Path::new("report.Rmd")) && is_rmarkdown(Path::new("talk.qmd")) && !is_rmarkdown(Path::new("notes.md")));
    }

    #[test]
    fn knitting_uses_what_is_installed() {
        let rmd = Path::new("/w/report.Rmd");
        let qmd = Path::new("/w/talk.qmd");
        assert_eq!(render_command(rmd, false, true).as_deref(), Some("Rscript -e \"rmarkdown::render('/w/report.Rmd')\""));
        assert!(render_command(rmd, true, false).is_some_and(|c| c.starts_with("quarto render ")), "Quarto renders R Markdown too");
        assert!(render_command(rmd, true, true).is_some_and(|c| c.starts_with("Rscript")), "R's own, when it's there");
        assert!(render_command(qmd, true, true).is_some_and(|c| c.starts_with("quarto render ")));
        assert_eq!(render_command(qmd, false, true), None, "Quarto's files need Quarto");
        assert_eq!(render_command(rmd, false, false), None);
        assert_eq!(output_html(rmd), PathBuf::from("/w/report.html"));
    }
}
