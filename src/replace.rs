use std::{
    cmp::Reverse,
    fs,
    io::{BufReader, Read, Write as _},
    path::Path,
};

use sha2::{Digest as _, Sha256};

use crate::types::MatchInfo;

#[must_use]
pub fn apply_replacements(content: &str, matches: &[MatchInfo], replacement: &str) -> String {
    let mut active: Vec<&MatchInfo> = matches.iter().filter(|m| !m.skip).collect();
    let mut result = content.to_string();
    if active.is_empty() {
        return result;
    }

    // sort by byte offset in descending order so we can replace from the end
    active.sort_unstable_by_key(|m| Reverse(m.byte_offset_start));

    for m in active {
        result.replace_range(m.byte_offset_start..m.byte_offset_end, replacement);
    }
    result
}

#[must_use]
pub fn has_overlapping_matches(matches: &[MatchInfo]) -> bool {
    let mut active: Vec<&MatchInfo> = matches.iter().filter(|m| !m.skip).collect();
    active.sort_unstable_by_key(|m| m.byte_offset_start);
    active
        .array_windows()
        .any(|[w0, w1]| w0.byte_offset_end > w1.byte_offset_start)
}

pub fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}

/// Hash the contents of a Reader with SHA256
pub fn hash_content<R: Read>(content: &mut R) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut buf = [0; 1024];
    while let Ok(size) = content.read(&mut buf) {
        if size == 0 {
            break;
        }
        hasher.update(&buf[0..size]);
    }
    hasher.finalize().into()
}

pub fn hash_file(path: impl AsRef<Path>) -> anyhow::Result<[u8; 32]> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    Ok(hash_content(&mut reader))
}

pub fn is_file_stale(path: impl AsRef<Path>, original_hash: [u8; 32]) -> anyhow::Result<bool> {
    Ok(hash_file(path)? != original_hash)
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_match(start: usize, end: usize) -> MatchInfo {
        MatchInfo {
            byte_offset_start: start,
            byte_offset_end: end,
            line_number: 1,
            matched_text: String::new(),
            line_content: String::new(),
            match_col_start: 0,
            match_col_end: 0,
            context_before: vec![],
            context_after: vec![],
            skip: false,
        }
    }

    fn make_skipped_match(start: usize, end: usize) -> MatchInfo {
        MatchInfo {
            skip: true,
            ..make_match(start, end)
        }
    }

    #[test]
    fn apply_single_replacement() {
        let content = "hello world";
        let matches = vec![make_match(6, 11)];
        let result = apply_replacements(content, &matches, "rust");
        assert_eq!(result, "hello rust");
    }

    #[test]
    fn apply_multiple_replacements() {
        let content = "foo bar foo baz foo";
        let matches = vec![make_match(0, 3), make_match(8, 11), make_match(16, 19)];
        let result = apply_replacements(content, &matches, "qux");
        assert_eq!(result, "qux bar qux baz qux");
    }

    #[test]
    fn skipped_matches_are_not_replaced() {
        let content = "foo bar foo";
        let matches = vec![make_match(0, 3), make_skipped_match(8, 11)];
        let result = apply_replacements(content, &matches, "baz");
        assert_eq!(result, "baz bar foo");
    }

    #[test]
    fn empty_replacement_deletes_text() {
        let content = "hello world";
        let matches = vec![make_match(5, 11)];
        let result = apply_replacements(content, &matches, "");
        assert_eq!(result, "hello");
    }

    #[test]
    fn no_active_matches_returns_original() {
        let content = "hello world";
        let matches = vec![make_skipped_match(0, 5)];
        let result = apply_replacements(content, &matches, "bye");
        assert_eq!(result, "hello world");
    }

    #[test]
    fn detect_overlapping_matches() {
        let matches = vec![make_match(0, 5), make_match(3, 8)];
        assert!(has_overlapping_matches(&matches));
    }

    #[test]
    fn non_overlapping_matches() {
        let matches = vec![make_match(0, 3), make_match(5, 8)];
        assert!(!has_overlapping_matches(&matches));
    }

    #[test]
    fn adjacent_matches_are_not_overlapping() {
        let matches = vec![make_match(0, 3), make_match(3, 6)];
        assert!(!has_overlapping_matches(&matches));
    }

    #[test]
    fn write_file_atomically_succeeds() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "original").unwrap();
        let result = write_file(&path, "replaced");
        assert!(result.is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), "replaced");
    }

    #[test]
    fn stale_file_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "original content").unwrap();
        let hash = hash_file(&path).unwrap();

        // Modify the file externally
        fs::write(&path, "modified content").unwrap();

        assert!(is_file_stale(&path, hash).unwrap());
    }

    #[test]
    fn fresh_file_not_stale() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, "original content").unwrap();
        let hash = hash_file(&path).unwrap();

        assert!(!is_file_stale(&path, hash).unwrap());
    }
}
