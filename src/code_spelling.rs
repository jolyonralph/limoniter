//! Spell checking for code: only the comments and strings are read as prose, and, if the person asks, the words
//! inside names too (`getHTTPResponse2` is get, HTTP and Response). Words that code is full of (`stdout`, `args`),
//! short words, acronyms, hexadecimal, web addresses, paths and the language's own keywords are left alone, and a
//! project can keep a list of words of its own.
//!
//! This part works on one line of text at a time and has no say over what's a comment or a string: the syntax
//! colouring tells it. It makes a short text of just the words to check, and `unmask` turns what the spell checker
//! finds in that back into places in the line.

use crate::spelling::Mark;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

/// What the spell checker looks at in a file of code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CodeSpelling {
    /// Comments and strings only.
    #[default]
    Comments,
    /// Comments and strings, and the words in names.
    Names,
    Off,
}

impl CodeSpelling {
    pub const ALL: [Self; 3] = [Self::Comments, Self::Names, Self::Off];

    pub fn id(self) -> &'static str {
        match self {
            Self::Comments => "comments",
            Self::Names => "names",
            Self::Off => "off",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Comments => "Comments and strings",
            Self::Names => "Comments, strings and names",
            Self::Off => "Don't check code",
        }
    }
}

/// Words that turn up all over code and aren't in a dictionary, known without being asked.
const PROGRAMMING_WORDS: &str = "\
stdout stderr stdin stdio stdlib stdint stdbool stddef utf unicode ascii latin ansi config configs conf args arg argv argc \
regex regexp regexes async await enum enums struct structs typedef typedefs union unions impl impls trait traits \
const consts mut mutex mutexes lifetimes lifetime boolean bool bools int ints uint uints float floats char chars str strs \
vec vecs param params unsafe namespace namespaces middleware backend frontend plugin plugins runtime runtimes toolchain \
toolchains codec codecs init inits deinit uninit dealloc realloc malloc calloc alloc memcpy memset memcmp strlen strcpy \
printf fprintf sprintf snprintf scanf fscanf sscanf putc getc fputs fgets puts gets stdarg varargs nullptr nullable \
null nil eof eol crlf lf cr tab tabs stdlib ptr ptrs refs ref deref derefs iter iters iterator iterators impl \
callback callbacks lambda lambdas closure closures stub stubs mock mocks mocking fixture fixtures dataset datasets \
json yaml toml xml html xhtml css scss sass less svg csv tsv sql sqlite mysql postgres postgresql mongodb redis \
http https ftp ssh ssl tls tcp udp dns url urls uri uris uuid guid api apis cli gui ide sdk ndk jdk jre jvm cpu gpu \
ram rom usb pci bios uefi kernel kernels syscall syscalls ioctl ioctls fd fds pid pids tid uid gid errno \
github gitlab gitignore gitattributes git svn mercurial npm npx pnpm yarn pip pipx cargo rustc rustup clippy rustfmt \
crate crates mod mods pub dyn repr cfg bitflags serde tokio egui eframe wasm webassembly wgpu opengl vulkan directx \
linux unix macos ios android windows msvc gcc clang llvm gnu glibc musl posix \
filesystem filesystems filename filenames pathname dirname basename username hostname localhost loopback \
timestamp timestamps timeout timeouts timezone timezones datetime utc gmt epoch \
checksum checksums hash hashes hashing sha md crc crc32 base64 hex hexdump bitmap bitmaps bitmask bitmasks bitfield bitfields \
bitwise bytecode bytecodes opcode opcodes mnemonic mnemonics endian endianness bigendian littleendian \
refactor refactors refactoring refactored linter linters linting lint lints formatter formatters minify minified \
transpile transpiler transpilers compiler compilers recompile recompiled precompiled precompile \
serialise serialised serialisation deserialise deserialised serialize serialized serialization deserialize deserialized \
tokenise tokenised tokeniser tokenize tokenized tokenizer lexer lexers parser parsers ast asts \
unmarshal marshal marshalling enum variadic inline inlined inlining \
webhook webhooks websocket websockets oauth jwt cors csrf xss sandbox sandboxed sandboxing \
boolean booleans tuple tuples ctor dtor getter getters setter setters accessor accessors mutator mutators \
subclass subclasses superclass interface interfaces abstract overload overloads override overrides overridden \
polymorphic polymorphism generics generic templated templates enumerate enumerated enumerates \
unwrap unwraps unwrapped expect expects ok err some none \
readme changelog todo fixme hack xxx noop nop nops eval exec execv fork forks pipe pipes \
sprite sprites tilemap tilemaps framebuffer framebuffers vram wram sram eeprom dma irq irqs nmi isr vblank hblank \
dev devs env envs sys proc procs lib libs bin src dist tmp tmpfile temp util utils misc impl docs doc \
param returns retval tparam typeparam deprecated noqa nolint pylint eslint prettier mypy pytest unittest \
bytes byte kib mib gib kb mb gb tb khz mhz ghz hz px em rem vh vw rgb rgba hsl hsla cmyk srgb \
bool utf8 utf16 utf32 ucs usize isize u8 i8 u16 i16 u32 i32 u64 i64 u128 i128 f32 f64 \
undef ifdef ifndef endif elif elifdef pragma define defines includes include macro macros preprocessor \
gettext i18n l10n locale locales unicode codepoint codepoints grapheme graphemes \
func funcs fn fns proc defn def defs var vars let lambda \
jsx tsx typescript javascript nodejs deno webpack vite babel rollup \
python pyc pyi numpy pandas matplotlib django flask jinja \
kotlin scala haskell erlang elixir clojure ocaml fortran cobol perl ruby golang \
bsd gpl lgpl mit apache licence licences licensing copyleft \
uninstall uninstaller installer installers unzip untar gzip gunzip bzip zlib lzma zstd tarball \
gui ui ux dpi gfx hud fps vsync hotkey hotkeys keybinding keybindings keymap keymaps \
modal modals tooltip tooltips dropdown dropdowns checkbox checkboxes textbox textboxes combobox comboboxes scrollbar scrollbars \
autocomplete autosave autosaved autoload autorun autostart autoindent \
dedupe dedup deduplicate deduplicated dedented dedent indent indents indented unindent \
realtime multithread multithreaded multiprocess singleton singletons \
subdir subdirs subfolder subfolders subpath subpaths substring substrings superset subset \
pre post prefix prefixes suffix suffixes preload preloaded postprocess preprocess \
metadata namespace roundtrip roundtrips workaround workarounds whitespace whitespaces \
";

