//! Folding and several selections, at the level of the text itself.

use crate::text_buffer::*;

fn buffer(lines: &[&str]) -> TextBuffer {
    let mut b = TextBuffer::default();
    b.set_content(lines.iter().map(|s| s.to_string()).collect());
    b
}

fn text(b: &TextBuffer) -> Vec<String> {
    b.all_lines().iter().map(|l| l.to_string()).collect()
}

#[test]
fn a_tag_wraps_the_selection_or_leaves_the_cursor_inside() {
    let mut b = buffer(&["some words here"]);
    b.set_selection((0, 5), (0, 10), false);
    b.wrap_selection("<b>", "</b>");
    assert_eq!(text(&b), ["some <b>words</b> here"]);
    b.undo();
    assert_eq!(text(&b), ["some words here"]);
    b.move_caret(0, 4, false);
    b.wrap_selection("**", "**");
    assert_eq!(text(&b), ["some**** words here"]);
    assert_eq!(b.caret_col, 6, "between the two");
}

#[test]
fn a_tag_can_wrap_several_lines_and_several_selections() {
    let mut b = buffer(&["one", "two"]);
    b.select_all();
    b.wrap_selection("<p>", "</p>");
    assert_eq!(text(&b), ["<p>one", "two</p>"]);
    let mut many = buffer(&["cat dog cat"]);
    many.set_selection((0, 0), (0, 3), false);
    many.make_selections().unwrap();
    many.wrap_selection("[", "]");
    assert_eq!(text(&many), ["[cat] dog [cat]"]);
}

#[test]
fn replacing_lines_can_change_how_many_there_are_and_undoes_in_one_step() {
    let mut b = buffer(&["a", "b", "c", "d"]);
    assert!(b.replace_lines(1, 2, vec!["x".into(), "y".into(), "z".into()]));
    assert_eq!(text(&b), ["a", "x", "y", "z", "d"]);
    assert_eq!((b.anchor_line, b.anchor_col, b.caret_line, b.caret_col), (1, 0, 3, 1), "what's new is selected");
    b.undo();
    assert_eq!(text(&b), ["a", "b", "c", "d"]);
    assert!(!b.is_modified());
    assert!(b.replace_lines(0, 3, vec!["all".into()]));
    assert_eq!(text(&b), ["all"]);
}

#[test]
fn replacing_lines_with_the_same_is_no_edit_and_leaves_other_lines_unmarked() {
    let mut b = buffer(&["a", "b", "c"]);
    assert!(!b.replace_lines(0, 2, vec!["a".into(), "b".into(), "c".into()]));
    assert!(!b.can_undo());
    b.toggle_bookmark(2);
    assert!(b.replace_lines(0, 2, vec!["a".into(), "B".into(), "c".into()]));
    assert_eq!(b.change_at(0), None);
    assert_eq!(b.change_at(1), Some(false));
    assert_eq!(b.change_at(2), None);
    assert!(b.is_bookmarked(2));
}

#[test]
fn the_paragraph_is_found_around_the_cursor_or_from_the_selection() {
    let mut b = buffer(&["a", "b", "", "c", "d", "e", "", "f"]);
    b.move_caret(4, 0, false);
    assert_eq!(b.rows_for_paragraph(), Some((3, 5)));
    b.move_caret(2, 0, false);
    assert_eq!(b.rows_for_paragraph(), None, "a blank line has no paragraph");
    b.set_selection((0, 0), (3, 1), false);
    assert_eq!(b.rows_for_paragraph(), Some((0, 3)));
    b.move_caret(7, 0, false);
    assert_eq!(b.rows_for_paragraph(), Some((7, 7)));
}

fn nested() -> TextBuffer {
    buffer(&["fn a() {", "    one", "    if x {", "        two", "    }", "", "}", "", "fn b() {", "    three", "}", "flat"])
}

#[test]
fn a_fold_covers_what_is_indented_under_a_line() {
    let b = nested();
    assert_eq!(b.fold_end(0), Some(4), "up to the last indented line, without the closing brace");
    assert_eq!(b.fold_end(2), Some(3));
    assert_eq!(b.fold_end(1), None, "nothing is indented under it");
    assert_eq!(b.fold_end(5), None, "a blank line");
    assert!(b.can_fold(0) && b.can_fold(2) && b.can_fold(8));
    assert!(!b.can_fold(1) && !b.can_fold(5) && !b.can_fold(11));
}

#[test]
fn blank_lines_inside_a_fold_go_with_it_and_trailing_ones_stay_out() {
    let b = buffer(&["top", "    a", "", "    b", "", "next"]);
    assert_eq!(b.fold_end(0), Some(3));
}

