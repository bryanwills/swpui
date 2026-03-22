#![expect(clippy::unwrap_used)]

use std::{fs, io::Write as _, sync::atomic::AtomicUsize};

use swpui::{
    replace::{apply_replacements, hash_file, is_file_stale, write_file},
    search::find_matches_in_content,
    types::MatchMode,
};

fn create_test_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    for (name, content) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }
    dir
}

#[test]
fn full_search_and_replace_workflow() {
    let dir = create_test_dir(&[("hello.txt", "hello world\nhello rust\n")]);
    let path = dir.path().join("hello.txt");
    let content = fs::read_to_string(&path).unwrap();

    // Search
    let matches = find_matches_in_content(
        &content,
        "hello",
        MatchMode::Literal,
        &AtomicUsize::new(0),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(matches.len(), 2);

    // Apply replacements
    let new_content = apply_replacements(&content, &matches, "hi", MatchMode::Literal);
    assert_eq!(new_content, "hi world\nhi rust\n");

    // Write atomically
    write_file(&path, &new_content).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "hi world\nhi rust\n");
}

#[test]
fn stale_file_prevents_write() {
    let dir = create_test_dir(&[("test.txt", "original content\n")]);
    let path = dir.path().join("test.txt");
    let hash = hash_file(&path).unwrap();

    // Modify externally
    fs::write(&path, "someone else changed this\n").unwrap();

    assert!(is_file_stale(&path, hash).unwrap());
}

#[test]
fn regex_search_and_replace() {
    let content = "foo123 bar456 foo789\n";
    let matches = find_matches_in_content(
        content,
        r"foo\d+",
        MatchMode::Regex,
        &AtomicUsize::new(0),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(matches.len(), 2);
    let new_content = apply_replacements(content, &matches, "replaced", MatchMode::Regex);
    assert_eq!(new_content, "replaced bar456 replaced\n");
}

#[test]
fn skip_preserves_matches() {
    let content = "aaa bbb aaa\n";
    let mut matches = find_matches_in_content(
        content,
        "aaa",
        MatchMode::Literal,
        &AtomicUsize::new(0),
        usize::MAX,
    )
    .unwrap();
    assert_eq!(matches.len(), 2);

    // Skip the first match
    matches[0].skip = true;
    let new_content = apply_replacements(content, &matches, "ccc", MatchMode::Literal);
    assert_eq!(new_content, "aaa bbb ccc\n");
}
