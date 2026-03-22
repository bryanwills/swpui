use std::{
    borrow::Cow,
    cmp::Reverse,
    fs,
    io::{BufReader, Read, Write as _},
    path::Path,
};

use convert_case::{Case, Casing as _};
use sha2::{Digest as _, Sha256};

use crate::types::{MatchInfo, MatchMode};

/// Cases to detect, ordered from least specific to most specific.
const CASES: [Case<'static>; 6] = [
    Case::Flat,
    Case::Snake,
    Case::Camel,
    Case::Kebab,
    Case::Pascal,
    Case::UpperSnake,
];

/// Adjust replacement text according to casing.
#[must_use]
pub fn case_aware_replacement<'a>(matched_text: &str, replacement: &'a str) -> Cow<'a, str> {
    if matched_text.is_empty() || replacement.is_empty() {
        return Cow::Borrowed(replacement);
    }

    let Some(matched_case) = detect_case(matched_text) else {
        return Cow::Borrowed(replacement);
    };

    // detect the replacement's case so that convert_case parses word boundaries correctly
    // before converting to the matched case
    let repl_case = detect_case(replacement);

    // if the matched text is `Flat` but the replacement is in a more specific lowercase case
    // (snake, kebab, camel), respect the replacement's case as-is
    if matched_case == Case::Flat
        && repl_case.is_some_and(|c| matches!(c, Case::Snake | Case::Kebab | Case::Camel))
    {
        return Cow::Borrowed(replacement);
    }

    let converted = if let Some(from_case) = repl_case {
        replacement.from_case(from_case).to_case(matched_case)
    } else {
        replacement.to_case(matched_case)
    };
    if converted == replacement {
        return Cow::Borrowed(replacement);
    }
    Cow::Owned(converted)
}

#[must_use]
pub fn apply_replacements(
    content: &str,
    matches: &[MatchInfo],
    replacement: &str,
    mode: MatchMode,
) -> String {
    let mut active: Vec<&MatchInfo> = matches.iter().filter(|m| !m.skip).collect();
    let mut result = content.to_string();
    if active.is_empty() {
        return result;
    }

    // sort by byte offset in descending order so we can replace from the end
    active.sort_unstable_by_key(|m| Reverse(m.byte_offset_start));

    for m in active {
        let repl = if mode == MatchMode::CaseAware {
            case_aware_replacement(&m.matched_text, replacement)
        } else {
            Cow::Borrowed(replacement)
        };
        result.replace_range(m.byte_offset_start..m.byte_offset_end, &repl);
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

/// Detect the case of a string by trying each case from least to most specific.
fn detect_case(s: &str) -> Option<Case<'static>> {
    CASES.iter().copied().find(|&case| s == s.to_case(case))
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
        let result = apply_replacements(content, &matches, "rust", MatchMode::Literal);
        assert_eq!(result, "hello rust");
    }

    #[test]
    fn apply_multiple_replacements() {
        let content = "foo bar foo baz foo";
        let matches = vec![make_match(0, 3), make_match(8, 11), make_match(16, 19)];
        let result = apply_replacements(content, &matches, "qux", MatchMode::Literal);
        assert_eq!(result, "qux bar qux baz qux");
    }

    #[test]
    fn skipped_matches_are_not_replaced() {
        let content = "foo bar foo";
        let matches = vec![make_match(0, 3), make_skipped_match(8, 11)];
        let result = apply_replacements(content, &matches, "baz", MatchMode::Literal);
        assert_eq!(result, "baz bar foo");
    }

    #[test]
    fn empty_replacement_deletes_text() {
        let content = "hello world";
        let matches = vec![make_match(5, 11)];
        let result = apply_replacements(content, &matches, "", MatchMode::Literal);
        assert_eq!(result, "hello");
    }

    #[test]
    fn no_active_matches_returns_original() {
        let content = "hello world";
        let matches = vec![make_skipped_match(0, 5)];
        let result = apply_replacements(content, &matches, "bye", MatchMode::Literal);
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

    #[test]
    fn case_aware_snake_to_upper_snake() {
        assert_eq!(
            case_aware_replacement("FOO_BAR", "baz_qux").as_ref(),
            "BAZ_QUX"
        );
    }

    #[test]
    fn case_aware_snake_to_kebab() {
        assert_eq!(
            case_aware_replacement("foo-bar", "baz_qux").as_ref(),
            "baz-qux"
        );
    }

    #[test]
    fn case_aware_pascal_to_pascal() {
        assert_eq!(
            case_aware_replacement("FooBar", "BazQux").as_ref(),
            "BazQux"
        );
    }

    #[test]
    fn case_aware_pascal_to_camel() {
        assert_eq!(
            case_aware_replacement("fooBar", "BazQux").as_ref(),
            "bazQux"
        );
    }

    #[test]
    fn case_aware_flat_preserves_snake_replacement() {
        assert_eq!(
            case_aware_replacement("foobar", "baz_qux").as_ref(),
            "baz_qux"
        );
        assert!(matches!(
            case_aware_replacement("foobar", "baz_qux"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn case_aware_flat_to_flat() {
        assert_eq!(
            case_aware_replacement("foobar", "bazqux").as_ref(),
            "bazqux"
        );
    }

    #[test]
    fn case_aware_flat_converts_pascal_replacement() {
        assert_eq!(
            case_aware_replacement("foobar", "BazQux").as_ref(),
            "bazqux"
        );
    }

    #[test]
    fn case_aware_same_case_no_change() {
        assert_eq!(
            case_aware_replacement("foo_bar", "baz_qux").as_ref(),
            "baz_qux"
        );
        assert!(matches!(
            case_aware_replacement("foo_bar", "baz_qux"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn case_aware_empty_matched() {
        assert_eq!(case_aware_replacement("", "bar").as_ref(), "bar");
        assert!(matches!(
            case_aware_replacement("", "bar"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn case_aware_empty_replacement() {
        assert_eq!(case_aware_replacement("Foo", "").as_ref(), "");
        assert!(matches!(
            case_aware_replacement("Foo", ""),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn case_aware_single_word_pascal() {
        assert_eq!(case_aware_replacement("Hello", "world").as_ref(), "World");
    }

    #[test]
    fn case_aware_single_word_upper() {
        assert_eq!(case_aware_replacement("HELLO", "world").as_ref(), "WORLD");
    }

    #[test]
    fn case_aware_apply_replacements() {
        let content = "Hello hello";
        let matches = vec![
            MatchInfo {
                matched_text: "Hello".to_string(),
                ..make_match(0, 5)
            },
            MatchInfo {
                matched_text: "hello".to_string(),
                ..make_match(6, 11)
            },
        ];
        let result = apply_replacements(content, &matches, "world", MatchMode::CaseAware);
        assert_eq!(result, "World world");
    }
}