#[test]
fn folding_hides_lines_without_touching_the_text() {
    let mut b = nested();
    let before = text(&b);
    assert!(b.toggle_fold(0));
    assert_eq!(b.unfolded_lines(), [0, 5, 6, 7, 8, 9, 10, 11]);
    assert_eq!(text(&b), before);
    assert!(!b.is_modified());
    assert!(!b.can_undo());
    assert!(b.toggle_fold(0));
    assert_eq!(b.unfolded_lines(), (0..12).collect::<Vec<_>>());
    assert!(!b.toggle_fold(1), "nothing to fold there");
}

#[test]
fn folds_inside_folds_keep_their_state() {
    let mut b = nested();
    b.toggle_fold(2);
    b.toggle_fold(0);
    assert_eq!(b.unfolded_lines()[..2], [0, 5]);
    b.toggle_fold(0);
    assert_eq!(b.unfolded_lines()[..5], [0, 1, 2, 4, 5], "the inner one is still folded");
}

#[test]
fn collapse_all_folds_the_outermost_and_expand_all_opens_everything() {
    let mut b = nested();
    b.toggle_fold(2);
    b.collapse_all();
    assert_eq!(b.unfolded_lines(), [0, 5, 6, 7, 8, 10, 11]);
    b.expand_all();
    assert!(!b.has_folds());
    assert_eq!(b.unfolded_lines().len(), 12);
}

#[test]
fn folding_moves_the_cursor_out_of_what_it_hides() {
    let mut b = nested();
    b.move_caret(3, 2, false);
    b.toggle_fold(0);
    assert_eq!((b.caret_line, b.caret_col), (0, 0));
    b.expand_all();
    b.move_caret(9, 0, false);
    b.collapse_all();
    assert_eq!(b.caret_line, 8);
}

#[test]
fn folds_follow_their_lines_as_others_come_and_go() {
    let mut b = nested();
    b.toggle_fold(8);
    b.move_caret(0, 0, false);
    b.new_line();
    assert!(b.is_folded(9), "a line added above moves it down");
    assert_eq!(b.folds().get(&9), Some(&10));
    b.set_selection((0, 0), (1, 0), false);
    b.delete_selection();
    assert!(b.is_folded(8), "and a line taken away moves it up");
    b.move_caret(11, 0, false);
    b.type_text("x");
    assert!(b.is_folded(8), "an edit below leaves it alone");
}

#[test]
fn an_edit_reaching_into_hidden_lines_opens_the_fold() {
    let mut b = nested();
    b.toggle_fold(8);
    b.set_selection((7, 0), (10, 0), false);
    b.delete_selection();
    assert!(!b.has_folds());
    let mut enter = nested();
    enter.toggle_fold(8);
    enter.move_caret(8, 0, false);
    enter.new_line();
    assert!(!enter.has_folds(), "a new line right under the folded one lands inside it");
}

#[test]
fn undo_brings_back_the_folds_of_that_moment() {
    let mut b = nested();
    b.toggle_fold(8);
    b.set_selection((7, 0), (10, 0), false);
    b.delete_selection();
    assert!(!b.has_folds());
    b.undo();
    assert!(b.is_folded(8));
}

#[test]
fn selected_lines_can_be_hidden_and_shown() {
    let mut b = buffer(&["a", "b", "c", "d", "e"]);
    b.set_selection((1, 0), (3, 1), false);
    assert!(b.hide_selected_lines());
    assert_eq!(b.unfolded_lines(), [0, 1, 4]);
    assert_eq!(b.caret_line, 1, "the cursor comes out of what's hidden");
    b.set_selection((0, 0), (1, 1), false);
    b.show_selected_lines();
    assert_eq!(b.unfolded_lines().len(), 5);
    b.move_caret(2, 0, false);
    assert!(!b.hide_selected_lines(), "one line has nothing to hide under it");
}

#[test]
fn reveal_opens_only_what_hides_the_line() {
    let mut b = nested();
    b.toggle_fold(0);
    b.toggle_fold(8);
    assert!(b.reveal_line(3));
    assert!(!b.is_folded(0));
    assert!(b.is_folded(8));
    assert!(!b.reveal_line(0));
    assert!(!b.reveal_line(8), "the folded line itself is showing");
}

fn apples() -> TextBuffer {
    let mut b = buffer(&["apple and apple", "pear", "an apple"]);
    b.set_selection((0, 0), (0, 5), false);
    b
}

