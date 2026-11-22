//! `.http` and `.rest` files, as VS Code's REST Client reads them: requests separated by `###`, each a method line,
//! headers and a body; `@name = value` variables used as `{{name}}`; built-in `{{$guid}}` and friends; and values
//! taken from the response of an earlier named request. This module only reads and works things out (no network, no
//! window), so it can be tested on its own. Sending is in `http_send.rs`, and the window's side in `http_host.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Whether a file is one, by its name.
pub fn is_http_name(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("http") || e.eq_ignore_ascii_case("rest"))
}

const METHODS: [&str; 9] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT"];

/// One request as written in the file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    /// From `# @name login`.
    pub name: Option<String>,
    /// The first line of the request's block (the line after the `###` that starts it), counting from 0.
    pub first_line: usize,
    /// The line the method and address are on.
    pub line: usize,
    /// The block's last line.
    pub last_line: usize,
    pub method: String,
    /// The address as written, with any `{{variables}}` still in it.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// The body as written, with `< ./file` lines and `{{variables}}` still in it.
    pub body: String,
    /// From `# @no-redirect`: the redirect is shown, not followed.
    pub no_redirect: bool,
    /// From `# @no-cookie-jar`: no cookies are sent or kept.
    pub no_cookie_jar: bool,
}

/// A whole file: its requests, and the variables it defines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HttpFile {
    pub requests: Vec<Request>,
    pub variables: Vec<(String, String)>,
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    (t.starts_with('#') || t.starts_with("//")) && !t.starts_with("###")
}

/// A line of the file like `@host = localhost` as (name, value).
fn variable_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    (!name.is_empty() && !name.contains(char::is_whitespace)).then(|| (name.to_string(), value.trim().to_string()))
}

/// The word after `@name` in a comment such as `# @name login`, and the other `@` marks.
fn comment_mark<'a>(line: &'a str, mark: &str) -> Option<&'a str> {
    let t = line.trim_start().trim_start_matches('#').trim_start_matches('/').trim();
    let rest = t.strip_prefix(mark)?;
    if rest.is_empty() { Some("") } else if rest.starts_with(char::is_whitespace) || rest.starts_with('=') { Some(rest.trim_start_matches(['=', ' ', '\t']).trim()) } else { None }
}

/// Splits a request line into its method and address; a line with only an address is a GET.
fn request_line(line: &str) -> (String, String) {
    let line = line.trim();
    let (method, rest) = match line.split_once(char::is_whitespace) {
        Some((first, rest)) if METHODS.contains(&first.to_ascii_uppercase().as_str()) => (first.to_ascii_uppercase(), rest.trim()),
        _ => ("GET".to_string(), line),
    };
    (method, rest.to_string())
}

/// An address without the protocol version (`HTTP/1.1`) that may follow it: the address is what matters.
fn without_version(url: &str) -> &str {
    match url.rsplit_once(char::is_whitespace) {
        Some((before, last)) if last.len() >= 6 && last.is_char_boundary(5) && last[..5].eq_ignore_ascii_case("HTTP/") => before.trim(),
        _ => url,
    }
}

/// Reads a file's text.
pub fn parse(text: &str) -> HttpFile {
    let lines: Vec<&str> = text.lines().collect();
    let mut file = HttpFile::default();
    let mut start = 0;
    while start <= lines.len() {
        let mut end = start;
        while end < lines.len() && !lines[end].trim_start().starts_with("###") {
            end += 1;
        }
        // The block is lines[start..end].
        if let Some(request) = parse_block(&lines, start, end, &mut file.variables) {
            file.requests.push(request);
        }
        start = end + 1;
    }
    file
}

fn parse_block(lines: &[&str], start: usize, end: usize, variables: &mut Vec<(String, String)>) -> Option<Request> {
    let mut request = Request { first_line: start, last_line: end.saturating_sub(1).max(start), ..Request::default() };
    let mut at = start;
    // Before the request line: blank lines, comments (some of which name or tune the request) and variables.
    while at < end {
        let line = lines[at];
        if line.trim().is_empty() {
            at += 1;
        } else if let Some(variable) = variable_line(line) {
            variables.push(variable);
            at += 1;
        } else if is_comment(line) {
            if let Some(name) = comment_mark(line, "@name") {
                request.name = Some(name.to_string()).filter(|n| !n.is_empty());
            }
            request.no_redirect |= comment_mark(line, "@no-redirect").is_some();
            request.no_cookie_jar |= comment_mark(line, "@no-cookie-jar").is_some();
            at += 1;
        } else {
            break;
        }
    }
    if at >= end {
        return None;
    }
    request.line = at;
    (request.method, request.url) = request_line(lines[at]);
    at += 1;
    // A long address may go on over the next lines, which begin with ? or &.
    while at < end && (lines[at].trim_start().starts_with('?') || lines[at].trim_start().starts_with('&')) {
        // The version, if it was written, is at the end of the last of these lines.
        request.url = without_version(&request.url).to_string();
        request.url.push_str(lines[at].trim());
        at += 1;
    }
    request.url = without_version(&request.url).to_string();
    // Headers run to the first blank line.
    while at < end && !lines[at].trim().is_empty() {
        let line = lines[at];
        at += 1;
        if is_comment(line) {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            request.headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    // The body is the rest, without the blank lines that end it.
    let mut body: Vec<&str> = lines[at.min(end)..end].to_vec();
    while body.first().is_some_and(|l| l.trim().is_empty()) {
        body.remove(0);
    }
    while body.last().is_some_and(|l| l.trim().is_empty()) {
        body.pop();
    }
    request.body = body.join("\n");
    Some(request)
}

/// Which request the cursor's line belongs to: the one whose block it's in, or else the nearest above it (or the
/// first, if it's above them all).
pub fn request_at(requests: &[Request], line: usize) -> Option<usize> {
    requests.iter().position(|r| r.first_line <= line && line <= r.last_line).or_else(|| requests.iter().rposition(|r| r.first_line <= line)).or(if requests.is_empty() { None } else { Some(0) })
}

// ---------- Values from earlier responses ----------

/// What an earlier named request's response said, kept for `{{name.response...}}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stored {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Everything a `{{...}}` may take its value from.
pub struct Scope<'a> {
    /// The file's own `@name = value` variables.
    pub file: &'a [(String, String)],
    /// The chosen environment's.
    pub environment: &'a [(String, String)],
    /// The project's `.env`.
    pub dotenv: &'a [(String, String)],
    /// The responses of the named requests sent so far.
    pub responses: &'a HashMap<String, Stored>,
    /// Where `< ./file` is looked for: the folder the `.http` file is in.
    pub folder: &'a Path,
}

fn lookup<'a>(list: &'a [(String, String)], name: &str) -> Option<&'a str> {
    // The last definition wins, as a later `@name =` overrides an earlier one.
    list.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Fills in every `{{...}}` in some text.
pub fn expand(text: &str, scope: &Scope) -> Result<String, String> {
    expand_deep(text, scope, 0)
}

