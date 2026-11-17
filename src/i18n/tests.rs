use super::*;

/// This thread's words in a made-up table, for as long as `f` runs.
fn with<T>(table: &[(&str, &str)], f: impl FnOnce() -> T) -> T {
    OVERRIDE.with(|o| *o.borrow_mut() = Some(table.iter().map(|(e, t)| (e.to_string(), t.to_string())).collect()));
    let out = f();
    OVERRIDE.with(|o| *o.borrow_mut() = None);
    out
}

#[test]
fn words_are_translated_where_the_table_has_them() {
    with(&[("Open", "Ouvrir")], || {
        assert_eq!(tr("Open"), "Ouvrir");
        assert_eq!(tr("Close"), "Close", "what isn't in the table stays English");
    });
    assert_eq!(tr("Open"), "Open");
}

#[test]
fn values_go_into_the_translation_wherever_it_puts_them() {
    let (count, name) = (3, "a.txt");
    assert_eq!(tf!("{count} lines in {name}"), "3 lines in a.txt", "English is format! as it was");
    assert_eq!(tf!("{} of {} at ${:04X}", 1, 2, 255), "1 of 2 at $00FF");
    with(&[("{count} lines in {name}", "dans {name}, {count} lignes"), ("{} of {} at ${:04X}", "{} sur {} \u{E0} ${:04X}"), ("{{braces}} {count}", "{{accolades}} {count}")], || {
        assert_eq!(tf!("{count} lines in {name}"), "dans a.txt, 3 lignes");
        assert_eq!(tf!("{} of {} at ${:04X}", 1, 2, 255), "1 sur 2 \u{E0} $00FF");
        assert_eq!(tf!("{{braces}} {count}"), "{accolades} 3");
        let width = 6;
        assert_eq!(tf!("[{name:>width$}]"), "[ a.txt]", "not in the table: format! as it was");
    });
    // Each argument is worked out once.
    let mut calls = 0;
    let mut once = || {
        calls += 1;
        calls
    };
    assert_eq!(tf!("{}", once()), "1");
    assert_eq!(calls, 1);
}

#[test]
fn generated_source_has_its_comments_translated_and_its_code_kept() {
    let source = "        ld a,2          ; print on the upper screen\n; The bank\nmessage: db \"; not a comment\",0 ; the text\n    // C too\n* 68000 too\n        rts";
    let out = with(&[("print on the upper screen", "afficher en haut"), ("The bank", "La banque"), ("the text", "le texte"), ("C too", "C aussi"), ("68000 too", "68000 aussi")], || comments(source));
    assert_eq!(out, "        ld a,2          ; afficher en haut\n; La banque\nmessage: db \"; not a comment\",0 ; le texte\n    // C aussi\n* 68000 aussi\n        rts");
    assert_eq!(comments(source), source, "in English, untouched");
}

#[test]
fn every_language_is_listed_by_its_own_name_and_its_table_reads() {
    let names: Vec<&str> = LANGUAGES.iter().map(|l| l.name).collect();
    assert_eq!(names, ["English", "Deutsch", "Espa\u{F1}ol", "Fran\u{E7}ais", "Italiano"]);
    for language in &LANGUAGES[1..] {
        for (english, translated) in read_table(language.table) {
            // A translation keeps the English's placeholders, so the values all go in.
            let mut wanted = placeholders(&english);
            let mut got = placeholders(&translated);
            wanted.sort();
            got.sort();
            assert_eq!(wanted, got, "{}: {english:?} \u{2192} {translated:?}", language.code);
        }
    }
}

/// The placeholders of a format string, as written.
fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            rest = &tail[2..];
        } else if tail.starts_with('{')
            && let Some(end) = tail.find('}')
        {
            out.push(tail[..=end].to_string());
            rest = &tail[end + 1..];
        } else {
            rest = &tail[1..];
        }
    }
    out
}

mod catalogue;