#[test]
fn make_selections_selects_every_copy_and_typing_replaces_them_all() {
    let mut b = apples();
    assert_eq!(b.make_selections(), Ok(3));
    assert_eq!(b.multi_on_line(0), [(0, 5, 5), (10, 15, 15)]);
    assert!(b.position_text().contains("3 selections"));
    b.type_text("X");
    assert_eq!(text(&b), ["X and X", "pear", "an X"]);
    b.type_text("Y");
    assert_eq!(text(&b), ["XY and XY", "pear", "an XY"], "the cursors carry on from there");
    assert_eq!(b.multi_count(), 3);
}

#[test]
fn several_selections_undo_in_one_step() {
    let mut b = apples();
    b.make_selections().unwrap();
    b.paste_text("kiwi", false);
    assert_eq!(text(&b), ["kiwi and kiwi", "pear", "an kiwi"]);
    b.undo();
    assert_eq!(text(&b), ["apple and apple", "pear", "an apple"]);
    assert!(!b.has_multi(), "and it's back to one cursor");
    b.redo();
    assert_eq!(text(&b), ["kiwi and kiwi", "pear", "an kiwi"]);
}

#[test]
fn make_selections_says_why_not() {
    let mut none = buffer(&["apple"]);
    assert!(none.make_selections().is_err(), "nothing selected");
    none.select_all();
    assert!(none.make_selections().unwrap_err().contains("no other copies"));
    let mut lines = buffer(&["a", "a"]);
    lines.set_selection((0, 0), (1, 1), false);
    assert!(lines.make_selections().is_err(), "more than one line");
    let mut column = buffer(&["a a"]);
    column.set_column_mode(true);
    column.set_selection((0, 0), (0, 1), false);
    assert!(column.make_selections().is_err());
    assert!(!column.has_multi());
}

#[test]
fn make_selections_stops_at_its_limit_and_changes_nothing() {
    let line = "a".repeat(MOST_SPOTS + 5);
    let mut b = buffer(&[&line]);
    b.set_selection((0, 0), (0, 1), false);
    assert!(b.make_selections().is_err());
    assert!(!b.has_multi());
}

#[test]
fn copies_overlapping_themselves_dont_overlap() {
    let mut b = buffer(&["aaaa"]);
    b.set_selection((0, 0), (0, 2), false);
    assert_eq!(b.make_selections(), Ok(2));
    b.type_text("-");
    assert_eq!(text(&b), ["--"]);
}

#[test]
fn backspace_and_delete_stop_at_the_ends_of_lines() {
    let mut b = buffer(&["ab", "cd"]);
    b.add_cursor(1, 0);
    assert_eq!(b.multi_count(), 2);
    b.backspace();
    assert_eq!(text(&b), ["ab", "cd"], "no joining lines: there'd be no telling which cursor went where");
    b.delete_forward();
    assert_eq!(text(&b), ["b", "d"]);
    b.multi_move(SpotMove::End, false);
    b.delete_forward();
    assert_eq!(text(&b), ["b", "d"]);
    b.backspace();
    assert_eq!(text(&b), ["", ""]);
}

#[test]
fn neighbouring_cursors_cant_trip_over_each_other() {
    let mut b = buffer(&["abcdef"]);
    b.add_cursor(0, 1);
    b.add_cursor(0, 2);
    assert_eq!(b.multi_count(), 3);
    b.backspace();
    assert_eq!(text(&b), ["cdef"], "the first has nothing before it, and the others take a letter each");
    b.type_text("X");
    assert!(b.line(0).contains('X'));
}

#[test]
fn ctrl_clicking_adds_and_takes_away_cursors() {
    let mut b = buffer(&["one", "two", "three"]);
    b.add_cursor(2, 1);
    assert_eq!(b.multi_count(), 2);
    b.add_cursor(1, 2);
    assert_eq!(b.multi_count(), 3);
    b.type_text("-");
    assert_eq!(text(&b), ["-one", "tw-o", "t-hree"]);
    b.add_cursor(1, 3);
    assert_eq!(b.multi_count(), 2, "a click on a cursor takes it away");
    b.add_cursor(2, 2);
    assert!(!b.has_multi(), "down to one, it's an ordinary cursor again");
    assert_eq!(b.caret_line, 0);
}

#[test]
fn a_selection_can_be_the_first_of_several() {
    let mut b = buffer(&["hello", "world"]);
    b.set_selection((0, 1), (0, 3), false);
    b.add_cursor(1, 2);
    assert_eq!(b.multi_text(), "el");
    b.type_text("X");
    assert_eq!(text(&b), ["hXlo", "woXrld"]);
}

#[test]
fn copying_gives_each_selection_on_its_own_line() {
    let mut b = apples();
    b.make_selections().unwrap();
    assert_eq!(b.multi_text(), "apple\r\napple\r\napple");
}

