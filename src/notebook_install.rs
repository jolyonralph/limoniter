//! Installing packages into a notebook's kernel, from its Kernel menu or from an error that says a module is
//! missing. This file holds what needs no window: which names are allowed, the package a module comes from, how
//! a missing-module error is recognised, the code that does the installing, and how its output is judged. The
//! code runs on the kernel itself (see `Session::ask_with_messages`), so the packages go where the notebook's own
//! code will look for them, never into some environment guessed at.

/// How a kernel's language installs packages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tongue {
    Python,
    R,
}

impl Tongue {
    /// The language of a kernel, if it's one packages can be installed for.
    pub fn of(language: &str) -> Option<Self> {
        match language.to_lowercase().as_str() {
            "python" => Some(Tongue::Python),
            "r" => Some(Tongue::R),
            _ => None,
        }
    }
}

/// Whether a word is a package or extra's name for Python: letters, digits, and `-`, `_` or `.` between them.
fn python_name(word: &str) -> bool {
    let bytes = word.as_bytes();
    !bytes.is_empty() && bytes[0].is_ascii_alphanumeric() && bytes[bytes.len() - 1].is_ascii_alphanumeric() && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Whether a word is a package's name for R: it begins with a letter and has letters, digits and dots.
fn r_name(word: &str) -> bool {
    word.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
}

/// Whether text is a version specifier of pip's kind: one or more of `==1.2`, `>=1`, `<3`, `~=2.1`, `!=1.0`, `===1`,
/// joined by commas.
fn python_versions(text: &str) -> bool {
    text.split(',').all(|part| {
        let version = part.trim_start_matches(['=', '<', '>', '~', '!']);
        let operator = &part[..part.len() - version.len()];
        matches!(operator, "==" | ">=" | "<=" | "~=" | "!=" | "<" | ">" | "===") && !version.is_empty() && version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'*' | b'+' | b'!' | b'_' | b'-'))
    })
}

/// One word of a Python package list: a name, extras in `[]` and versions.
fn python_requirement(word: &str) -> bool {
    let name_end = word.find(['[', '=', '<', '>', '~', '!']).unwrap_or(word.len());
    let (name, mut rest) = word.split_at(name_end);
    if !python_name(name) {
        return false;
    }
    if let Some(inner) = rest.strip_prefix('[') {
        let Some(close) = inner.find(']') else { return false };
        if inner[..close].is_empty() || !inner[..close].split(',').all(python_name) {
            return false;
        }
        rest = &inner[close + 1..];
    }
    rest.is_empty() || python_versions(rest)
}

/// The packages typed into the box, checked, for a kernel of this language. Nothing that isn't plainly a package
/// name (with extras and versions, for Python) is let through, because what's accepted is put into code that
/// runs on the kernel.
pub fn check_names(tongue: Tongue, text: &str) -> Result<Vec<String>, String> {
    let words: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    if words.is_empty() {
        return Err(crate::i18n::tr("Type the names of the packages to install.").to_string());
    }
    for word in &words {
        let fine = match tongue {
            Tongue::Python => python_requirement(word),
            Tongue::R => r_name(word),
        };
        if !fine {
            return Err(match tongue {
                Tongue::Python => crate::i18n::tf!("\u{201c}{word}\u{201d} isn't a package name. Use letters, digits and - _ . only, with [extras] and versions such as ==2.2 if needed."),
                Tongue::R => crate::i18n::tf!("\u{201c}{word}\u{201d} isn't a package name. R's packages have letters, digits and dots only, and no versions."),
            });
        }
    }
    Ok(words)
}

/// The code that installs these (already checked) packages on the kernel.
pub fn install_code(tongue: Tongue, packages: &[String]) -> String {
    match tongue {
        // Each in quotes, so that a `>=` is never taken for the shell's redirection.
        // The caches are cleared after, so a module that was missing a moment ago is found at once.
        Tongue::Python => format!("%pip install {}\n__import__(\"importlib\").invalidate_caches()", packages.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(" ")),
        Tongue::R => {
            let list = packages.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(", ");
            // R only warns when a package isn't there to be had, so the code checks that each one arrived.
            format!("install.packages(c({list}), repos = \"https://cloud.r-project.org\")\nfor (p in c({list})) if (!requireNamespace(p, quietly = TRUE)) stop(paste0(\"The package \", p, \" could not be installed.\"))")
        }
    }
}

