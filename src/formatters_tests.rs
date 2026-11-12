use super::*;

fn run(kind: Kind, text: &str) -> String {
    apply(kind, text, "  ").unwrap()
}

#[test]
fn xml_lays_elements_out_by_depth() {
    assert_eq!(run(Kind::Xml, "<a><b>x</b><c/><d><e/></d></a>"), "<a>\n  <b>x</b>\n  <c/>\n  <d>\n    <e/>\n  </d>\n</a>");
}

#[test]
fn xml_keeps_the_prolog_comments_doctype_and_attributes_as_written() {
    let text = "<?xml version=\"1.0\"?><!DOCTYPE a [<!ENTITY b \">\">]><!-- note --><a x=\"1>2\" y='&lt;'><b/></a>";
    assert_eq!(run(Kind::Xml, text), "<?xml version=\"1.0\"?>\n<!DOCTYPE a [<!ENTITY b \">\">]>\n<!-- note -->\n<a x=\"1>2\" y='&lt;'>\n  <b/>\n</a>");
}

#[test]
fn xml_text_keeps_its_spaces_wherever_they_could_mean_something() {
    assert_eq!(run(Kind::Xml, "<a><b> two  spaces </b></a>"), "<a>\n  <b> two  spaces </b>\n</a>");
    assert_eq!(run(Kind::Xml, "<a>mixed <b>bold</b> text</a>"), "<a>mixed <b>bold</b> text</a>");
    assert_eq!(run(Kind::Xml, "<a xml:space=\"preserve\">  <b/>  </a>"), "<a xml:space=\"preserve\">  <b/>  </a>");
    assert_eq!(run(Kind::Xml, "<a><![CDATA[ <x>  ]]></a>"), "<a><![CDATA[ <x>  ]]></a>");
}

#[test]
fn xml_empty_elements_close_up_and_compress_removes_the_layout() {
    assert_eq!(run(Kind::Xml, "<a>\n\n   </a>"), "<a></a>");
    assert_eq!(run(Kind::XmlCompact, "<a>\n  <b>x y</b>\n  <c/>\n</a>\n"), "<a><b>x y</b><c/></a>\n");
}

#[test]
fn xml_that_isnt_well_formed_is_refused_and_a_fragment_is_fine() {
    let problem = apply(Kind::Xml, "<a><b></a>", "  ").unwrap_err();
    assert!(problem.contains("well-formed"), "{problem}");
    assert!(apply(Kind::Xml, "<a", "  ").is_err());
    assert_eq!(run(Kind::Xml, "<a/><b/>"), "<a/>\n<b/>", "part of a document can be selected");
}

#[test]
fn html_lays_out_blocks_and_leaves_inline_text_flowing() {
    let text = "<html><head><title>T</title></head><body><div><p>one <b>two</b>  three</p><ul><li>a</li><li>b</li></ul></div></body></html>";
    assert_eq!(run(Kind::Html, text), "<html>\n  <head>\n    <title>T</title>\n  </head>\n  <body>\n    <div>\n      <p>one <b>two</b> three</p>\n      <ul>\n        <li>a</li>\n        <li>b</li>\n      </ul>\n    </div>\n  </body>\n</html>");
}

#[test]
fn html_copes_with_void_and_unclosed_elements() {
    assert_eq!(run(Kind::Html, "<div><br><img src=a><hr></div>"), "<div><br><img src=a>\n  <hr>\n</div>");
    assert_eq!(run(Kind::Html, "<ul><li>a<li>b</ul>"), "<ul>\n  <li>a\n  <li>b\n</ul>");
    assert_eq!(run(Kind::Html, "<p>a<p>b"), "<p>a\n<p>b");
    assert_eq!(run(Kind::Html, "</div>x"), "</div>\nx", "a closing tag with nothing to close does no harm");
}

#[test]
fn html_keeps_scripts_styles_and_pre_exactly() {
    let text = "<div><pre>  a\n   b </pre><script>if (a<b) {\n  c();\n}</script><STYLE>a { b: c }</STYLE></div>";
    let done = run(Kind::Html, text);
    for kept in ["<pre>  a\n   b </pre>", "<script>if (a<b) {\n  c();\n}</script>", "<STYLE>a { b: c }</STYLE>"] {
        assert!(done.contains(kept), "{kept} in {done}");
    }
}

