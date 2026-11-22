//! Finding SQL statements in a file: where each one starts and ends, so Ctrl+Enter runs the one at the cursor.
//! Semicolons end statements, except inside strings, quoted names, comments, PostgreSQL's dollar-quoted bodies,
//! and the BEGIN … END of a trigger or procedure.

use super::Engine;
use std::ops::Range;

/// What's being read through.
#[derive(Clone, Copy, PartialEq)]
enum Inside {
    Code,
    Quote(u8),
    LineComment,
    BlockComment,
}

/// Each statement's place in the text, in bytes: from its first word to its semicolon (or its end), with the
/// space around it left out. Pieces with nothing but comments in them aren't statements, and are left out.
pub fn split(text: &str, engine: Engine) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut inside = Inside::Code;
    let mut start: Option<usize> = None;
    // Where the statement's code ends so far, so trailing comments aren't part of it.
    let mut last_code = 0;
    // The words that matter for BEGIN … END: whether this statement makes a trigger, procedure or function, and
    // how deep in BEGIN (or CASE) it is.
    let mut first_word: Option<String> = None;
    let mut makes_body = false;
    let mut depth = 0usize;
    let mut i = 0;
    let finish = |pieces: &mut Vec<Range<usize>>, start: &mut Option<usize>, end: usize| {
        if let Some(from) = start.take() {
            pieces.push(from..end);
        }
    };
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match inside {
            Inside::LineComment => {
                if b == b'\n' {
                    inside = Inside::Code;
                }
                i += 1;
            }
            Inside::BlockComment => {
                if b == b'*' && next == Some(b'/') {
                    inside = Inside::Code;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Inside::Quote(q) => {
                // MySQL's backslash escapes, and a doubled quote, are part of the string.
                if (b == b'\\' && q == b'\'' && engine == Engine::MySql) || (b == q && next == Some(q)) {
                    i += 2;
                } else {
                    if b == q {
                        inside = Inside::Code;
                        last_code = i + 1;
                    }
                    i += 1;
                }
            }
            Inside::Code => {
                if (b == b'-' && next == Some(b'-')) || (b == b'#' && engine == Engine::MySql) {
                    inside = Inside::LineComment;
                    i += if b == b'#' { 1 } else { 2 };
                    continue;
                }
                if b == b'/' && next == Some(b'*') {
                    inside = Inside::BlockComment;
                    i += 2;
                    continue;
                }
                if b.is_ascii_whitespace() {
                    i += 1;
                    continue;
                }
                if b == b';' && depth == 0 {
                    finish(&mut pieces, &mut start, i + 1);
                    first_word = None;
                    makes_body = false;
                    i += 1;
                    continue;
                }
                if start.is_none() {
                    if b == b';' {
                        i += 1;
                        continue;
                    }
                    start = Some(i);
                }
                if matches!(b, b'\'' | b'"' | b'`') {
                    inside = Inside::Quote(b);
                    i += 1;
                    continue;
                }
                // PostgreSQL's $tag$ … $tag$, which function bodies are written in.
                if b == b'$' && engine == Engine::Postgres && (i == 0 || !is_word_byte(bytes[i - 1])) {
                    let tag_end = bytes[i + 1..].iter().position(|c| !is_word_byte(*c)).map(|p| i + 1 + p);
                    if let Some(tag_end) = tag_end.filter(|e| bytes[*e] == b'$' && !bytes.get(i + 1).is_some_and(u8::is_ascii_digit)) {
                        let tag = &text[i..=tag_end];
                        let body = tag_end + 1;
                        let close = text[body..].find(tag).map_or(bytes.len(), |p| body + p + tag.len());
                        i = close;
                        last_code = close;
                        continue;
                    }
                }
                if is_word_byte(b) {
                    let end = bytes[i..].iter().position(|c| !is_word_byte(*c)).map_or(bytes.len(), |p| i + p);
                    let word = text[i..end].to_ascii_uppercase();
                    if first_word.is_none() {
                        first_word = Some(word.clone());
                    } else if first_word.as_deref() == Some("CREATE") && matches!(word.as_str(), "TRIGGER" | "PROCEDURE" | "FUNCTION" | "EVENT") && engine != Engine::Postgres {
                        makes_body = true;
                    }
                    if makes_body {
                        match word.as_str() {
                            "BEGIN" | "CASE" => depth += 1,
                            "END" => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                    }
                    i = end;
                    last_code = end;
                    continue;
                }
                i += 1;
                last_code = i;
            }
        }
    }
    let end = last_code.max(pieces.last().map_or(0, |p: &Range<usize>| p.end));
    finish(&mut pieces, &mut start, end);
    pieces.retain(|p| p.start < p.end);
    pieces
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// The statement the cursor is in (a byte offset), or the one before it when it's between statements, or the
/// first when it's before them all.
pub fn at_cursor(text: &str, cursor: usize, engine: Engine) -> Option<Range<usize>> {
    let pieces = split(text, engine);
    pieces.iter().find(|p| p.start <= cursor && cursor <= p.end).or_else(|| pieces.iter().rev().find(|p| p.end <= cursor)).or_else(|| pieces.first()).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(text: &str, engine: Engine) -> Vec<&str> {
        split(text, engine).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn semicolons_end_statements_but_not_in_strings_names_or_comments() {
        let text = "SELECT 'a;b', \"c;d\" FROM t; -- one; two\n  INSERT INTO t VALUES (1) /* ; */ ;\n\nUPDATE t SET x = 'it''s;'";
        assert_eq!(pieces(text, Engine::Postgres), ["SELECT 'a;b', \"c;d\" FROM t;", "INSERT INTO t VALUES (1) /* ; */ ;", "UPDATE t SET x = 'it''s;'"]);
    }

    #[test]
    fn mysql_has_backticks_backslashes_and_hash_comments() {
        let text = "SELECT `a;b` FROM t WHERE x = 'don\\'t;'; # gone;\nSELECT 2";
        assert_eq!(pieces(text, Engine::MySql), ["SELECT `a;b` FROM t WHERE x = 'don\\'t;';", "SELECT 2"]);
    }

    #[test]
    fn dollar_quoted_bodies_keep_their_semicolons() {
        let text = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql;\nSELECT f(); SELECT $1";
        assert_eq!(pieces(text, Engine::Postgres), ["CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END; $body$ LANGUAGE plpgsql;", "SELECT f();", "SELECT $1"]);
        assert_eq!(pieces("SELECT $$a;b$$; SELECT 2", Engine::Postgres), ["SELECT $$a;b$$;", "SELECT 2"]);
    }

    #[test]
    fn a_triggers_body_is_one_statement() {
        let text = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = CASE WHEN n > 1 THEN 0 ELSE n END; DELETE FROM c; END; SELECT 1;";
        assert_eq!(pieces(text, Engine::Sqlite), ["CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = CASE WHEN n > 1 THEN 0 ELSE n END; DELETE FROM c; END;", "SELECT 1;"]);
        // BEGIN on its own starts a transaction, and ends at its semicolon.
        assert_eq!(pieces("BEGIN; SELECT 1; END;", Engine::Sqlite), ["BEGIN;", "SELECT 1;", "END;"]);
    }

    #[test]
    fn comments_alone_and_empty_statements_are_left_out() {
        assert_eq!(pieces("-- nothing here\n;;  /* or here */ ", Engine::Sqlite), Vec::<&str>::new());
        assert_eq!(pieces("SELECT 1 -- the end", Engine::Sqlite), ["SELECT 1"]);
        assert_eq!(pieces("-- what it does\nSELECT 1;", Engine::Sqlite), ["SELECT 1;"]);
    }

    #[test]
    fn the_cursor_finds_its_statement_or_the_one_before() {
        let text = "SELECT 1;\n\nSELECT 2;\n";
        let at = |cursor| at_cursor(text, cursor, Engine::Sqlite).map(|r| &text[r]);
        assert_eq!(at(0), Some("SELECT 1;"));
        assert_eq!(at(9), Some("SELECT 1;"), "just after its semicolon");
        assert_eq!(at(10), Some("SELECT 1;"), "on the blank line after it");
        assert_eq!(at(14), Some("SELECT 2;"));
        assert_eq!(at(text.len()), Some("SELECT 2;"), "at the very end");
        assert_eq!(at_cursor("  ", 1, Engine::Sqlite), None);
    }
}
