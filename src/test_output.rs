//! Reading what more test runners print, test by test: Jest's and Vitest's verbose lists, PHPUnit's TeamCity
//! lines, testthat's location reporter, and the extras the older readers in `tasks.rs` pick up (the times pytest
//! and Go give, what a failed assertion expected and what came). Each reader looks only for its own runner's
//! lines, so the output of any runner can be given to all of them.

use crate::tasks::TestResult;

/// The place a failure names in a stack line such as `at Object.<anonymous> (tests/sum.test.js:5:19)`, `at
/// tests/sum.test.js:5:19` or `❯ tests/sum.test.ts:5:23`: the file and the line.
pub fn stack_place(line: &str) -> Option<(String, usize)> {
    let t = line.trim();
    let inner = if let Some(rest) = t.strip_prefix("at ") {
        match (rest.rfind('('), rest.rfind(')')) {
            (Some(open), Some(close)) if open < close => &rest[open + 1..close],
            _ => rest,
        }
    } else {
        t.strip_prefix('\u{276F}')?.trim()
    };
    let mut parts = inner.rsplitn(3, ':');
    let _column = parts.next()?.trim().parse::<usize>().ok()?;
    let number = parts.next()?.trim().parse::<usize>().ok()?;
    let file = parts.next()?.trim();
    (!file.is_empty() && !file.contains(' ') || file.contains('/') || file.contains('\\')).then(|| (file.trim_start_matches("file://").to_string(), number))
}

/// A time such as `3ms`, `(3 ms)` or `0.5s`, as a short phrase, if the end of the line has one.
fn trailing_time(line: &str) -> (&str, Option<String>) {
    let t = line.trim_end();
    let (body, bracketed) = match t.strip_suffix(')') {
        Some(rest) => match rest.rfind('(') {
            Some(open) => (&rest[open + 1..], Some(&rest[..open])),
            None => (t, None),
        },
        None => (t, None),
    };
    let candidate = bracketed.map_or_else(|| t.rsplit_once(' ').map(|(_, last)| last), |_| Some(body)).unwrap_or("");
    let digits: String = candidate.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let unit = candidate.trim()[digits.len()..].trim();
    if digits.is_empty() || !matches!(unit, "ms" | "s") {
        return (t, None);
    }
    let cut = match bracketed {
        Some(before) => before.trim_end(),
        None => t[..t.len() - candidate.len()].trim_end(),
    };
    (cut, Some(format!("{digits} {unit}")))
}