fn expand_deep(text: &str, scope: &Scope, depth: usize) -> Result<String, String> {
    if depth > 8 {
        return Err(crate::i18n::tr("The variables refer to each other in a circle.").to_string());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open + 2..].find("}}") else { break };
        out.push_str(&rest[..open]);
        let inner = rest[open + 2..open + 2 + close].trim();
        out.push_str(&value_of(inner, scope, depth)?);
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn value_of(inner: &str, scope: &Scope, depth: usize) -> Result<String, String> {
    if let Some(built_in) = inner.strip_prefix('$') {
        return built_in_value(built_in, scope);
    }
    if let Some((name, after)) = inner.split_once(".response.") {
        return response_value(name.trim(), after.trim(), scope);
    }
    if let Some(value) = lookup(scope.file, inner).or_else(|| lookup(scope.environment, inner)) {
        return expand_deep(value, scope, depth + 1);
    }
    Err(crate::i18n::tf!("{{{{{inner}}}}} isn't defined: add @{inner} = to the file, or to the environment."))
}

/// The time now, in seconds since 1970 (a counter that is only ever used to make something random).
fn now_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// Random bytes, from the system's own random keys: enough for an id or a number in a request.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(now_nanos());
    hasher.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    hasher.finish()
}

/// A version 4 UUID.
pub fn guid() -> String {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&random_u64().to_le_bytes());
    bytes[8..].copy_from_slice(&random_u64().to_le_bytes());
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// Seconds for an offset such as `-1 d` or `3 h`.
fn offset_seconds(amount: &str, unit: &str) -> Result<i64, String> {
    let n: i64 = amount.parse().map_err(|_| crate::i18n::tf!("{amount} isn't a number of {unit}."))?;
    let each = match unit {
        "ms" => return Ok(n / 1000),
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        "w" => 7 * 86_400,
        "M" => 30 * 86_400,
        "y" => 365 * 86_400,
        _ => return Err(crate::i18n::tf!("{unit} isn't a unit of time here: use s, m, h, d, w, M or y.")),
    };
    Ok(n * each)
}

fn built_in_value(text: &str, scope: &Scope) -> Result<String, String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let name = words.first().copied().unwrap_or("");
    let args = &words[words.len().min(1)..];
    match name {
        "guid" | "uuid" => Ok(guid()),
        "timestamp" => {
            let now = chrono::Utc::now().timestamp();
            let offset = if let [amount, unit] = args { offset_seconds(amount, unit)? } else { 0 };
            Ok((now + offset).to_string())
        }
        "datetime" | "localDatetime" => {
            let offset = if let [_, amount, unit] = args { offset_seconds(amount, unit)? } else { 0 };
            let moment = chrono::Utc::now() + chrono::Duration::seconds(offset);
            match args.first().copied() {
                Some("rfc1123") => Ok(moment.format("%a, %d %b %Y %H:%M:%S GMT").to_string()),
                Some("iso8601") => Ok(moment.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
                _ => Err(crate::i18n::tr("{{$datetime}} takes rfc1123 or iso8601, as {{$datetime iso8601}}.").to_string()),
            }
        }
        "randomInt" => {
            let (low, high) = match args {
                [low, high] => (low.parse::<i64>(), high.parse::<i64>()),
                _ => return Err(crate::i18n::tr("{{$randomInt}} takes a smallest and a largest number, as {{$randomInt 1 10}}.").to_string()),
            };
            let (Ok(low), Ok(high)) = (low, high) else { return Err(crate::i18n::tr("{{$randomInt}} takes two whole numbers, as {{$randomInt 1 10}}.").to_string()) };
            let (low, high) = (low.min(high), low.max(high));
            let span = (high - low) as u64 + 1;
            Ok((low + (random_u64() % span) as i64).to_string())
        }
        "processEnv" => {
            let variable = args.first().ok_or_else(|| crate::i18n::tr("{{$processEnv}} needs the name of a variable.").to_string())?;
            std::env::var(variable).map_err(|_| crate::i18n::tf!("The environment variable {variable} isn't set."))
        }
        "dotenv" => {
            let variable = args.first().ok_or_else(|| crate::i18n::tr("{{$dotenv}} needs the name of a variable.").to_string())?;
            lookup(scope.dotenv, variable).map(str::to_string).ok_or_else(|| crate::i18n::tf!("{variable} isn't in the project's .env file."))
        }
        _ => Err(crate::i18n::tf!("{{{{${name}}}}} isn't one of the built-in variables.")),
    }
}

fn response_value(name: &str, after: &str, scope: &Scope) -> Result<String, String> {
    let stored = scope.responses.get(name).ok_or_else(|| crate::i18n::tf!("{name} hasn't been sent yet: send it first (its line says # @name {name})."))?;
    if let Some(header) = after.strip_prefix("headers.") {
        return stored.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(header)).map(|(_, v)| v.clone()).ok_or_else(|| crate::i18n::tf!("{name}'s response had no {header} header."));
    }
    let Some(path) = after.strip_prefix("body.") else {
        return Err(crate::i18n::tf!("After {name}.response, put body. or headers. and then what to take."));
    };
    if path == "*" {
        return Ok(stored.body.clone());
    }
    let found = if path.starts_with('$') {
        serde_json::from_str::<serde_json::Value>(&stored.body).ok().and_then(|json| json_path(&json, path))
    } else {
        xml_path(&stored.body, path)
    };
    found.ok_or_else(|| crate::i18n::tf!("Nothing was found at {path} in {name}'s response."))
}

// ---------- JSONPath, and a little XPath ----------

