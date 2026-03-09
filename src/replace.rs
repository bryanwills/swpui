use crate::types::MatchInfo;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write as _;
use std::path::Path;

pub fn apply_replacements(content: &str, matches: &[MatchInfo], replacement: &str) -> String {
    let mut active: Vec<&MatchInfo> = matches.iter().filter(|m| !m.skip).collect();
    if active.is_empty() {
        return content.to_string();
    }

    // Sort by byte offset descending so we can replace from the end
    active.sort_by(|a, b| b.byte_offset_start.cmp(&a.byte_offset_start));

    let mut result = content.to_string();
    for m in active {
        result.replace_range(m.byte_offset_start..m.byte_offset_end, replacement);
    }
    result
}

pub fn has_overlapping_matches(matches: &[MatchInfo]) -> bool {
    let mut active: Vec<&MatchInfo> = matches.iter().filter(|m| !m.skip).collect();
    active.sort_by_key(|m| m.byte_offset_start);
    active
        .windows(2)
        .any(|w| w[0].byte_offset_end > w[1].byte_offset_start)
}

pub fn write_file_atomically(path: &Path, content: &str) -> anyhow::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}

#[must_use]
pub fn compute_content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub fn is_file_stale(path: &Path, original_hash: u64) -> anyhow::Result<bool> {
    let current_content = std::fs::read_to_string(path)?;
    Ok(compute_content_hash(&current_content) != original_hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types::MatchInfo;

    fn make_match(start: usize, end: usize) -> MatchInfo {
        MatchInfo {
            byte_offset_start: start,
            byte_offset_end: end,
            line_number: 1,
            matched_text: String::new(),
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
        std::fs::write(&path, "original").unwrap();
        let result = write_file_atomically(&path, "replaced");
        assert!(result.is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replaced");
    }

    #[test]
    fn stale_file_detected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "original content").unwrap();
        let hash = compute_content_hash("original content");

        // Modify the file externally
        std::fs::write(&path, "modified content").unwrap();

        assert!(is_file_stale(&path, hash).unwrap());
    }

    #[test]
    fn fresh_file_not_stale() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "original content").unwrap();
        let hash = compute_content_hash("original content");

        assert!(!is_file_stale(&path, hash).unwrap());
    }
}