/// Jest's `--verbose` list: `✓ name (3 ms)` and `✕ name (4 ms)`, and the `● suite › name` blocks that say why.
fn jest(lines: &[String]) -> Vec<TestResult> {
    // Jest names a file after PASS or FAIL; Vitest's FAIL is followed by a test's name, with " > " in it.
    if !lines.iter().any(|l| (l.trim_start().starts_with("PASS ") || l.trim_start().starts_with("FAIL ")) && !l.contains(" > ")) {
        return Vec::new();
    }
    let mut results: Vec<TestResult> = Vec::new();
    for line in lines {
        let t = line.trim();
        let Some((passed, rest)) = t.strip_prefix('\u{2713}').map(|r| (true, r)).or_else(|| t.strip_prefix('\u{2715}').map(|r| (false, r))).or_else(|| t.strip_prefix('\u{2717}').map(|r| (false, r))) else {
            continue;
        };
        if rest.contains(" > ") {
            continue;
        }
        let (name, time) = trailing_time(rest.trim());
        results.push(TestResult { name: name.trim().to_string(), passed, note: time, ..Default::default() });
    }
    // "  ● sum › subtracts": the failure's own block, with Expected and Received and a stack.
    let mut i = 0;
    while i < lines.len() {
        if let Some(title) = lines[i].trim().strip_prefix('\u{25CF}').filter(|t| !t.trim().is_empty() && !t.contains("Console")) {
            let name = title.rsplit('\u{203A}').next().unwrap_or("").trim();
            let end = (i + 1..lines.len()).find(|&j| lines[j].trim().starts_with('\u{25CF}')).unwrap_or(lines.len());
            let block = &lines[i + 1..end];
            if let Some(result) = results.iter_mut().find(|r| !r.passed && r.name == name) {
                for l in block {
                    let w = l.trim();
                    if (w.starts_with("Expected") || w.starts_with("Received")) && result.detail.len() < 4 {
                        result.detail.push(w.to_string());
                    }
                }
                if result.detail.is_empty()
                    && let Some(first) = block.iter().map(|l| l.trim()).find(|l| !l.is_empty())
                {
                    result.detail.push(first.to_string());
                }
                result.place = block.iter().find_map(|l| stack_place(l));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    results
}

/// Vitest's verbose reporter: ` ✓ file > suite > name 3ms` and ` × file > suite > name 2ms`, and the failure
/// blocks that follow, headed ` FAIL  file > suite > name`.
fn vitest(lines: &[String]) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    for line in lines {
        let t = line.trim();
        let Some((passed, rest)) = t.strip_prefix('\u{2713}').map(|r| (true, r)).or_else(|| t.strip_prefix('\u{00D7}').map(|r| (false, r))).or_else(|| t.strip_prefix('\u{2717}').map(|r| (false, r))) else {
            continue;
        };
        if !rest.contains(" > ") {
            continue;
        }
        let (name, time) = trailing_time(rest.trim());
        results.push(TestResult { name: name.trim().to_string(), passed, note: time, ..Default::default() });
    }
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line.trim().strip_prefix("FAIL ").map(str::trim).filter(|n| n.contains(" > ")) else {
            continue;
        };
        let end = (i + 1..lines.len()).find(|&j| lines[j].trim().starts_with("FAIL ")).unwrap_or(lines.len());
        let block = &lines[i + 1..end];
        if let Some(result) = results.iter_mut().find(|r| !r.passed && r.name == name) {
            if let Some(first) = block.iter().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with('\u{23AF}')) {
                result.detail.push(first.to_string());
            }
            result.place = block.iter().find_map(|l| stack_place(l));
        }
    }
    results
}

/// What a TeamCity line says: its message's name, and its `key='value'` pairs with the escapes (`|n`, `|'`, `||`,
/// `|[`, `|]`) undone.
fn teamcity_line(line: &str) -> Option<(String, Vec<(String, String)>)> {
    let rest = line.trim().strip_prefix("##teamcity[")?.strip_suffix(']')?;
    let (message, mut rest) = rest.split_once(' ')?;
    let mut pairs = Vec::new();
    while let Some((key, after)) = rest.trim_start().split_once("='") {
        let mut value = String::new();
        let mut chars = after.char_indices();
        let mut end = after.len();
        while let Some((at, c)) = chars.next() {
            match c {
                '|' => match chars.next().map(|(_, e)| e) {
                    Some('n') => value.push('\n'),
                    Some('r') => value.push('\r'),
                    Some(other) => value.push(other),
                    None => {}
                },
                '\'' => {
                    end = at + 1;
                    break;
                }
                c => value.push(c),
            }
        }
        pairs.push((key.trim().to_string(), value));
        rest = &after[end..];
    }
    Some((message.to_string(), pairs))
}