/// The first value a JSONPath finds, as text: `$.a.b`, `$.list[0].id`, `$['key']`, `$.list[*].id`, `$..id`.
pub fn json_path(json: &serde_json::Value, path: &str) -> Option<String> {
    let mut now: Vec<&serde_json::Value> = vec![json];
    let mut rest = path.strip_prefix('$')?;
    while !rest.is_empty() {
        let mut next = Vec::new();
        if let Some(after) = rest.strip_prefix("..") {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = &after[..end];
            for value in &now {
                descend(value, key, &mut next);
            }
            rest = &after[end..];
        } else if let Some(after) = rest.strip_prefix('.') {
            let end = after.find(['.', '[']).unwrap_or(after.len());
            let key = &after[..end];
            for value in &now {
                match (key, value) {
                    ("*", serde_json::Value::Object(map)) => next.extend(map.values()),
                    ("*", serde_json::Value::Array(list)) => next.extend(list.iter()),
                    (key, serde_json::Value::Object(map)) => next.extend(map.get(key)),
                    _ => {}
                }
            }
            rest = &after[end..];
        } else {
            let after = rest.strip_prefix('[')?;
            let end = after.find(']')?;
            let inside = after[..end].trim();
            for value in &now {
                if inside == "*" {
                    match value {
                        serde_json::Value::Array(list) => next.extend(list.iter()),
                        serde_json::Value::Object(map) => next.extend(map.values()),
                        _ => {}
                    }
                } else if let Some(key) = inside.strip_prefix('\'').and_then(|k| k.strip_suffix('\'')).or_else(|| inside.strip_prefix('"').and_then(|k| k.strip_suffix('"'))) {
                    next.extend(value.get(key));
                } else if let Ok(index) = inside.parse::<i64>()
                    && let serde_json::Value::Array(list) = value
                {
                    let at = if index < 0 { list.len().checked_sub(index.unsigned_abs() as usize) } else { Some(index as usize) };
                    next.extend(at.and_then(|at| list.get(at)));
                }
            }
            rest = &after[end + 1..];
        }
        now = next;
    }
    now.first().map(|value| match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

fn descend<'a>(value: &'a serde_json::Value, key: &str, found: &mut Vec<&'a serde_json::Value>) {
    match value {
        serde_json::Value::Object(map) => {
            found.extend(map.get(key));
            for child in map.values() {
                descend(child, key, found);
            }
        }
        serde_json::Value::Array(list) => {
            for child in list {
                descend(child, key, found);
            }
        }
        _ => {}
    }
}

/// The text of the first element a simple XPath finds: `/a/b`, `//b`, `/a/b/@id`, `//b/text()`, `/a/b[2]`.
pub fn xml_path(xml: &str, path: &str) -> Option<String> {
    let document = roxmltree::Document::parse_with_options(xml, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).ok()?;
    let anywhere = path.starts_with("//");
    let steps: Vec<&str> = path.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let mut now: Vec<roxmltree::Node> = if anywhere { document.root().descendants().filter(|n| n.is_element()).collect() } else { vec![document.root()] };
    for (number, step) in steps.iter().enumerate() {
        if let Some(attribute) = step.strip_prefix('@') {
            return now.iter().find_map(|n| n.attribute(attribute)).map(str::to_string);
        }
        if *step == "text()" {
            return now.first().map(|n| n.text().unwrap_or("").to_string());
        }
        let (tag, index) = match step.split_once('[') {
            Some((tag, rest)) => (tag, rest.trim_end_matches(']').parse::<usize>().ok()),
            None => (*step, None),
        };
        // With // the first step's elements are already all of them: they only need to be the right name.
        let candidates: Vec<roxmltree::Node> = if anywhere && number == 0 { now.clone() } else { now.iter().flat_map(|n| n.children().filter(|c| c.is_element())).collect() };
        let mut matching: Vec<roxmltree::Node> = candidates.into_iter().filter(|n| tag == "*" || n.tag_name().name() == tag).collect();
        if let Some(index) = index {
            matching = matching.get(index.saturating_sub(1)).copied().into_iter().collect();
        }
        now = matching;
    }
    let node = now.first()?;
    Some(node.text().map_or_else(|| node.descendants().filter_map(|d| d.text()).collect::<String>(), str::to_string))
}

// ---------- Making a request ready ----------

/// A request with every variable filled in, ready to send.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub no_redirect: bool,
    pub no_cookie_jar: bool,
}

fn read_body_file(name: &str, scope: &Scope) -> Result<Vec<u8>, String> {
    let path = scope.folder.join(name);
    std::fs::read(&path).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", path.display()))
}

/// The body with `< ./file` lines replaced by the file, and `<@ ./file` by the file with its variables filled in.
fn body_bytes(body: &str, scope: &Scope, multipart: bool) -> Result<Vec<u8>, String> {
    let file_of = |line: &str| -> Option<(bool, String)> {
        let line = line.trim_end();
        if let Some(rest) = line.strip_prefix("<@") {
            // `<@latin1 ./file` names an encoding first, which is skipped: the file is read as UTF-8.
            let rest = rest.trim_start();
            let rest = rest.split_once(char::is_whitespace).filter(|(first, _)| !first.contains(['/', '\\', '.'])).map_or(rest, |(_, after)| after.trim_start());
            return Some((true, rest.to_string()));
        }
        let rest = line.strip_prefix("< ")?.trim();
        (!rest.is_empty()).then(|| (false, rest.to_string()))
    };
    let lines: Vec<&str> = body.split('\n').collect();
    if let [only] = lines.as_slice()
        && let Some((fill, name)) = file_of(only)
    {
        let bytes = read_body_file(&name, scope)?;
        return if fill { Ok(expand(&String::from_utf8_lossy(&bytes), scope)?.into_bytes()) } else { Ok(bytes) };
    }
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        match file_of(line) {
            Some((fill, name)) => {
                let text = String::from_utf8_lossy(&read_body_file(&name, scope)?).into_owned();
                out.push(if fill { expand(&text, scope)? } else { text });
            }
            None => out.push(expand(line, scope)?),
        }
    }
    // Multipart bodies are cut up at CRLFs, so a body typed with plain line ends is given them.
    let joined = out.join(if multipart { "\r\n" } else { "\n" });
    Ok(joined.into_bytes())
}

/// Fills in a request's variables and files.
pub fn resolve(request: &Request, scope: &Scope) -> Result<Resolved, String> {
    let mut url = expand(&request.url, scope)?;
    if !url.contains("://") {
        url = format!("http://{url}");
    }
    let mut headers = Vec::new();
    for (name, value) in &request.headers {
        let (name, mut value) = (expand(name, scope)?, expand(value, scope)?);
        // `Authorization: Basic user password` is the REST Client's way to write the encoded form.
        if name.eq_ignore_ascii_case("authorization")
            && let Some(rest) = value.strip_prefix("Basic ")
            && let Some((user, password)) = rest.trim().split_once(char::is_whitespace)
        {
            value = format!("Basic {}", crate::conversions::base64_encode(format!("{}:{}", user.trim(), password.trim()).as_bytes(), false));
        }
        headers.push((name, value));
    }
    let multipart = headers.iter().any(|(n, v)| n.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().starts_with("multipart/"));
    let body = if request.body.is_empty() { Vec::new() } else { body_bytes(&request.body, scope, multipart)? };
    Ok(Resolved { method: request.method.clone(), url, headers, body, no_redirect: request.no_redirect, no_cookie_jar: request.no_cookie_jar })
}

/// A `curl` command that sends the same request, to paste into a terminal.
pub fn to_curl(request: &Resolved) -> String {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
    let mut parts = vec!["curl".to_string()];
    if request.method != "GET" {
        parts.push(format!("-X {}", request.method));
    }
    if request.method == "HEAD" {
        parts.push("-I".to_string());
    }
    if !request.no_redirect {
        parts.push("-L".to_string());
    }
    parts.push(quote(&request.url));
    for (name, value) in &request.headers {
        parts.push(format!("-H {}", quote(&format!("{name}: {value}"))));
    }
    if !request.body.is_empty() {
        parts.push(format!("--data-binary {}", quote(&String::from_utf8_lossy(&request.body))));
    }
    parts.join(" \\\n  ")
}

// ---------- Environments ----------