thread_local! {
    static MODE: Cell<CodeSpelling> = const { Cell::new(CodeSpelling::Comments) };
    static PROJECT_WORDS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static PROJECT_OPEN: Cell<bool> = const { Cell::new(false) };
    static BUILT_IN: HashSet<&'static str> = PROGRAMMING_WORDS.split_whitespace().collect();
    /// Each name's first line in a file, for the file as it was at a version, so a name is read where it starts.
    static FIRST_USES: RefCell<Option<(FileKey, HashMap<String, usize>)>> = const { RefCell::new(None) };
}

pub fn set_mode(mode: CodeSpelling) {
    MODE.set(mode);
}

pub fn mode() -> CodeSpelling {
    MODE.get()
}

/// The words the open project keeps (or none, with no project open), as it has them. Says whether they differ from
/// the last ones.
pub fn set_project_words(words: Option<&[String]>) -> bool {
    PROJECT_OPEN.set(words.is_some());
    let wanted: HashSet<String> = words.unwrap_or_default().iter().map(|word| word.to_lowercase()).collect();
    PROJECT_WORDS.with_borrow_mut(|have| {
        let changed = *have != wanted;
        *have = wanted;
        changed
    })
}

/// Whether a project is open to keep a word in.
pub fn project_open() -> bool {
    PROJECT_OPEN.get()
}

/// Whether the project keeps this word.
pub fn is_project_word(word: &str) -> bool {
    PROJECT_WORDS.with_borrow(|words| words.contains(&word.to_lowercase()))
}