#[test]
fn html_keeps_comments_doctype_and_a_stray_angle_bracket() {
    assert_eq!(run(Kind::Html, "<!DOCTYPE html><!-- a  b --><p>1 < 2</p>"), "<!DOCTYPE html>\n<!-- a  b -->\n<p>1 < 2</p>");
}

#[test]
fn html_compress_takes_out_what_doesnt_show() {
    assert_eq!(run(Kind::HtmlCompact, "<div>\n  <p>a  <b>b</b> c</p>\n  <p>d</p>\n</div>"), "<div><p>a <b>b</b> c</p><p>d</p></div>");
}

#[test]
fn css_gets_one_declaration_to_a_line() {
    assert_eq!(run(Kind::Css, "a,b{color:red;margin:0 auto}c{d:e;}"), "a,b {\n  color:red;\n  margin:0 auto\n}\n\nc {\n  d:e;\n}");
}

#[test]
fn css_copes_with_at_rules_strings_comments_and_urls() {
    assert_eq!(run(Kind::Css, "@media x{a{b:c}}"), "@media x {\n  a {\n    b:c\n  }\n}");
    assert_eq!(run(Kind::Css, "a{content:\"};{\";b:url(data:x;y)}"), "a {\n  content:\"};{\";\n  b:url(data:x;y)\n}");
    assert_eq!(run(Kind::Css, "/* one */\na{/* two */b:c}"), "/* one */\na {\n  /* two */\n  b:c\n}");
}

#[test]
fn css_compress_keeps_what_matters() {
    assert_eq!(run(Kind::CssCompact, "/* x */\na , b {\n  color : red ;\n  margin: 0  auto;\n}\n"), "a,b{color:red;margin:0 auto;}\n");
    assert_eq!(run(Kind::CssCompact, "a :hover { b: \"c,  d: e\" }"), "a :hover{b:\"c,  d: e\"}", "a space before a colon in a selector means something, and strings stay");
    assert_eq!(run(Kind::CssCompact, "a{b:calc(1px + 2px)}"), "a{b:calc(1px + 2px)}");
}

#[test]
fn script_and_code_kinds_go_through_the_code_formatter() {
    assert_eq!(run(Kind::Js, "a();b();"), "a();\nb();");
    assert_eq!(run(Kind::JsCompact, "  a(); // x\n"), "a();\n");
    assert_eq!(run(Kind::CodeAttach, "f()\n{\ng();\n}"), "f() {\n  g();\n}");
    assert_eq!(run(Kind::CodeBreak, "f() {\ng();\n}"), "f()\n{\n  g();\n}");
    assert!(apply(Kind::Js, "a {", "  ").is_err());
}

#[test]
fn sql_starts_each_clause_on_a_line_and_makes_keywords_upper_case() {
    assert_eq!(run(Kind::Sql, "select a, b from c where d = 1 and e = 2 order by f;"), "SELECT a,\n  b\nFROM c\nWHERE d = 1\n  AND e = 2\nORDER BY f;");
}

#[test]
fn sql_joins_subqueries_and_between() {
    assert_eq!(run(Kind::Sql, "select * from a left outer join b on a.x = b.x where c between 1 and 2"), "SELECT *\nFROM a\nLEFT OUTER JOIN b\n  ON a.x = b.x\nWHERE c BETWEEN 1 AND 2");
    assert_eq!(run(Kind::Sql, "select a from (select b from c) d"), "SELECT a\nFROM (\n  SELECT b\n  FROM c\n) d");
    assert_eq!(run(Kind::Sql, "select left(a, 1), count(b, c) from d"), "SELECT LEFT(a, 1),\n  count(b, c)\nFROM d", "a function's own commas don't break the line");
}

#[test]
fn sql_never_touches_strings_quoted_names_or_comments() {
    let text = "select 'from  where' , \"Select\" , [order by], `from` from t -- from here\nwhere a = 'it''s'";
    let done = run(Kind::Sql, text);
    for kept in ["'from  where'", "\"Select\"", "[order by]", "`from`", "-- from here", "'it''s'"] {
        assert!(done.contains(kept), "{kept} in {done}");
    }
}

#[test]
fn sql_only_upper_cases_words_that_cant_be_names() {
    assert_eq!(run(Kind::Sql, "select name, Value from Users"), "SELECT name,\n  Value\nFROM Users");
}