/// The environments a project keeps: by `http-client.env.json` (JetBrains') and `.vscode/settings.json`'s
/// `rest-client.environmentVariables` (the REST Client's).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Environments {
    /// The variables every environment has (`$shared`).
    pub shared: Vec<(String, String)>,
    pub named: Vec<(String, Vec<(String, String)>)>,
}

impl Environments {
    pub fn names(&self) -> Vec<String> {
        self.named.iter().map(|(name, _)| name.clone()).collect()
    }

    /// The variables when `chosen` is the environment: the shared ones, then its own over them.
    pub fn variables(&self, chosen: Option<&str>) -> Vec<(String, String)> {
        let mut all = self.shared.clone();
        if let Some((_, own)) = chosen.and_then(|c| self.named.iter().find(|(name, _)| name == c)) {
            all.extend(own.iter().cloned());
        }
        all
    }

    fn add(&mut self, name: &str, values: Vec<(String, String)>) {
        if name == "$shared" {
            self.shared.extend(values);
        } else if let Some((_, existing)) = self.named.iter_mut().find(|(n, _)| n == name) {
            existing.extend(values);
        } else {
            self.named.push((name.to_string(), values));
        }
    }

    fn add_json(&mut self, json: &serde_json::Value) {
        let Some(map) = json.as_object() else { return };
        for (name, values) in map {
            let Some(values) = values.as_object() else { continue };
            let list = values
                .iter()
                .filter_map(|(key, value)| match value {
                    serde_json::Value::String(text) => Some((key.clone(), text.clone())),
                    serde_json::Value::Number(_) | serde_json::Value::Bool(_) => Some((key.clone(), value.to_string())),
                    _ => None,
                })
                .collect();
            self.add(name, list);
        }
    }
}

/// The environments for a `.http` file: looked for in its folder and those above it, up to the project's.
pub fn environments_for(file: &Path) -> Environments {
    let mut found = Environments::default();
    let Some(folder) = file.parent() else { return found };
    let root = crate::tasks::root_for(file);
    for dir in folder.ancestors().take(10) {
        for name in ["http-client.env.json", "http-client.private.env.json"] {
            if let Ok(text) = std::fs::read_to_string(dir.join(name))
                && let Ok(json) = crate::vsix_grammar::parse_jsonc(&text)
            {
                found.add_json(&json);
            }
        }
        if let Ok(text) = std::fs::read_to_string(dir.join(".vscode").join("settings.json"))
            && let Ok(json) = crate::vsix_grammar::parse_jsonc(&text)
            && let Some(variables) = json.get("rest-client.environmentVariables")
        {
            found.add_json(variables);
        }
        if dir == root || dir.join(".git").exists() {
            break;
        }
    }
    found
}

// ---------- Cookies ----------

#[derive(Clone, Debug, PartialEq)]
struct Cookie {
    name: String,
    value: String,
    domain: String,
    host_only: bool,
    path: String,
    secure: bool,
    /// Seconds since 1970, or None for one that lasts until the cookies are cleared.
    expires: Option<i64>,
}

/// The cookies the responses to a file's requests have set, sent back on the next requests to the same site.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CookieJar {
    cookies: Vec<Cookie>,
}

/// The host and path of an address, and whether it's https.
pub fn split_url(url: &str) -> (String, String, bool) {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host.starts_with('[') { host.split_once(']').map_or(host, |(h, _)| h).trim_start_matches('[') } else { host.split(':').next().unwrap_or(host) };
    let path = rest[end..].split(['?', '#']).next().unwrap_or("");
    (host.to_ascii_lowercase(), if path.is_empty() { "/".to_string() } else { path.to_string() }, scheme.eq_ignore_ascii_case("https"))
}

impl CookieJar {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }

    pub fn len(&self) -> usize {
        self.cookies.len()
    }

    #[cfg(test)]
    pub fn clear(&mut self) {
        self.cookies.clear();
    }

    /// Each cookie as "name=value (site)", for listing.
    pub fn described(&self) -> Vec<String> {
        self.cookies.iter().map(|c| format!("{}={} ({}{})", c.name, c.value, c.domain, c.path)).collect()
    }

    /// Keeps what a `Set-Cookie` header says, for a response from `url`. `now` is in seconds since 1970.
    pub fn store(&mut self, url: &str, set_cookie: &str, now: i64) {
        let (host, path, _) = split_url(url);
        let mut parts = set_cookie.split(';');
        let Some((name, value)) = parts.next().and_then(|first| first.split_once('=')) else { return };
        let mut cookie = Cookie { name: name.trim().to_string(), value: value.trim().to_string(), domain: host.clone(), host_only: true, path: default_path(&path), secure: false, expires: None };
        if cookie.name.is_empty() {
            return;
        }
        let mut max_age = None;
        for attribute in parts {
            let (key, value) = attribute.split_once('=').map_or((attribute.trim(), ""), |(k, v)| (k.trim(), v.trim()));
            match key.to_ascii_lowercase().as_str() {
                "domain" if !value.is_empty() => {
                    let domain = value.trim_start_matches('.').to_ascii_lowercase();
                    // A site can't set a cookie for another's domain.
                    if host == domain || host.ends_with(&format!(".{domain}")) {
                        cookie.domain = domain;
                        cookie.host_only = false;
                    } else {
                        return;
                    }
                }
                "path" if value.starts_with('/') => cookie.path = value.to_string(),
                "secure" => cookie.secure = true,
                "max-age" => max_age = value.parse::<i64>().ok(),
                "expires" => {
                    if let Ok(when) = chrono::DateTime::parse_from_rfc2822(value) {
                        cookie.expires = Some(when.timestamp());
                    }
                }
                _ => {}
            }
        }
        // Max-Age wins over Expires.
        if let Some(age) = max_age {
            cookie.expires = Some(now + age);
        }
        self.cookies.retain(|c| !(c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path));
        if cookie.expires.is_none_or(|when| when > now) {
            self.cookies.push(cookie);
        }
    }

    /// The `Cookie` header's value for a request to `url`, if any cookies belong.
    pub fn header_for(&self, url: &str, now: i64) -> Option<String> {
        let (host, path, https) = split_url(url);
        let mut matching: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| c.expires.is_none_or(|when| when > now))
            .filter(|c| if c.host_only { host == c.domain } else { host == c.domain || host.ends_with(&format!(".{}", c.domain)) })
            .filter(|c| path == c.path || path.starts_with(&c.path) && (c.path.ends_with('/') || path[c.path.len()..].starts_with('/')))
            .filter(|c| https || !c.secure)
            .collect();
        if matching.is_empty() {
            return None;
        }
        // The more specific paths first, as servers expect.
        matching.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        Some(matching.iter().map(|c| format!("{}={}", c.name, c.value)).collect::<Vec<_>>().join("; "))
    }
}

fn default_path(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(at) => path[..at].to_string(),
    }
}

// ---------- Laying a response's body out ----------