/// Modules whose package has another name on PyPI.
const MODULE_PACKAGES: &[(&str, &str)] = &[
    ("cv2", "opencv-python"),
    ("PIL", "pillow"),
    ("sklearn", "scikit-learn"),
    ("yaml", "pyyaml"),
    ("bs4", "beautifulsoup4"),
    ("skimage", "scikit-image"),
    ("dateutil", "python-dateutil"),
    ("dotenv", "python-dotenv"),
    ("Crypto", "pycryptodome"),
    ("serial", "pyserial"),
    ("OpenSSL", "pyopenssl"),
    ("jwt", "pyjwt"),
    ("docx", "python-docx"),
    ("pptx", "python-pptx"),
    ("fitz", "pymupdf"),
    ("magic", "python-magic"),
    ("usb", "pyusb"),
    ("git", "gitpython"),
    ("attr", "attrs"),
    ("psycopg2", "psycopg2-binary"),
    ("MySQLdb", "mysqlclient"),
    ("zmq", "pyzmq"),
    ("gi", "pygobject"),
    ("wx", "wxpython"),
    ("umap", "umap-learn"),
    ("tensorflow_datasets", "tensorflow-datasets"),
];

/// The package to install for a Python module: the table above for the usual odd ones, otherwise the module's
/// own name. A dotted module (`matplotlib.pyplot`) is its top level's.
pub fn package_for_module(module: &str) -> String {
    let top = module.split('.').next().unwrap_or(module);
    MODULE_PACKAGES.iter().find(|(m, _)| *m == top).map_or_else(|| top.to_string(), |(_, p)| (*p).to_string())
}

/// What a missing-module error asks for: the package, and the language it is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub tongue: Tongue,
    pub package: String,
}

/// The word inside the first pair of quotes (straight, curly or single) in a message.
fn quoted(message: &str) -> Option<&str> {
    let open = message.find(['\'', '\u{2018}', '"'])?;
    let after = &message[open..];
    let skip = after.chars().next()?.len_utf8();
    let inner = &after[skip..];
    let close = inner.find(['\'', '\u{2019}', '"'])?;
    Some(&inner[..close])
}

/// Whether an error (its name and message, as a cell showed it) says that a module or package isn't installed.
pub fn missing_from_error(name: &str, message: &str) -> Option<Missing> {
    let message = message.trim();
    let python_name_ok = matches!(name, "ModuleNotFoundError" | "ImportError");
    if python_name_ok && let Some(rest) = message.strip_prefix("No module named ") {
        let module = quoted(rest).unwrap_or_else(|| rest.trim());
        let package = package_for_module(module);
        // Private modules and anything odd-looking aren't offered: the button must only name something installable.
        return python_name(&package).then_some(Missing { tongue: Tongue::Python, package });
    }
    if let Some(at) = message.find("there is no package called ") {
        let package = quoted(&message[at + "there is no package called ".len()..])?;
        return r_name(package).then(|| Missing { tongue: Tongue::R, package: package.to_string() });
    }
    None
}

/// How an install went, from what the kernel printed and what it failed with. pip reports a failure on its own
/// lines starting `ERROR:`, without making the cell fail, so those count as a failure too. Success says nothing.
pub fn outcome(output: &str, failure: Option<&str>) -> Result<(), String> {
    if let Some(why) = failure {
        return Err(why.to_string());
    }
    // pip colours its messages, so the colour codes are taken off before the lines are read.
    let plain = crate::notebook_output::strip_ansi(output);
    match plain.lines().rev().find(|line| line.starts_with("ERROR:")) {
        Some(line) => Err(line.trim_start_matches("ERROR:").trim().to_string()),
        None => Ok(()),
    }
}