#[test]
fn sql_statements_are_kept_apart_and_dollar_quoting_stays() {
    assert_eq!(run(Kind::Sql, "select 1;select 2;"), "SELECT 1;\n\nSELECT 2;");
    let body = "create function f() as $$ select  1 ; $$";
    assert!(run(Kind::Sql, body).contains("$$ select  1 ; $$"));
    assert!(run(Kind::Sql, "delete from a").starts_with("DELETE FROM a"));
    assert!(run(Kind::Sql, "update a set b = 1, c = 2").contains("SET b = 1,\n  c = 2"));
}

const X12: &str = "ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *210101*1200*U*00401*000000001*0*P*>~GS*PO*A*B*20210101*1200*1*X*004010~ST*850*0001~SE*2*0001~";

#[test]
fn x12_files_split_at_their_own_terminator_and_join_back() {
    assert_eq!(edi_marks(X12), Ok(('~', None)));
    let lines = run(Kind::EdiLines, X12);
    assert_eq!(lines.lines().count(), 4);
    assert!(lines.lines().all(|line| line.ends_with('~')));
    assert_eq!(run(Kind::EdiOneLine, &lines), X12);
}

#[test]
fn edifact_respects_its_release_character_and_una() {
    assert_eq!(run(Kind::EdiLines, "UNB+A'BGM+1?'2'UNZ+1'"), "UNB+A'\nBGM+1?'2'\nUNZ+1'");
    assert_eq!(run(Kind::EdiLines, "UNA:+.? ~UNB+A~BGM+1~"), "UNA:+.? ~\nUNB+A~\nBGM+1~", "UNA says which marks are in use");
    assert_eq!(run(Kind::EdiOneLine, "UNB+A'\nBGM+1'\n"), "UNB+A'BGM+1'\n");
}

#[test]
fn files_that_arent_edi_are_refused() {
    assert!(apply(Kind::EdiLines, "plain text", "  ").unwrap_err().contains("X12"));
    assert!(apply(Kind::EdiLines, "ISA*00", "  ").unwrap_err().contains("too short"));
    let mut line_ended = X12.to_string().into_bytes();
    line_ended[105] = b'\n';
    assert!(apply(Kind::EdiLines, &String::from_utf8(line_ended).unwrap(), "  ").unwrap_err().contains("line break"));
}

#[test]
fn a_trailing_line_break_is_kept_or_left_off_as_it_was() {
    assert_eq!(run(Kind::Xml, "<a/>\r\n"), "<a/>\n");
    assert_eq!(run(Kind::Xml, "<a/>"), "<a/>");
}

#[test]
fn odd_input_never_goes_wrong_and_changes_only_layout() {
    let kinds = [Kind::Xml, Kind::XmlCompact, Kind::Html, Kind::HtmlCompact, Kind::Js, Kind::JsCompact, Kind::Css, Kind::CssCompact, Kind::CodeAttach, Kind::CodeBreak, Kind::Sql, Kind::EdiLines, Kind::EdiOneLine];
    let samples = ["", " ", "<", ">", "</", "<a", "<!--", "<![CDATA[", "<?", "<!", "<a b='", "'", "\"", "`", "/*", "--", "$", "$$", "$a", "[", "]", "{", "}", "(", ")", ";", ",", "\u{e9}<a>\u{4e2d}</a>", "UNA", "ISA", "UNB", "select", "<script>", "<pre", "a{", "a}", "\u{10FF41}", "<a>\u{1F600}</a>", "\n\n\n"];
    for kind in kinds {
        for text in samples {
            if let Ok(done) = apply(kind, text, "  ") {
                match kind {
                    Kind::JsCompact | Kind::CssCompact | Kind::Sql => {}
                    _ => assert!(same_but_for_space(text, &done), "{kind:?} on {text:?} gave {done:?}"),
                }
            }
        }
    }
}

#[test]
fn reformatting_twice_changes_nothing_the_second_time() {
    let cases = [
        (Kind::Xml, "<a><b>x</b><!-- c --><d><e/></d></a>"),
        (Kind::Html, "<html><body><div><p>one <b>two</b></p><ul><li>a</ul></div></body></html>"),
        (Kind::Css, "a{b:c;d:e}f{g:h}"),
        (Kind::Sql, "select a, b from c join d on e = f where g = 1 or h = 2"),
    ];
    for (kind, text) in cases {
        let once = run(kind, text);
        assert_eq!(run(kind, &once), once, "{kind:?}");
    }
}