/// What a response's body is, which decides how it's shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Json,
    Xml,
    Html,
    Image,
    Text,
    /// Bytes that aren't text, and aren't a picture it can show.
    Binary,
}

/// What a body is, from its `Content-Type` and, failing that, its first bytes.
pub fn body_kind(content_type: &str, body: &[u8]) -> BodyKind {
    let kind = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let start = String::from_utf8_lossy(&body[..body.len().min(512)]).trim_start().to_string();
    if kind.starts_with("image/") || body.starts_with(b"\x89PNG") || body.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return BodyKind::Image;
    }
    if kind.contains("json") {
        return BodyKind::Json;
    }
    if kind == "text/html" || kind == "application/xhtml+xml" {
        return BodyKind::Html;
    }
    if kind.contains("xml") {
        return BodyKind::Xml;
    }
    // Control characters in what's meant to be text mean it isn't; text in an old character set has none.
    if body.iter().take(2048).any(|b| *b < 9 || (*b > 13 && *b < 32 && *b != 27)) {
        return BodyKind::Binary;
    }
    if kind.is_empty() || kind.starts_with("text/") {
        if start.starts_with('{') || start.starts_with('[') {
            return if serde_json::from_slice::<serde_json::Value>(body).is_ok() { BodyKind::Json } else { BodyKind::Text };
        }
        let lower = start.to_ascii_lowercase();
        if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
            return BodyKind::Html;
        }
        if lower.starts_with("<?xml") || (lower.starts_with('<') && kind.is_empty()) {
            return BodyKind::Xml;
        }
    }
    BodyKind::Text
}

/// JSON written out with each level indented, or None if it isn't JSON.
pub fn pretty_json(text: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

const VOID: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// XML or HTML with each element on its own line and each level indented. An element holding only text stays on one
/// line, and what's in `<script>`, `<style>` and `<pre>` is left as it is.
pub fn indent_markup(text: &str) -> String {
    #[derive(Debug)]
    enum Piece {
        Open(String, String),
        Close(String),
        Lone(String),
        Text(String),
    }
    let mut pieces = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('<') {
            let end = if inner.starts_with("!--") { inner.find("-->").map(|e| e + 3) } else if inner.starts_with("![CDATA[") { inner.find("]]>").map(|e| e + 3) } else { inner.find('>').map(|e| e + 1) };
            let Some(end) = end else {
                pieces.push(Piece::Text(rest.to_string()));
                break;
            };
            let tag = &rest[..end + 1];
            rest = &rest[end + 1..];
            let name: String = tag.trim_start_matches(['<', '/']).chars().take_while(|c| !c.is_whitespace() && *c != '>' && *c != '/').collect();
            let lower = name.to_ascii_lowercase();
            if tag.starts_with("<!") || tag.starts_with("<?") || tag.ends_with("/>") || VOID.contains(&lower.as_str()) && !tag.starts_with("</") {
                pieces.push(Piece::Lone(tag.to_string()));
            } else if tag.starts_with("</") {
                pieces.push(Piece::Close(name));
            } else {
                pieces.push(Piece::Open(name.clone(), tag.to_string()));
                if matches!(lower.as_str(), "script" | "style" | "pre") {
                    let closing = format!("</{lower}");
                    if let Some(at) = rest.to_ascii_lowercase().find(&closing)
                        && at > 0
                    {
                        pieces.push(Piece::Text(format!("\u{1}{}", &rest[..at])));
                        rest = &rest[at..];
                    }
                }
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let words = &rest[..end];
            rest = &rest[end..];
            if !words.trim().is_empty() {
                pieces.push(Piece::Text(words.trim().to_string()));
            }
        }
    }
    let mut out = String::new();
    let mut depth = 0usize;
    let mut at = 0;
    let line = |out: &mut String, depth: usize, words: &str| {
        out.push_str(&"  ".repeat(depth));
        out.push_str(words);
        out.push('\n');
    };
    while at < pieces.len() {
        match &pieces[at] {
            Piece::Open(name, tag) => {
                // An element with only text in it goes on one line.
                if let (Some(Piece::Text(words)), Some(Piece::Close(close))) = (pieces.get(at + 1), pieces.get(at + 2))
                    && close == name
                    && !words.starts_with('\u{1}')
                {
                    line(&mut out, depth, &format!("{tag}{words}</{close}>"));
                    at += 3;
                    continue;
                }
                if let Some(Piece::Close(close)) = pieces.get(at + 1)
                    && close == name
                {
                    line(&mut out, depth, &format!("{tag}</{close}>"));
                    at += 2;
                    continue;
                }
                line(&mut out, depth, tag);
                depth += 1;
            }
            Piece::Close(name) => {
                depth = depth.saturating_sub(1);
                line(&mut out, depth, &format!("</{name}>"));
            }
            Piece::Lone(tag) => line(&mut out, depth, tag),
            Piece::Text(words) => {
                for words in words.trim_start_matches('\u{1}').lines() {
                    line(&mut out, depth, words.trim_end());
                }
            }
        }
        at += 1;
    }
    out.trim_end().to_string()
}

/// What a stretch of a laid-out body is, for colouring it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Key,
    Text,
    Number,
    Keyword,
    Tag,
    Attribute,
    Comment,
}

/// Cuts laid-out JSON into stretches by what each is.
pub fn colour_json(text: &str) -> Vec<(String, Tone)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<(String, Tone)> = Vec::new();
    let push = |out: &mut Vec<(String, Tone)>, s: String, tone: Tone| match out.last_mut() {
        Some((last, t)) if *t == tone => last.push_str(&s),
        _ => out.push((s, tone)),
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '"' {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            let j = (j + 1).min(chars.len());
            let word: String = chars[i..j].iter().collect();
            // A string followed by a colon is a key.
            let mut after = j;
            while after < chars.len() && chars[after] == ' ' {
                after += 1;
            }
            push(&mut out, word, if chars.get(after) == Some(&':') { Tone::Key } else { Tone::Text });
            i = j;
        } else if c.is_ascii_digit() || c == '-' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_digit() || matches!(chars[j], '.' | 'e' | 'E' | '+' | '-')) {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), Tone::Number);
            i = j;
        } else if c.is_ascii_alphabetic() {
            let mut j = i;
            while j < chars.len() && chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            push(&mut out, chars[i..j].iter().collect(), Tone::Keyword);
            i = j;
        } else {
            push(&mut out, c.to_string(), Tone::Plain);
            i += 1;
        }
    }
    out
}

