use heddle::tools::fuzzy_match::{cascading_match, find_closest_match};

mod common;

#[test]
fn exact_match_level_0() {
    let content = "hello foo bar world";
    let search = "foo bar";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 0);
    assert_eq!(r.start_index, 6);
    assert_eq!(r.matched_text, "foo bar");
}

#[test]
fn level_1_extra_spaces() {
    let content = "hello foo  bar world";
    let search = "foo bar";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 1);
    assert_eq!(r.matched_text, "foo  bar");
    assert_eq!(r.start_index, 6);
}

#[test]
fn level_1_tabs_vs_spaces() {
    let content = "function\tfoo(\tbar\t)";
    let search = "function foo( bar )";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 1);
    assert_eq!(r.matched_text, "function\tfoo(\tbar\t)");
}

#[test]
fn level_1_trailing_whitespace() {
    let content = "foo bar  \nbaz";
    let search = "foo bar\nbaz";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 1);
    assert_eq!(r.matched_text, "foo bar  \nbaz");
}

#[test]
fn level_2_different_indentation() {
    let content = "\tif (true) {\n\t\treturn 1;\n\t}";
    let search = "  if (true) {\n    return 1;\n  }";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 2);
    assert_eq!(r.matched_text, "\tif (true) {\n\t\treturn 1;\n\t}");
}

#[test]
fn level_2_preserves_original_indentation() {
    let content = "header\n    foo()\n    bar()\nfooter";
    let search = "  foo()\n  bar()";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 2);
    assert_eq!(r.matched_text, "    foo()\n    bar()");
    let start = r.start_index;
    let end = start + r.matched_text.len();
    let replaced = format!("{}{}{}", &content[..start], "REPLACED", &content[end..]);
    assert_eq!(replaced, "header\nREPLACED\nfooter");
}

#[test]
fn level_3_trailing_whitespace_per_line() {
    let content = "  foo()  \n  bar()  ";
    let search = "  foo()\n  bar()";
    let r = cascading_match(content, search).expect("match");
    assert!(r.level <= 3);
    assert_eq!(r.matched_text, "  foo()  \n  bar()  ");
}

#[test]
fn returns_none_when_all_levels_fail() {
    let content = "completely different content here";
    let search = "nothing matches this at all xyz123";
    assert!(cascading_match(content, search).is_none());
}

#[test]
fn level_0_multiline_exact() {
    let content = "line1\nline2\nline3\nline4";
    let search = "line2\nline3";
    let r = cascading_match(content, search).expect("match");
    assert_eq!(r.level, 0);
    assert_eq!(r.start_index, 6);
}

#[test]
fn find_closest_match_returns_line_and_snippet() {
    let content = "alpha\nbeta\ngamma\ndelta\nepsilon";
    let r = find_closest_match(content, "gamm").expect("close match");
    assert_eq!(r.line, 3);
    assert!(r.snippet.contains("gamma"));
}

#[test]
fn find_closest_match_returns_none_for_unrelated() {
    let content = "aaa\nbbb\nccc";
    assert!(find_closest_match(content, "xyz123completely_unrelated_token").is_none());
}