/// Whether a word is one the code world knows already, or the project does.
pub fn is_known(word: &str) -> bool {
    let lower = word.to_lowercase();
    BUILT_IN.with(|words| words.contains(lower.as_str())) || PROJECT_WORDS.with_borrow(|words| words.contains(&lower))
}

/// Languages that are written for people to read, so their text is checked all through as before.
pub fn is_prose_language(name: &str) -> bool {
    let name = name.to_lowercase();
    ["markdown", "latex", "tex", "restructuredtext", "asciidoc", "org mode", "textile", "commit message", "html", "xml", "vue", "svelte", "plain text"].iter().any(|kind| name.contains(kind))
}

/// The text to check, and where each of its characters came from in the line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    pub origin: Vec<usize>,
}

/// Stretches of a line, by character, that the syntax colouring found.
pub struct Regions<'a> {
    pub comments: &'a [(usize, usize)],
    pub strings: &'a [(usize, usize)],
    pub keywords: &'a [(usize, usize)],
}

fn inside(spans: &[(usize, usize)], at: usize) -> bool {
    spans.iter().any(|&(from, to)| at >= from && at < to)
}

/// The parts of one run of letters, by character: `HTTPResponse` is `HTTP` and `Response`, `getURL` is `get` and
/// `URL`, and `parseXML` is `parse` and `XML`.
fn camel_parts(letters: &[char]) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut from = 0;
    for at in 1..letters.len() {
        let (before, here) = (letters[at - 1], letters[at]);
        let next_is_lower = letters.get(at + 1).is_some_and(|c| c.is_lowercase());
        let boundary = (before.is_lowercase() && here.is_uppercase()) || (before.is_uppercase() && here.is_uppercase() && next_is_lower);
        if boundary {
            parts.push((from, at));
            from = at;
        }
    }
    parts.push((from, letters.len()));
    parts
}

/// The words in a name or a word, by character, ready to look up: split at camelCase, PascalCase, snake_case,
/// kebab-case and digits. An apostrophe between letters stays in its word (`don't`).
pub fn name_parts(token: &[char]) -> Vec<(usize, usize)> {
    let mut parts = Vec::new();
    let mut at = 0;
    while at < token.len() {
        if !token[at].is_alphabetic() {
            at += 1;
            continue;
        }
        let from = at;
        while at < token.len() && (token[at].is_alphabetic() || (matches!(token[at], '\'' | '\u{2019}') && at > from && token.get(at + 1).is_some_and(|c| c.is_alphabetic()))) {
            at += 1;
        }
        let run = &token[from..at];
        if run.contains(&'\'') || run.contains(&'\u{2019}') {
            parts.push((from, at));
        } else {
            parts.extend(camel_parts(run).into_iter().map(|(a, z)| (from + a, from + z)));
        }
    }
    parts
}

/// Whether a part is left alone for being too short to matter, or an acronym.
fn left_alone(part: &[char]) -> bool {
    let letters = part.iter().filter(|c| c.is_alphabetic()).count();
    if letters <= 3 {
        return true;
    }
    // HTTP, ASCII, MAX_LENGTH: all capitals.
    part.iter().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

fn is_hex_run(token: &[char]) -> bool {
    !token.is_empty() && token.iter().all(char::is_ascii_hexdigit) && token.iter().any(char::is_ascii_digit)
}

/// A web address, an e-mail address or a path: what's between spaces holds one.
fn is_address_or_path(chunk: &[char]) -> bool {
    let text: String = chunk.iter().collect();
    let text = text.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';' | '*'));
    if text.contains("://") || text.to_lowercase().starts_with("www.") || (text.contains('@') && text.contains('.')) {
        return true;
    }
    let mut starts = ["/", "./", "../", "~/"].iter().filter(|start| text.strip_prefix(*start).and_then(|rest| rest.chars().next()).is_some_and(char::is_alphanumeric));
    let drive = text.len() > 3 && text.as_bytes()[1] == b':' && matches!(text.as_bytes()[2], b'\\' | b'/') && text.as_bytes()[0].is_ascii_alphabetic();
    if starts.next().is_some() || drive {
        return true;
    }
    let inner = text.trim_matches(|c: char| matches!(c, '/' | '\\' | '*'));
    let separators = inner.chars().filter(|c| matches!(c, '/' | '\\')).count();
    separators >= 2 || (separators == 1 && inner.contains('.'))
}

