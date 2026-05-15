use heddle::tools::string_distance::{find_closest, levenshtein};

mod common;

#[test]
fn identical_strings_distance_zero() {
    assert_eq!(levenshtein("hello", "hello"), 0);
}

#[test]
fn single_char_difference() {
    assert_eq!(levenshtein("cat", "bat"), 1);
    assert_eq!(levenshtein("cat", "ca"), 1);
    assert_eq!(levenshtein("cat", "cats"), 1);
}

#[test]
fn completely_different_strings() {
    assert_eq!(levenshtein("abc", "xyz"), 3);
    assert_eq!(levenshtein("", "hello"), 5);
    assert_eq!(levenshtein("hello", ""), 5);
}

#[test]
fn case_sensitive() {
    assert_eq!(levenshtein("Hello", "hello"), 1);
    assert_eq!(levenshtein("ABC", "abc"), 3);
}

#[test]
fn find_closest_within_max_distance() {
    let candidates: Vec<String> = ["read_file", "write_file", "edit_file"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(find_closest("reed_file", &candidates, 3), Some("read_file"));
    assert_eq!(
        find_closest("writ_file", &candidates, 3),
        Some("write_file")
    );
}

#[test]
fn find_closest_returns_none_when_too_far() {
    let candidates: Vec<String> = ["read_file", "write_file"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        find_closest("completely_different_tool_name", &candidates, 3),
        None
    );
}