/// PHPUnit's `--teamcity` lines: each test started, failed and finished, with the time it took and, for a failed
/// comparison, what was expected and what came.
fn phpunit(lines: &[String]) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    let mut names: std::collections::HashMap<String, (String, Option<String>)> = std::collections::HashMap::new();
    for line in lines {
        let Some((message, pairs)) = teamcity_line(line) else { continue };
        let get = |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        let Some(raw) = get("name") else { continue };
        match message.as_str() {
            "testStarted" => {
                // The location hint has the file and the class: php_qn:///work/FooTest.php::\Tests\FooTest::testAdd
                let (name, file) = match get("locationHint").and_then(|h| h.strip_prefix("php_qn://")) {
                    Some(hint) => match hint.split_once("::") {
                        Some((file, rest)) => (rest.trim_start_matches('\\').to_string(), Some(file.to_string())),
                        None => (raw.to_string(), None),
                    },
                    None => (raw.to_string(), None),
                };
                names.insert(raw.to_string(), (name, file));
            }
            "testFailed" => {
                let (name, file) = names.get(raw).cloned().unwrap_or((raw.to_string(), None));
                let mut detail = vec![get("message").unwrap_or("").lines().next().unwrap_or("").to_string()];
                if let (Some(expected), Some(actual)) = (get("expected"), get("actual")) {
                    detail = vec![crate::i18n::tf!("Expected: {expected}"), crate::i18n::tf!("Got: {actual}")];
                }
                // The details end with the file and line the assertion was made at.
                let place = get("details").and_then(|d| d.lines().rev().find_map(|l| crate::tasks::place_in(l.trim()))).map(|(f, n, _)| (f, n)).or_else(|| file.map(|f| (f, 1)));
                results.retain(|r| r.name != name);
                results.push(TestResult { name, passed: false, place, detail, ..Default::default() });
            }
            "testFinished" => {
                let (name, _) = names.get(raw).cloned().unwrap_or((raw.to_string(), None));
                let time = get("duration").map(|ms| format!("{ms} ms"));
                match results.iter_mut().find(|r| r.name == name) {
                    Some(failed) => failed.note = time,
                    None => results.push(TestResult { name, passed: true, note: time, ..Default::default() }),
                }
            }
            _ => {}
        }
    }
    results
}

/// testthat's location reporter: `Start test: name`, then a `file:line:column [success]` line for each
/// expectation, then `End test: name`.
fn testthat(lines: &[String]) -> Vec<TestResult> {
    let mut results: Vec<TestResult> = Vec::new();
    let mut current: Option<TestResult> = None;
    for line in lines {
        let t = line.trim();
        if let Some(name) = t.strip_prefix("Start test: ") {
            current = Some(TestResult { name: name.trim().to_string(), passed: true, ..Default::default() });
        } else if t.starts_with("End test: ") {
            results.extend(current.take());
        } else if let Some(test) = current.as_mut()
            && let Some((place, outcome)) = t.rsplit_once(" [")
        {
            let outcome = outcome.trim_end_matches(']');
            if matches!(outcome, "failure" | "error") && test.passed {
                test.passed = false;
                let mut parts = place.rsplitn(3, ':');
                let _column = parts.next();
                if let (Some(number), Some(file)) = (parts.next().and_then(|n| n.parse().ok()), parts.next()) {
                    // The reporter names the file without its folder.
                    let file = if file.contains('/') { file.to_string() } else { format!("tests/testthat/{file}") };
                    test.place = Some((file, number));
                }
                test.detail.push(crate::i18n::tf!("An expectation failed here ({outcome})."));
            }
        }
    }
    results
}

/// pytest's `--durations=0` lines (`0.01s call tests/test_x.py::test_add`) give the times.
fn pytest_times(lines: &[String], results: &mut [TestResult]) {
    for line in lines {
        let mut words = line.split_whitespace();
        let (Some(time), Some("call"), Some(name)) = (words.next().and_then(|t| t.strip_suffix('s')).filter(|t| t.parse::<f64>().is_ok()), words.next(), words.next()) else {
            continue;
        };
        if let Some(result) = results.iter_mut().find(|r| r.name == name && r.note.is_none()) {
            result.note = Some(format!("{time} s"));
        }
    }
}