/// The text to spell check in a line of code, and where it came from. Whether each name is looked at is up to
/// `first_use`: it's asked once for each name in the line that isn't in a comment or a string, and says whether
/// the line is where the name starts.
pub fn masked_line(line: &str, regions: &Regions, mode: CodeSpelling, first_use: &mut dyn FnMut(&str) -> bool) -> Masked {
    let mut masked = Masked::default();
    if mode == CodeSpelling::Off {
        return masked;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let here = chars[at];
        if !(here.is_alphanumeric() || matches!(here, '_' | '-' | '\'' | '\u{2019}')) {
            at += 1;
            continue;
        }
        let from = at;
        while at < chars.len() && (chars[at].is_alphanumeric() || matches!(chars[at], '_' | '-' | '\'' | '\u{2019}')) {
            at += 1;
        }
        let mut token = (from, at);
        // Dashes and apostrophes at the ends are punctuation.
        while token.0 < token.1 && !chars[token.0].is_alphanumeric() && chars[token.0] != '_' {
            token.0 += 1;
        }
        while token.1 > token.0 && !chars[token.1 - 1].is_alphanumeric() && chars[token.1 - 1] != '_' {
            token.1 -= 1;
        }
        if token.0 >= token.1 {
            continue;
        }
        let in_prose = inside(regions.comments, token.0) || inside(regions.strings, token.0);
        if !in_prose && (mode != CodeSpelling::Names || inside(regions.keywords, token.0)) {
            continue;
        }
        let mut word: Vec<char> = chars[token.0..token.1].to_vec();
        let mut start = token.0;
        // A letter straight after a backslash in a string is an escape (\n, \t), not the start of a word.
        if in_prose && start > 0 && chars[start - 1] == '\\' && word.first().is_some_and(|c| c.is_alphabetic()) {
            word.remove(0);
            start += 1;
        }
        if word.is_empty() {
            continue;
        }
        // Hexadecimal: 0x1F, #deadbeef, or digits and a-f letters only.
        let marker = start.checked_sub(1).map(|i| chars[i]);
        let hex_digits = word.iter().skip(if word.len() > 2 && word[0] == '0' && matches!(word[1], 'x' | 'X') { 2 } else { 0 });
        let prefixed_hex = (matches!(marker, Some('#' | '$')) || (word.len() > 2 && word[0] == '0' && matches!(word[1], 'x' | 'X'))) && hex_digits.clone().all(char::is_ascii_hexdigit);
        if prefixed_hex || is_hex_run(&word) {
            continue;
        }
        // The stretch between spaces it's in: a web address or a path leaves all its words alone.
        let mut chunk_from = start;
        while chunk_from > 0 && !chars[chunk_from - 1].is_whitespace() {
            chunk_from -= 1;
        }
        let mut chunk_to = token.1;
        while chunk_to < chars.len() && !chars[chunk_to].is_whitespace() {
            chunk_to += 1;
        }
        if is_address_or_path(&chars[chunk_from..chunk_to]) {
            continue;
        }
        let parts: Vec<(usize, usize)> = name_parts(&word).into_iter().filter(|&(a, z)| !left_alone(&word[a..z]) && !is_known(&word[a..z].iter().collect::<String>())).collect();
        if parts.is_empty() {
            continue;
        }
        if !in_prose {
            let name: String = word.iter().collect();
            if !first_use(&name) {
                continue;
            }
        }
        for (a, z) in parts {
            if !masked.text.is_empty() {
                masked.text.push(' ');
                masked.origin.push(masked.origin.last().copied().unwrap_or(0));
            }
            for (offset, c) in word[a..z].iter().enumerate() {
                masked.text.push(*c);
                masked.origin.push(start + a + offset);
            }
        }
    }
    masked
}

/// What the spell checker found in a masked text, as places in the line it came from.
pub fn unmask(masked: &Masked, marks: &[Mark]) -> Vec<Mark> {
    marks.iter().filter_map(|&(from, to)| Some((*masked.origin.get(from)?, masked.origin.get(to.checked_sub(1)?)? + 1))).collect()
}