#[test]
fn pasting_many_lines_goes_one_to_each_selection_or_not_at_all() {
    let mut b = apples();
    b.make_selections().unwrap();
    b.paste_text("a\r\nb\r\nc\r\n", false);
    assert_eq!(text(&b), ["a and b", "pear", "an c"], "a last empty line doesn't count");
    b.paste_text("x\ny", false);
    assert_eq!(text(&b), ["a and b", "pear", "an c"]);
    assert!(b.notice.take().is_some_and(|notice| notice.contains("2 lines") && notice.contains("3 selections")));
    assert_eq!(b.multi_count(), 3, "and they're all still there");
}

#[test]
fn every_cursor_moves_together_and_those_that_meet_become_one() {
    let mut b = buffer(&["abc", "de", "f"]);
    b.add_cursor(1, 2);
    b.add_cursor(2, 1);
    b.multi_move(SpotMove::Right, false);
    assert_eq!(b.multi_on_line(0), [(1, 1, 1)]);
    assert_eq!(b.multi_on_line(1), [(2, 2, 2)], "stops at the end of the line");
    b.multi_move(SpotMove::Home, false);
    assert_eq!(b.multi_on_line(1), [(0, 0, 0)]);
    b.multi_move(SpotMove::End, false);
    assert_eq!(b.multi_on_line(0), [(3, 3, 3)]);
    b.multi_move(SpotMove::Up, false);
    assert_eq!(b.multi_on_line(0), [(2, 2, 2), (3, 3, 3)], "at different columns they're still two");
    b.multi_move(SpotMove::Home, false);
    assert_eq!(b.multi_count(), 2, "on the same spot they've become one");
    b.multi_move(SpotMove::Up, false);
    assert!(!b.has_multi(), "when only one is left, it's an ordinary cursor again");
    assert_eq!((b.caret_line, b.caret_col), (0, 0));
}

#[test]
fn shift_grows_every_selection_along_its_line() {
    let mut b = buffer(&["abc", "abc"]);
    b.add_cursor(1, 0);
    b.multi_move(SpotMove::Right, true);
    b.multi_move(SpotMove::Right, true);
    assert_eq!(b.multi_text(), "ab\r\nab");
    b.multi_move(SpotMove::Left, false);
    assert_eq!(b.multi_on_line(0), [(0, 0, 0)], "without Shift, a selection closes up");
}

#[test]
fn tab_inserts_at_cursors_and_indents_lines_with_selections() {
    set_edit_prefs(EditPrefs { spaces_for_tab: true, ..EditPrefs::default() });
    set_tab_size(2);
    let mut b = buffer(&["ab", "cd"]);
    b.move_caret(0, 1, false);
    b.add_cursor(1, 1);
    b.tab_key(false);
    assert_eq!(text(&b), ["a  b", "c  d"]);
    let mut selected = buffer(&["x y x", "z", "x"]);
    selected.set_selection((0, 0), (0, 1), false);
    selected.make_selections().unwrap();
    selected.tab_key(false);
    assert_eq!(text(&selected), ["  x y x", "z", "  x"]);
    assert_eq!(selected.multi_text(), "x\r\nx\r\nx", "each line once, and the selections stay on their text");
    selected.tab_key(true);
    assert_eq!(text(&selected), ["x y x", "z", "x"]);
    assert_eq!(selected.multi_text(), "x\r\nx\r\nx");
    set_edit_prefs(EditPrefs::default());
    set_tab_size(DEFAULT_TAB_SIZE);
}

#[test]
fn anything_else_ends_several_selections() {
    let mut b = apples();
    b.make_selections().unwrap();
    b.move_caret(1, 0, false);
    assert!(!b.has_multi());
    let mut enter = apples();
    enter.make_selections().unwrap();
    enter.new_line();
    assert!(!enter.has_multi(), "Enter happens at one cursor");
    assert_eq!(enter.line_count(), 4);
    let mut all = apples();
    all.make_selections().unwrap();
    all.select_all();
    assert!(!all.has_multi());
}

#[test]
fn typing_text_with_a_line_break_goes_to_one_place() {
    let mut b = apples();
    b.make_selections().unwrap();
    b.type_text("a\nb");
    assert!(!b.has_multi());
}

#[test]
fn make_selections_opens_folds_hiding_its_copies() {
    let mut b = buffer(&["top apple", "    apple", "next"]);
    b.toggle_fold(0);
    b.set_selection((0, 4), (0, 9), false);
    b.make_selections().unwrap();
    assert!(!b.has_folds(), "editing what can't be seen would be a nasty surprise");
}