/// What a failed pytest assertion said: the `E   assert 0 == 1` lines under the test's own heading.
fn pytest_details(lines: &[String], results: &mut [TestResult]) {
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = line.trim().strip_prefix("____").map(|s| s.trim_matches(|c| c == '_' || c == ' ')).filter(|n| !n.is_empty()) else {
            continue;
        };
        let end = (i + 1..lines.len()).find(|&j| lines[j].trim().starts_with("____") || lines[j].trim().starts_with("====")).unwrap_or(lines.len());
        let said: Vec<String> = lines[i + 1..end].iter().filter_map(|l| l.strip_prefix("E ")).map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).take(4).collect();
        if let Some(result) = results.iter_mut().find(|r| !r.passed && r.detail.is_empty() && r.name.ends_with(&format!("::{name}"))) {
            result.detail = said;
        }
    }
}

/// What a Go test printed under its own lines (`maths_test.go:14: got 0, want 1`), and its time.
fn go_extras(lines: &[String], results: &mut [TestResult]) {
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("--- PASS: ").or_else(|| t.strip_prefix("--- FAIL: ")) else { continue };
        let mut words = rest.split_whitespace();
        let (Some(name), Some(time)) = (words.next(), words.next().map(|t| t.trim_matches(|c| c == '(' || c == ')'))) else { continue };
        let Some(result) = results.iter_mut().find(|r| r.name == name) else { continue };
        result.note = time.ends_with('s').then(|| time.replace('s', " s").trim().to_string());
        if !result.passed
            && result.detail.is_empty()
            && let Some((_, _, said)) = lines[..i].iter().rev().take(20).find_map(|l| crate::tasks::place_in(l).filter(|(f, _, _)| f.ends_with("_test.go")))
            && !said.is_empty()
        {
            result.detail.push(said);
        }
    }
}

/// What a Rust panic said, under its test: `left: 0` and `right: 1` for `assert_eq!`, or the message.
fn cargo_details(lines: &[String], results: &mut [TestResult]) {
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("thread '") else { continue };
        let Some((name, _)) = rest.split_once("' ") else { continue };
        let said: Vec<String> = lines[i + 1..].iter().map(|l| l.trim()).take_while(|l| !l.is_empty() && !l.starts_with("note:") && !l.starts_with("stack backtrace")).map(str::to_string).take(4).collect();
        if let Some(result) = results.iter_mut().find(|r| !r.passed && r.name == name && r.detail.is_empty()) {
            result.detail = said;
        }
    }
}