// ---------- Where a name starts ----------

/// Which text the first-use index was made from: the buffer, its version and its length.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileKey {
    buffer: usize,
    version: u64,
    lines: usize,
}

/// Past this many lines names aren't read, as finding where each starts would be slow.
const MOST_LINES_INDEXED: usize = 200_000;

fn tokens_of(line: &str, mut each: impl FnMut(&str)) {
    let mut word = String::new();
    for c in line.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || matches!(c, '_' | '-') {
            word.push(c);
        } else {
            if word.chars().count() >= 4 {
                each(&word);
            }
            word.clear();
        }
    }
}

/// Whether line `line` is where a name first appears in the text, so that its words are read there and not at every
/// use. A name that's in a comment before its first use in code is read in the comment instead.
pub fn is_first_use(buffer: &crate::text_buffer::TextBuffer, line: usize, name: &str) -> bool {
    let key = FileKey { buffer: std::ptr::from_ref(buffer) as usize, version: buffer.version(), lines: buffer.line_count() };
    if key.lines > MOST_LINES_INDEXED {
        return false;
    }
    FIRST_USES.with_borrow_mut(|kept| {
        if kept.as_ref().is_none_or(|(have, _)| *have != key) {
            let mut firsts: HashMap<String, usize> = HashMap::new();
            for number in 0..key.lines {
                tokens_of(&buffer.line(number), |word| {
                    firsts.entry(word.to_string()).or_insert(number);
                });
            }
            *kept = Some((key, firsts));
        }
        kept.as_ref().and_then(|(_, firsts)| firsts.get(name)).is_none_or(|first| *first >= line)
    })
}

// ---------- A project's own words ----------

/// Where a folder project keeps its words: a file in the folder, so Git has them too.
pub fn words_file(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".limoniter").join("words.txt")
}

/// The words in a project's list: one a line, blank lines and lines starting # left out, kept in order and without
/// repeats.
pub fn read_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')).map(str::to_lowercase).collect();
    words.sort();
    words.dedup();
    words
}