/// Cuts laid-out XML or HTML into stretches by what each is.
pub fn colour_markup(text: &str) -> Vec<(String, Tone)> {
    let mut out: Vec<(String, Tone)> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map_or(rest.len(), |e| e + 3);
            out.push((rest[..end].to_string(), Tone::Comment));
            rest = &rest[end..];
        } else if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |e| e + 1);
            let tag = &rest[..end];
            rest = &rest[end..];
            let name_end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len().saturating_sub(usize::from(tag.ends_with('>'))));
            out.push((tag[..name_end].to_string(), Tone::Tag));
            let inside = &tag[name_end..];
            let mut inside_rest = inside;
            while let Some(eq) = inside_rest.find('=') {
                let name_start = inside_rest[..eq].rfind(char::is_whitespace).map_or(0, |s| s + 1);
                out.push((inside_rest[..name_start].to_string(), Tone::Plain));
                out.push((inside_rest[name_start..eq].to_string(), Tone::Attribute));
                let after = &inside_rest[eq..];
                let value_end = match after.as_bytes().get(1) {
                    Some(b'"') | Some(b'\'') => after[2..].find(after.as_bytes()[1] as char).map_or(after.len(), |e| e + 3),
                    _ => after.find(char::is_whitespace).unwrap_or(after.len()),
                };
                out.push(("=".to_string(), Tone::Plain));
                out.push((after[1..value_end].to_string(), Tone::Text));
                inside_rest = &after[value_end..];
            }
            if let Some(closing) = inside_rest.strip_suffix('>') {
                out.push((closing.to_string(), Tone::Plain));
                out.push((">".to_string(), Tone::Tag));
            } else {
                out.push((inside_rest.to_string(), Tone::Plain));
            }
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push((rest[..end].to_string(), Tone::Plain));
            rest = &rest[end..];
        }
    }
    out.retain(|(s, _)| !s.is_empty());
    out
}