/// The last few lines of what an install printed, for the box. A progress line pip rewrites with carriage returns
/// shows as the last of its versions.
pub fn last_lines(output: &str, count: usize) -> String {
    let plain = crate::notebook_output::strip_ansi(output);
    let lines: Vec<&str> = plain.lines().map(|line| line.rsplit('\r').next().unwrap_or(line)).filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn py(text: &str) -> Result<Vec<String>, String> {
        check_names(Tongue::Python, text)
    }

    #[test]
    fn python_names_with_extras_and_versions_are_let_through() {
        assert_eq!(py("pandas numpy==1.26.4  matplotlib").unwrap(), ["pandas", "numpy==1.26.4", "matplotlib"]);
        assert_eq!(py("requests[socks,security]>=2.0,<3").unwrap(), ["requests[socks,security]>=2.0,<3"]);
        assert!(py("scikit-learn~=1.4 zope.interface typing_extensions pkg===1.0 a!=2.*").is_ok());
        assert!(py("x").is_ok());
    }

    #[test]
    fn anything_else_is_refused_before_it_gets_near_the_kernel() {
        for bad in ["", "   ", "pandas; import os", "pandas && rm", "-e .", "--index-url=http://x", "git+https://x/y.git", "./local", "a/b", "pandas==", "pandas>>1", "pandas[]", "pandas[x", "pandas[a b]", "\"quoted\"", "pandas$HOME", "pkg`x`", "-pandas", "pandas-", "pandas==1;2", "pandas==1 | x", "a@b", "p==1.0$"] {
            assert!(py(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(py("a b; c").is_err());
        assert_eq!(py("  ").unwrap_err(), "Type the names of the packages to install.");
    }

    #[test]
    fn r_names_are_letters_digits_and_dots_and_nothing_else() {
        assert_eq!(check_names(Tongue::R, "ggplot2 data.table").unwrap(), ["ggplot2", "data.table"]);
        for bad in ["ggplot2==3", "my-pkg", "1abc", "a_b", "x[y]", "a;b", "\"a\"", ".x"] {
            assert!(check_names(Tongue::R, bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_code_quotes_every_name() {
        let names = vec!["pandas>=2".to_string(), "numpy".to_string()];
        assert_eq!(install_code(Tongue::Python, &names), "%pip install \"pandas>=2\" \"numpy\"\n__import__(\"importlib\").invalidate_caches()");
        let r = install_code(Tongue::R, &["ggplot2".into(), "dplyr".into()]);
        assert!(r.starts_with("install.packages(c(\"ggplot2\", \"dplyr\"), repos = \"https://cloud.r-project.org\")"));
        assert!(r.contains("requireNamespace"));
    }

    #[test]
    fn only_python_and_r_can_install() {
        assert_eq!(Tongue::of("python"), Some(Tongue::Python));
        assert_eq!(Tongue::of("R"), Some(Tongue::R));
        assert_eq!(Tongue::of("julia"), None);
    }

    #[test]
    fn modules_map_to_their_packages() {
        for (module, package) in [("cv2", "opencv-python"), ("PIL", "pillow"), ("PIL.Image", "pillow"), ("sklearn", "scikit-learn"), ("sklearn.linear_model", "scikit-learn"), ("yaml", "pyyaml"), ("bs4", "beautifulsoup4"), ("skimage", "scikit-image"), ("dateutil", "python-dateutil"), ("dotenv", "python-dotenv"), ("Crypto", "pycryptodome"), ("matplotlib.pyplot", "matplotlib"), ("pandas", "pandas")] {
            assert_eq!(package_for_module(module), package, "{module}");
        }
    }

    #[test]
    fn python_errors_about_a_missing_module_are_recognised() {
        let python = |package: &str| Some(Missing { tongue: Tongue::Python, package: package.into() });
        assert_eq!(missing_from_error("ModuleNotFoundError", "No module named 'matplotlib'"), python("matplotlib"));
        assert_eq!(missing_from_error("ModuleNotFoundError", "No module named 'matplotlib.pyplot'"), python("matplotlib"));
        assert_eq!(missing_from_error("ModuleNotFoundError", "No module named 'cv2'"), python("opencv-python"));
        assert_eq!(missing_from_error("ImportError", "No module named bs4"), python("beautifulsoup4"));
        assert_eq!(missing_from_error("ImportError", "No module named \"yaml\""), python("pyyaml"));
        // Not about a missing module, or not something to install.
        assert_eq!(missing_from_error("ImportError", "cannot import name 'x' from 'y'"), None);
        assert_eq!(missing_from_error("ValueError", "No module named 'x'"), None);
        assert_eq!(missing_from_error("ModuleNotFoundError", "No module named '_private'"), None);
        assert_eq!(missing_from_error("ModuleNotFoundError", "No module named 'a; b'"), None);
        assert_eq!(missing_from_error("NameError", "name 'x' is not defined"), None);
    }

    #[test]
    fn r_errors_about_a_missing_package_are_recognised() {
        let r = |package: &str| Some(Missing { tongue: Tongue::R, package: package.into() });
        assert_eq!(missing_from_error("Error", "there is no package called \u{2018}ggplot2\u{2019}"), r("ggplot2"));
        assert_eq!(missing_from_error("Error", "there is no package called 'data.table'"), r("data.table"));
        assert_eq!(missing_from_error("Error", "Error in library(x) : there is no package called \u{2018}x\u{2019}"), r("x"));
        assert_eq!(missing_from_error("Error", "there is no package called \u{2018}a b\u{2019}"), None);
        assert_eq!(missing_from_error("Error", "object 'x' not found"), None);
    }

    #[test]
    fn an_install_is_judged_by_its_failure_and_pips_error_lines() {
        assert_eq!(outcome("Collecting x\nSuccessfully installed x-1.0\n", None), Ok(()));
        assert_eq!(outcome("WARNING: pip is old\nERROR: No matching distribution found for nope\n", None), Err("No matching distribution found for nope".into()));
        assert_eq!(outcome("", Some("KeyboardInterrupt: ")), Err("KeyboardInterrupt: ".into()));
        assert_eq!(outcome("\u{1b}[31mERROR: No matching distribution found for x\u{1b}[0m\n", None), Err("No matching distribution found for x".into()), "pip's colours are ignored");
        assert_eq!(last_lines("\u{1b}[32mok\u{1b}[0m", 1), "ok");
    }

    #[test]
    fn the_last_lines_are_kept_and_rewritten_progress_shows_its_latest() {
        assert_eq!(last_lines("a\nb\nc\nd\n", 2), "c\nd");
        assert_eq!(last_lines("a\n\n\nb", 5), "a\nb");
        assert_eq!(last_lines("10%\r50%\r100%\ndone", 2), "100%\ndone");
        assert_eq!(last_lines("", 3), "");
    }
}