pub fn write_words(words: &[String]) -> String {
    words.iter().map(|word| format!("{word}\n")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        name_parts(&chars).into_iter().map(|(a, z)| chars[a..z].iter().collect()).collect()
    }

    fn masked_of(line: &str, comments: &[(usize, usize)], strings: &[(usize, usize)], mode: CodeSpelling) -> Masked {
        masked_line(line, &Regions { comments, strings, keywords: &[] }, mode, &mut |_| true)
    }

    fn checked(line: &str, comments: &[(usize, usize)], strings: &[(usize, usize)], mode: CodeSpelling) -> String {
        masked_of(line, comments, strings, mode).text
    }

    #[test]
    fn names_are_split_at_every_kind_of_joint() {
        assert_eq!(parts("getHTTPResponse2"), ["get", "HTTP", "Response"]);
        assert_eq!(parts("snake_case_name"), ["snake", "case", "name"]);
        assert_eq!(parts("kebab-case-name"), ["kebab", "case", "name"]);
        assert_eq!(parts("PascalCaseName"), ["Pascal", "Case", "Name"]);
        assert_eq!(parts("parseXML"), ["parse", "XML"]);
        assert_eq!(parts("utf8string"), ["utf", "string"]);
        assert_eq!(parts("don't"), ["don't"]);
    }

    #[test]
    fn code_outside_comments_and_strings_is_left_alone_by_default() {
        assert_eq!(checked("let recieve = 1;", &[], &[], CodeSpelling::Comments), "");
        assert_eq!(checked("x = 1; // recieve it", &[(7, 20)], &[], CodeSpelling::Comments), "recieve");
        assert_eq!(checked("say(\"helo wrld\")", &[], &[(3, 15)], CodeSpelling::Comments), "helo wrld");
    }

    #[test]
    fn names_are_read_when_asked_and_by_their_parts() {
        let masked = masked_of("let recieveMessage = 1;", &[], &[], CodeSpelling::Names);
        assert_eq!(masked.text, "recieve Message");
        // Each character knows where it came from, and a mark on a part is a mark in the line.
        assert_eq!(unmask(&masked, &[(0, 7)]), [(4, 11)]);
        assert_eq!(unmask(&masked, &[(8, 15)]), [(11, 18)]);
        assert_eq!(checked("recieveMessage", &[], &[], CodeSpelling::Off), "");
    }

    #[test]
    fn a_name_is_read_where_it_starts_and_not_at_every_use() {
        let mut asked = Vec::new();
        let regions = Regions { comments: &[], strings: &[], keywords: &[] };
        let masked = masked_line("x = recieveCount + 1", &regions, CodeSpelling::Names, &mut |name| {
            asked.push(name.to_string());
            false
        });
        assert_eq!(masked.text, "");
        assert_eq!(asked, ["recieveCount"], "only the names with a word to check are asked about");
        // Comments and strings are read at every use, whatever the names do.
        let masked = masked_line("// recieveCount", &Regions { comments: &[(0, 15)], strings: &[], keywords: &[] }, CodeSpelling::Names, &mut |_| false);
        assert_eq!(masked.text, "recieve Count");
    }

    #[test]
    fn short_words_acronyms_hex_addresses_paths_and_known_words_are_left_alone() {
        let line = "// the cat HTTP MAX_LENGTH 0xDEADBEEF #cafebabe deadbeef0 https://exemple.org/pagee src/mian.rs ./thingy/stuf C:\\dirr\\file stdout args regex async";
        let all = [(0, line.chars().count())];
        assert_eq!(checked(line, &all, &[], CodeSpelling::Comments), "");
        // The same words, not left alone, would be checked.
        assert_eq!(checked("// recieve", &[(0, 10)], &[], CodeSpelling::Comments), "recieve");
    }

    #[test]
    fn keywords_and_escapes_are_left_alone() {
        let regions = Regions { comments: &[], strings: &[], keywords: &[(0, 6)] };
        assert_eq!(masked_line("struct recieve", &regions, CodeSpelling::Names, &mut |_| true).text, "recieve", "the keyword is skipped, the name isn't");
        let regions = Regions { comments: &[], strings: &[(0, 12)], keywords: &[] };
        assert_eq!(masked_line("\"line\\nbreak\"", &regions, CodeSpelling::Comments, &mut |_| true).text, "line break");
    }

    #[test]
    fn the_projects_own_words_are_known_in_any_case() {
        set_project_words(Some(&["Frobnicate".to_string()]));
        assert!(is_known("frobnicate") && is_known("FROBNICATE"));
        assert!(is_known("stdout") && is_known("Regex"), "common programming words are known out of the box");
        assert_eq!(checked("// frobnicate widgets", &[(0, 21)], &[], CodeSpelling::Comments), "widgets");
        assert!(!set_project_words(Some(&["frobnicate".to_string()])), "the same words again change nothing");
        assert!(project_open() && is_project_word("Frobnicate") && !is_project_word("stdout"));
        set_project_words(None);
        assert!(!is_known("frobnicate") && !project_open());
    }

    #[test]
    fn a_projects_word_list_is_read_and_written_as_a_line_a_word() {
        let words = read_words("# our words\nFrobnicate\n\n  widget  \nfrobnicate\n");
        assert_eq!(words, ["frobnicate", "widget"]);
        assert_eq!(read_words(&write_words(&words)), words);
        assert_eq!(words_file(std::path::Path::new("proj")), std::path::Path::new("proj").join(".limoniter").join("words.txt"));
    }

    #[test]
    fn markup_and_prose_are_not_code() {
        assert!(is_prose_language("Markdown") && is_prose_language("HTML") && is_prose_language("LaTeX"));
        assert!(!is_prose_language("Rust") && !is_prose_language("C++") && !is_prose_language("Python"));
    }

    #[test]
    fn the_setting_has_names_that_come_back() {
        for mode in CodeSpelling::ALL {
            assert_eq!(CodeSpelling::from_id(mode.id()), Some(mode));
        }
        assert_eq!(CodeSpelling::default(), CodeSpelling::Comments);
    }
}