/// The folder a request's `< ./file` is found in, from the `.http` file's path.
pub fn folder_of(file: Option<&Path>) -> PathBuf {
    file.and_then(|f| f.parent()).map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@host = localhost:8080
@token = abc

### List the users
# @name users
GET http://{{host}}/users?page=1
    &limit=10 HTTP/1.1
Accept: application/json
// a comment between the headers
Authorization: Bearer {{token}}

###

POST http://{{host}}/login
Content-Type: application/json

{
  \"user\": \"ann\",

  \"password\": \"{{$processEnv HOME}}\"
}


### no method at all
https://example.com/a
";

    fn empty_scope<'a>(file: &'a [(String, String)], responses: &'a HashMap<String, Stored>) -> Scope<'a> {
        Scope { file, environment: &[], dotenv: &[], responses, folder: Path::new(".") }
    }

    #[test]
    fn requests_are_cut_at_triple_hashes_with_their_parts() {
        let file = parse(SAMPLE);
        assert_eq!(file.variables, [("host".to_string(), "localhost:8080".to_string()), ("token".to_string(), "abc".to_string())]);
        assert_eq!(file.requests.len(), 3);
        let first = &file.requests[0];
        assert_eq!(first.name.as_deref(), Some("users"));
        assert_eq!(first.method, "GET");
        assert_eq!(first.url, "http://{{host}}/users?page=1&limit=10", "the continued address is joined, and the version dropped");
        assert_eq!(first.headers, [("Accept".to_string(), "application/json".to_string()), ("Authorization".to_string(), "Bearer {{token}}".to_string())]);
        assert!(first.body.is_empty());
        assert_eq!(first.line, 5);
        let second = &file.requests[1];
        assert_eq!((second.method.as_str(), second.name.as_deref()), ("POST", None));
        assert!(second.body.starts_with("{\n  \"user\""), "{}", second.body);
        assert!(second.body.ends_with("}"), "blank lines after the body are not part of it");
        assert!(second.body.contains(",\n\n  \"password\""), "blank lines inside it are kept");
        assert_eq!((file.requests[2].method.as_str(), file.requests[2].url.as_str()), ("GET", "https://example.com/a"));
    }

    #[test]
    fn rest_clients_own_documented_examples_read() {
        let file = parse("GET https://example.com/comments/1 HTTP/1.1\n\n###\n\nGET https://example.com/topics/1\nAccept-Language: en-GB\n\n###\nPOST https://example.com/comments HTTP/1.1\ncontent-type: application/xml\nauthorization: token xxx\n\n<request>\n    <name>sample</name>\n    <time>Wed, 21 Oct 2015 18:27:50 GMT</time>\n</request>\n");
        assert_eq!(file.requests.len(), 3);
        assert_eq!(file.requests[0].url, "https://example.com/comments/1");
        assert_eq!(file.requests[1].headers[0].0, "Accept-Language");
        assert_eq!(file.requests[2].headers.len(), 2);
        assert_eq!(file.requests[2].body.lines().count(), 4);
        // # @name and // @name both name a request, and the marks that tune it.
        let file = parse("// @name second\n# @no-redirect\n# @no-cookie-jar\nGET http://a/b\n");
        assert_eq!(file.requests[0].name.as_deref(), Some("second"));
        assert!(file.requests[0].no_redirect && file.requests[0].no_cookie_jar);
        // The text after ### is a title, and a block with no request is no request.
        let file = parse("### Only a title\n@x = 1\n###\n");
        assert!(file.requests.is_empty());
        assert_eq!(file.variables.len(), 1);
    }

    #[test]
    fn file_variables_nest_and_the_last_one_wins() {
        let vars = vec![("host".to_string(), "h:1".to_string()), ("base".to_string(), "http://{{host}}/api".to_string()), ("host".to_string(), "h:2".to_string())];
        let responses = HashMap::new();
        let scope = empty_scope(&vars, &responses);
        assert_eq!(expand("{{base}}/x {{ host }}", &scope).unwrap(), "http://h:2/api/x h:2");
        assert!(expand("{{nothing}}", &scope).unwrap_err().contains("nothing"));
        let circle = vec![("a".to_string(), "{{b}}".to_string()), ("b".to_string(), "{{a}}".to_string())];
        assert!(expand("{{a}}", &empty_scope(&circle, &responses)).is_err());
        // An environment's variables come after the file's.
        let env = vec![("region".to_string(), "eu".to_string())];
        let with_env = Scope { environment: &env, ..empty_scope(&vars, &responses) };
        assert_eq!(expand("{{region}}", &with_env).unwrap(), "eu");
    }

    #[test]
    fn built_in_variables_make_ids_numbers_and_times() {
        let (vars, responses) = (Vec::new(), HashMap::new());
        let dotenv = vec![("API_KEY".to_string(), "s3cret".to_string())];
        let scope = Scope { dotenv: &dotenv, ..empty_scope(&vars, &responses) };
        let id = expand("{{$guid}}", &scope).unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[14], b'4', "a version 4 id");
        assert_ne!(id, expand("{{$guid}}", &scope).unwrap());
        for _ in 0..50 {
            let n: i64 = expand("{{$randomInt 5 8}}", &scope).unwrap().parse().unwrap();
            assert!((5..=8).contains(&n));
        }
        let now: i64 = expand("{{$timestamp}}", &scope).unwrap().parse().unwrap();
        let earlier: i64 = expand("{{$timestamp -1 d}}", &scope).unwrap().parse().unwrap();
        assert!((now - earlier - 86_400).abs() <= 2);
        assert_eq!(expand("{{$dotenv API_KEY}}", &scope).unwrap(), "s3cret");
        assert!(expand("{{$dotenv MISSING}}", &scope).is_err());
        assert!(expand("{{$processEnv PATH}}", &scope).is_ok());
        assert!(expand("{{$datetime iso8601}}", &scope).unwrap().ends_with('Z'));
        assert!(expand("{{$datetime rfc1123}}", &scope).unwrap().ends_with("GMT"));
        assert!(expand("{{$nonsense}}", &scope).is_err());
        assert!(expand("{{$randomInt}}", &scope).is_err());
    }

    fn stored() -> HashMap<String, Stored> {
        let mut responses = HashMap::new();
        responses.insert(
            "login".to_string(),
            Stored { status: 200, headers: vec![("X-Request-Id".to_string(), "r-9".to_string())], body: r#"{"token": "t0k", "user": {"id": 7, "tags": ["a", "b"]}, "list": [{"id": 1}, {"id": 2}]}"#.to_string() },
        );
        responses.insert("page".to_string(), Stored { status: 200, headers: Vec::new(), body: "<root><item id=\"3\"><name>first</name></item><item id=\"4\"><name>second</name></item></root>".to_string() });
        responses
    }

    #[test]
    fn a_named_requests_response_gives_values_by_jsonpath_xpath_and_header() {
        let (vars, responses) = (Vec::new(), stored());
        let scope = empty_scope(&vars, &responses);
        let get = |text: &str| expand(text, &scope);
        assert_eq!(get("{{login.response.body.$.token}}").unwrap(), "t0k");
        assert_eq!(get("{{login.response.body.$.user.id}}").unwrap(), "7");
        assert_eq!(get("{{login.response.body.$.user.tags[1]}}").unwrap(), "b");
        assert_eq!(get("{{login.response.body.$.list[-1].id}}").unwrap(), "2");
        assert_eq!(get("{{login.response.body.$['user']['id']}}").unwrap(), "7");
        assert_eq!(get("{{login.response.body.$..id}}").unwrap(), "7");
        assert_eq!(get("{{login.response.body.$.list[*].id}}").unwrap(), "1");
        assert_eq!(get("{{login.response.headers.x-request-id}}").unwrap(), "r-9");
        assert!(get("{{login.response.body.*}}").unwrap().contains("t0k"));
        assert_eq!(get("{{page.response.body.//item/name}}").unwrap(), "first");
        assert_eq!(get("{{page.response.body./root/item[2]/@id}}").unwrap(), "4");
        assert_eq!(get("{{page.response.body.//name/text()}}").unwrap(), "first");
        assert!(get("{{login.response.body.$.nope}}").unwrap_err().contains("nope"));
        assert!(get("{{later.response.body.$.x}}").unwrap_err().contains("later"));
        assert!(get("{{login.response.headers.Nope}}").is_err());
    }

    #[test]
    fn resolving_fills_the_address_headers_and_body() {
        let vars = vec![("host".to_string(), "localhost:8080".to_string()), ("user".to_string(), "ann".to_string())];
        let responses = HashMap::new();
        let scope = empty_scope(&vars, &responses);
        let file = parse("POST {{host}}/x\nAuthorization: Basic ann secret\nContent-Type: application/json\n\n{\"u\": \"{{user}}\"}");
        let resolved = resolve(&file.requests[0], &scope).unwrap();
        assert_eq!(resolved.url, "http://localhost:8080/x", "a bare host gets http://");
        assert_eq!(resolved.headers[0].1, "Basic YW5uOnNlY3JldA==", "user and password are encoded");
        assert_eq!(String::from_utf8(resolved.body).unwrap(), "{\"u\": \"ann\"}");
    }

    #[test]
    fn a_body_can_come_from_a_file_and_a_multipart_one_gets_crlfs() {
        let folder = std::env::temp_dir().join(format!("limoniter-http-body-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("data.json"), "{\"a\": \"{{who}}\"}").unwrap();
        std::fs::write(folder.join("blob.bin"), [0u8, 1, 255, 254]).unwrap();
        let vars = vec![("who".to_string(), "me".to_string())];
        let responses = HashMap::new();
        let scope = Scope { folder: &folder, ..empty_scope(&vars, &responses) };
        let file = parse("POST http://a/b\n\n< ./data.json\n###\nPOST http://a/b\n\n<@ ./data.json\n###\nPOST http://a/b\n\n< ./blob.bin\n###\nPOST http://a/c\nContent-Type: multipart/form-data; boundary=X\n\n--X\nContent-Disposition: form-data; name=\"f\"\n\n< ./data.json\n--X--\n");
        assert_eq!(resolve(&file.requests[0], &scope).unwrap().body, b"{\"a\": \"{{who}}\"}", "< takes the file as it is");
        assert_eq!(resolve(&file.requests[1], &scope).unwrap().body, b"{\"a\": \"me\"}", "<@ fills its variables in");
        assert_eq!(resolve(&file.requests[2], &scope).unwrap().body, [0, 1, 255, 254], "binary files are sent as they are");
        let multipart = String::from_utf8(resolve(&file.requests[3], &scope).unwrap().body).unwrap();
        assert!(multipart.contains("\r\n\r\n{\"a\""), "{multipart:?}");
        assert!(multipart.ends_with("--X--"));
        let missing = parse("POST http://a\n\n< ./nope.txt");
        assert!(resolve(&missing.requests[0], &scope).unwrap_err().contains("nope.txt"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn curl_quotes_what_it_must() {
        let resolved = Resolved { method: "POST".into(), url: "http://a/b?c=1".into(), headers: vec![("Content-Type".into(), "text/plain".into())], body: b"it's".to_vec(), ..Resolved::default() };
        let curl = to_curl(&resolved);
        assert!(curl.starts_with("curl \\\n  -X POST"));
        assert!(curl.contains("-H 'Content-Type: text/plain'"));
        assert!(curl.contains("--data-binary 'it'\\''s'"), "{curl}");
        assert!(!to_curl(&Resolved { method: "GET".into(), url: "http://a".into(), no_redirect: true, ..Resolved::default() }).contains("-L"));
    }

    #[test]
    fn environments_come_from_the_jetbrains_file_and_the_vscode_settings() {
        let folder = std::env::temp_dir().join(format!("limoniter-http-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join(".vscode")).unwrap();
        std::fs::create_dir_all(folder.join("api")).unwrap();
        std::fs::write(folder.join(".git"), "").unwrap();
        std::fs::write(folder.join("http-client.env.json"), r#"{"$shared": {"version": "v1"}, "local": {"host": "localhost:8080", "port": 8080, "debug": true}, "staging": {"host": "staging.example.com"}}"#).unwrap();
        std::fs::write(folder.join("http-client.private.env.json"), r#"{"local": {"token": "mine"}}"#).unwrap();
        std::fs::write(folder.join(".vscode").join("settings.json"), "{\n // the REST Client's own\n \"rest-client.environmentVariables\": {\"$shared\": {\"agent\": \"limoniter\"}, \"prod\": {\"host\": \"example.com\"}}\n}").unwrap();
        let environments = environments_for(&folder.join("api").join("calls.http"));
        let mut names = environments.names();
        names.sort();
        assert_eq!(names, ["local", "prod", "staging"]);
        let local = environments.variables(Some("local"));
        assert_eq!(lookup(&local, "host"), Some("localhost:8080"));
        assert_eq!(lookup(&local, "port"), Some("8080"));
        assert_eq!(lookup(&local, "debug"), Some("true"));
        assert_eq!(lookup(&local, "token"), Some("mine"), "the private file adds to the same environment");
        assert_eq!(lookup(&local, "version"), Some("v1"));
        assert_eq!(lookup(&local, "agent"), Some("limoniter"), "$shared of both files");
        assert_eq!(lookup(&environments.variables(Some("prod")), "host"), Some("example.com"));
        assert_eq!(lookup(&environments.variables(None), "host"), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn cookies_are_kept_for_their_site_and_path_until_they_expire() {
        let mut jar = CookieJar::default();
        jar.store("http://example.com/app/login", "sid=abc; Path=/app; HttpOnly", 1000);
        jar.store("http://example.com/", "theme=dark; Max-Age=60", 1000);
        jar.store("https://example.com/", "tls=1; Secure; Domain=example.com", 1000);
        jar.store("http://example.com/", "evil=1; Domain=other.com", 1000);
        assert_eq!(jar.len(), 3, "another site's domain is refused");
        assert_eq!(jar.header_for("http://example.com/app/x", 1010).unwrap(), "sid=abc; theme=dark", "the longer path first, and no secure cookie over http");
        assert_eq!(jar.header_for("http://example.com/other", 1010).unwrap(), "theme=dark", "the /app cookie doesn't go to /other");
        assert!(jar.header_for("https://example.com/other", 1010).unwrap().contains("tls=1"));
        assert!(jar.header_for("http://sub.example.com/", 1010).is_none_or(|h| !h.contains("theme")));
        assert!(jar.header_for("http://example.com/other", 1100).is_none(), "the cookie's minute is over");
        // A new value replaces the old, and Max-Age=0 deletes.
        jar.store("http://example.com/app/", "sid=def; Path=/app", 1000);
        assert_eq!(jar.header_for("http://example.com/app/", 1000).unwrap(), "sid=def; theme=dark");
        jar.store("http://example.com/app/", "sid=; Path=/app; Max-Age=0", 1000);
        assert_eq!(jar.header_for("http://example.com/app/", 1000).unwrap(), "theme=dark");
        jar.clear();
        assert!(jar.is_empty());
    }

    #[test]
    fn bodies_are_told_apart_and_laid_out() {
        assert_eq!(body_kind("application/json; charset=utf-8", b"{}"), BodyKind::Json);
        assert_eq!(body_kind("application/vnd.api+json", b"{}"), BodyKind::Json);
        assert_eq!(body_kind("", b"{\"a\": 1}"), BodyKind::Json);
        assert_eq!(body_kind("text/html", b"<p>"), BodyKind::Html);
        assert_eq!(body_kind("application/xml", b"<a/>"), BodyKind::Xml);
        assert_eq!(body_kind("image/png", b"\x89PNG"), BodyKind::Image);
        assert_eq!(body_kind("application/octet-stream", &[0, 1, 2, 255]), BodyKind::Binary);
        assert_eq!(body_kind("text/plain", b"hello"), BodyKind::Text);
        assert_eq!(pretty_json("{\"a\":[1,2],\"b\":{\"c\":null}}").unwrap(), "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": {\n    \"c\": null\n  }\n}");
        assert!(pretty_json("not json").is_none());
        let xml = indent_markup("<?xml version=\"1.0\"?><a><b>text</b><c><d/></c><e></e></a>");
        assert_eq!(xml, "<?xml version=\"1.0\"?>\n<a>\n  <b>text</b>\n  <c>\n    <d/>\n  </c>\n  <e></e>\n</a>");
        let html = indent_markup("<!DOCTYPE html><html><body><p>Hi<br>there</p><script>if (a<b) {}</script></body></html>");
        assert!(html.contains("<body>\n    <p>"), "{html}");
        assert!(html.contains("<br>"), "void elements don't open a level");
        assert!(html.contains("      if (a<b) {}\n"), "script is left alone: {html}");
        assert!(html.ends_with("</body>\n</html>"), "{html}");
    }

    #[test]
    fn colouring_keeps_all_the_text() {
        let json = pretty_json("{\"name\": \"x\", \"n\": -15, \"ok\": true, \"none\": null}").unwrap();
        let spans = colour_json(&json);
        assert_eq!(spans.iter().map(|(s, _)| s.as_str()).collect::<String>(), json);
        assert!(spans.contains(&("\"name\"".to_string(), Tone::Key)));
        assert!(spans.contains(&("\"x\"".to_string(), Tone::Text)));
        assert!(spans.contains(&("-15".to_string(), Tone::Number)));
        assert!(spans.contains(&("true".to_string(), Tone::Keyword)));
        let markup = "<a id=\"1\" class='b'>\n  <!-- hi -->\n  <b>text</b>\n</a>";
        let spans = colour_markup(markup);
        assert_eq!(spans.iter().map(|(s, _)| s.as_str()).collect::<String>(), markup);
        assert!(spans.contains(&("id".to_string(), Tone::Attribute)));
        assert!(spans.contains(&("\"1\"".to_string(), Tone::Text)));
        assert!(spans.contains(&("<!-- hi -->".to_string(), Tone::Comment)));
    }

    #[test]
    fn splitting_an_address() {
        assert_eq!(split_url("https://User@Example.com:8443/a/b?x=1#f"), ("example.com".to_string(), "/a/b".to_string(), true));
        assert_eq!(split_url("http://localhost:8080"), ("localhost".to_string(), "/".to_string(), false));
    }

    #[test]
    fn the_request_at_a_line_is_the_one_whose_block_it_is_in() {
        let file = parse(SAMPLE);
        // Lines: 0-1 variables, 3 is ### (belongs to the block after it), the first request spans 4 to 11.
        assert_eq!(request_at(&file.requests, 5), Some(0));
        assert_eq!(request_at(&file.requests, 0), Some(0), "above them all is the first");
        let last = file.requests[2].line;
        assert_eq!(request_at(&file.requests, last), Some(2));
        let second = &file.requests[1];
        assert_eq!(request_at(&file.requests, second.line + 2), Some(1));
        assert_eq!(request_at(&[], 3), None);
    }

    #[test]
    fn names_of_http_files() {
        assert!(is_http_name(Path::new("a/calls.http")));
        assert!(is_http_name(Path::new("CALLS.REST")));
        assert!(!is_http_name(Path::new("calls.txt")));
    }
}