/// Everything these readers find in a runner's output, to be added to the older readers' results.
pub fn more_results(lines: &[String], results: &mut Vec<TestResult>) {
    results.extend(jest(lines));
    results.extend(vitest(lines));
    results.extend(phpunit(lines));
    results.extend(testthat(lines));
    pytest_times(lines, results);
    pytest_details(lines, results);
    go_extras(lines, results);
    cargo_details(lines, results);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn jest_says_what_it_expected_and_where() {
        let out = lines("PASS ./a.test.js\nFAIL ./sum.test.js\n  sum\n    \u{2713} adds (2 ms)\n    \u{2715} subtracts (3 ms)\n\n  \u{25CF} sum \u{203A} subtracts\n\n    expect(received).toBe(expected) // Object.is equality\n\n    Expected: 1\n    Received: 0\n\n      at Object.<anonymous> (sum.test.js:5:19)\n");
        let results = jest(&out);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed && results[0].note.as_deref() == Some("2 ms"));
        assert_eq!(results[1].detail, ["Expected: 1", "Received: 0"]);
        assert_eq!(results[1].place, Some(("sum.test.js".into(), 5)));
    }

    #[test]
    fn vitest_names_carry_their_file() {
        let out = lines(" \u{2713} tests/sum.test.ts > sum > adds 3ms\n \u{00D7} tests/sum.test.ts > sum > subtracts 2ms\n \u{2713} tests/other.ts (2 tests) 4ms\n\n FAIL  tests/sum.test.ts > sum > subtracts\nAssertionError: expected 0 to be 1 // Object.is equality\n\n \u{276F} tests/sum.test.ts:5:23\n");
        let results = vitest(&out);
        assert_eq!(results.len(), 2, "the file's own summary line isn't a test");
        assert_eq!((results[0].name.as_str(), results[0].note.as_deref()), ("tests/sum.test.ts > sum > adds", Some("3 ms")));
        assert!(!results[1].passed);
        assert_eq!(results[1].detail, ["AssertionError: expected 0 to be 1 // Object.is equality"]);
        assert_eq!(results[1].place, Some(("tests/sum.test.ts".into(), 5)));
    }

    #[test]
    fn phpunit_teamcity_gives_expected_and_got() {
        let out = lines("##teamcity[testStarted name='testAdd' locationHint='php_qn:///w/tests/MathTest.php::\\Tests\\MathTest::testAdd' flowId='1']\n##teamcity[testFinished name='testAdd' duration='3' flowId='1']\n##teamcity[testStarted name='testSub' locationHint='php_qn:///w/tests/MathTest.php::\\Tests\\MathTest::testSub' flowId='1']\n##teamcity[testFailed name='testSub' message='Failed asserting that 0 matches expected 1.' details=' |n/w/tests/MathTest.php:12|n ' duration='1' type='comparisonFailure' actual='0' expected='1' flowId='1']\n##teamcity[testFinished name='testSub' duration='1' flowId='1']\n");
        let results = phpunit(&out);
        assert_eq!(results.len(), 2);
        assert_eq!((results[0].name.as_str(), results[0].passed, results[0].note.as_deref()), ("Tests\\MathTest::testAdd", true, Some("3 ms")));
        assert!(!results[1].passed);
        assert_eq!(results[1].detail, ["Expected: 1", "Got: 0"]);
        assert_eq!(results[1].place, Some(("/w/tests/MathTest.php".into(), 12)));
    }

    #[test]
    fn teamcity_escapes_are_undone() {
        let (message, pairs) = teamcity_line("##teamcity[testFailed name='it|'s' message='a|nb |[x|]' x='1']").unwrap();
        assert_eq!(message, "testFailed");
        assert_eq!(pairs[0].1, "it's");
        assert_eq!(pairs[1].1, "a\nb [x]");
    }

    #[test]
    fn testthat_locations_mark_the_failing_line() {
        let out = lines("Start test: adds\n  test-a.R:1:21 [success]\nEnd test: adds\n\nStart test: subtracts\n  test-a.R:2:26 [failure]\nEnd test: subtracts\n");
        let results = testthat(&out);
        assert_eq!(results.len(), 2);
        assert!(results[0].passed);
        assert_eq!(results[1].place, Some(("tests/testthat/test-a.R".into(), 2)));
    }

    #[test]
    fn pytest_times_and_assertions_are_added() {
        let out = lines("t.py::test_sub FAILED [100%]\n=== FAILURES ===\n____ test_sub ____\n>       assert 1 - 1 == 1\nE       assert 0 == 1\nE        +  where 0 = 1 - 1\n\nt.py:6: AssertionError\n=== slowest durations ===\n0.01s call     t.py::test_sub\n");
        let results = crate::tasks::test_results(&out);
        assert_eq!(results[0].note.as_deref(), Some("0.01 s"));
        assert_eq!(results[0].detail[0], "assert 0 == 1");
    }

    #[test]
    fn rust_and_go_failures_say_why() {
        let cargo = lines("test t::sub ... FAILED\n\n---- t::sub stdout ----\nthread 't::sub' (9) panicked at src/lib.rs:20:9:\nassertion `left == right` failed\n  left: 0\n right: 1\nnote: run with RUST_BACKTRACE\n");
        let results = crate::tasks::test_results(&cargo);
        assert_eq!(results[0].detail, ["assertion `left == right` failed", "left: 0", "right: 1"]);
        let go = lines("=== RUN   TestSub\n    maths_test.go:14: got 0, want 1\n--- FAIL: TestSub (0.02s)\nFAIL\n");
        let results = crate::tasks::test_results(&go);
        assert_eq!(results[0].detail, ["got 0, want 1"]);
        assert_eq!(results[0].note.as_deref(), Some("0.02 s"));
    }
}
