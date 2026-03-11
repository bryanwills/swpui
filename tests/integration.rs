#![allow(clippy::unwrap_used)]

use std::io::Write as _;
use swpui::replace::{
    apply_replacements, compute_content_hash, is_file_stale, write_file_atomically,
};
use swpui::search::{find_matches_in_content, search_directory};
use swpui::types::MatchMode;

fn create_test_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }
    dir
}

#[test]
fn full_search_and_replace_workflow() {
    let dir = create_test_dir(&[
        ("hello.txt", "hello world\nhello rust\n"),
        ("other.txt", "no match here\n"),
    ]);

    // Search
    let results = search_directory(dir.path(), "hello", MatchMode::Literal);
    assert_eq!(results.len(), 1);
    let fm = &results[0];
    assert_eq!(fm.matches.len(), 2);

    // Apply replacements
    let content = std::fs::read_to_string(&fm.path).unwrap();
    let new_content = apply_replacements(&content, &fm.matches, "hi");
    assert_eq!(new_content, "hi world\nhi rust\n");

    // Write atomically
    write_file_atomically(&fm.path, &new_content).unwrap();
    assert_eq!(
        std::fs::read_to_string(&fm.path).unwrap(),
        "hi world\nhi rust\n"
    );
}

#[test]
fn stale_file_prevents_write() {
    let dir = create_test_dir(&[("test.txt", "original content\n")]);
    let path = dir.path().join("test.txt");
    let hash = compute_content_hash("original content\n");

    // Modify externally
    std::fs::write(&path, "someone else changed this\n").unwrap();

    assert!(is_file_stale(&path, hash).unwrap());
}

#[test]
fn regex_search_and_replace() {
    let content = "foo123 bar456 foo789\n";
    let matches = find_matches_in_content(content, r"foo\d+", MatchMode::Regex).unwrap();
    assert_eq!(matches.len(), 2);
    let new_content = apply_replacements(content, &matches, "replaced");
    assert_eq!(new_content, "replaced bar456 replaced\n");
}

#[test]
fn skip_preserves_matches() {
    let content = "aaa bbb aaa\n";
    let mut matches = find_matches_in_content(content, "aaa", MatchMode::Literal).unwrap();
    assert_eq!(matches.len(), 2);

    // Skip the first match
    matches[0].skip = true;
    let new_content = apply_replacements(content, &matches, "ccc");
    assert_eq!(new_content, "aaa bbb ccc\n");
}
