use crate::types::{ContextLine, FileMatches, MatchInfo, MatchMode};
use std::path::Path;

const CONTEXT_LINES: usize = 3;

pub fn find_matches_in_content(
    content: &str,
    pattern: &str,
    mode: MatchMode,
) -> anyhow::Result<Vec<MatchInfo>> {
    if pattern.is_empty() {
        return Ok(vec![]);
    }

    let byte_ranges: Vec<(usize, usize)> = match mode {
        MatchMode::Literal => {
            let pattern_bytes = pattern.as_bytes();
            let mut ranges = vec![];
            let mut start = 0;
            while let Some(pos) =
                memchr::memmem::find(&content.as_bytes()[start..], pattern_bytes)
            {
                let abs_pos = start + pos;
                ranges.push((abs_pos, abs_pos + pattern_bytes.len()));
                start = abs_pos + pattern_bytes.len();
            }
            ranges
        }
        MatchMode::Regex => {
            let re = regex::Regex::new(pattern)?;
            re.find_iter(content)
                .map(|m| (m.start(), m.end()))
                .collect()
        }
    };

    let lines: Vec<&str> = content.lines().collect();
    let line_byte_offsets: Vec<usize> = std::iter::once(0)
        .chain(content.match_indices('\n').map(|(i, _)| i + 1))
        .collect();

    let mut matches = Vec::with_capacity(byte_ranges.len());
    for (byte_start, byte_end) in byte_ranges {
        let line_idx = line_byte_offsets
            .partition_point(|&offset| offset <= byte_start)
            .saturating_sub(1);
        let line_number = line_idx + 1;

        let context_before: Vec<ContextLine> = (line_idx.saturating_sub(CONTEXT_LINES)..line_idx)
            .filter_map(|i| {
                lines.get(i).map(|content| ContextLine {
                    line_number: i + 1,
                    content: (*content).to_string(),
                })
            })
            .collect();

        let context_after: Vec<ContextLine> = ((line_idx + 1)
            ..=(line_idx + CONTEXT_LINES).min(lines.len().saturating_sub(1)))
            .filter_map(|i| {
                lines.get(i).map(|content| ContextLine {
                    line_number: i + 1,
                    content: (*content).to_string(),
                })
            })
            .collect();

        matches.push(MatchInfo {
            byte_offset_start: byte_start,
            byte_offset_end: byte_end,
            line_number,
            matched_text: content[byte_start..byte_end].to_string(),
            context_before,
            context_after,
            skip: false,
        });
    }

    Ok(matches)
}

pub fn search_directory(dir: &Path, pattern: &str, mode: MatchMode) -> Vec<FileMatches> {
    let walker = ignore::WalkBuilder::new(dir).build();
    let mut results = vec![];
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(matches) = find_matches_in_content(&content, pattern, mode) else {
            continue;
        };
        if !matches.is_empty() {
            results.push(FileMatches {
                path: entry.path().to_path_buf(),
                matches,
            });
        }
    }
    results
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use tempfile::TempDir;

    fn create_test_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
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
    fn literal_search_finds_matches() {
        let content = "line one\nfoo bar\nline three\nfoo again\n";
        let matches = find_matches_in_content(content, "foo", MatchMode::Literal).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line_number, 2);
        assert_eq!(matches[0].matched_text, "foo");
        assert_eq!(matches[1].line_number, 4);
    }

    #[test]
    fn regex_search_finds_matches() {
        let content = "hello world\nhello rust\ngoodbye\n";
        let matches = find_matches_in_content(content, r"hello \w+", MatchMode::Regex).unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "hello world");
        assert_eq!(matches[1].matched_text, "hello rust");
    }

    #[test]
    fn context_lines_are_captured() {
        let content = "a\nb\nc\nmatch\nd\ne\nf\n";
        let matches = find_matches_in_content(content, "match", MatchMode::Literal).unwrap();
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.line_number, 4);
        assert_eq!(m.context_before.len(), 3);
        assert_eq!(m.context_before[0].content, "a");
        assert_eq!(m.context_before[1].content, "b");
        assert_eq!(m.context_before[2].content, "c");
        assert_eq!(m.context_after.len(), 3);
        assert_eq!(m.context_after[0].content, "d");
        assert_eq!(m.context_after[1].content, "e");
        assert_eq!(m.context_after[2].content, "f");
    }

    #[test]
    fn context_lines_at_file_start() {
        let content = "match\na\nb\nc\n";
        let matches = find_matches_in_content(content, "match", MatchMode::Literal).unwrap();
        assert_eq!(matches[0].context_before.len(), 0);
        assert_eq!(matches[0].context_after.len(), 3);
    }

    #[test]
    fn context_lines_at_file_end() {
        let content = "a\nb\nc\nmatch\n";
        let matches = find_matches_in_content(content, "match", MatchMode::Literal).unwrap();
        assert_eq!(matches[0].context_before.len(), 3);
        assert_eq!(matches[0].context_after.len(), 0);
    }

    #[test]
    fn empty_pattern_returns_no_matches() {
        let content = "hello world\n";
        let matches = find_matches_in_content(content, "", MatchMode::Literal).unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn invalid_regex_returns_error() {
        let content = "hello\n";
        let result = find_matches_in_content(content, "[invalid", MatchMode::Regex);
        assert!(result.is_err());
    }

    #[test]
    fn byte_offsets_are_correct() {
        let content = "hello foo world\n";
        let matches = find_matches_in_content(content, "foo", MatchMode::Literal).unwrap();
        assert_eq!(matches[0].byte_offset_start, 6);
        assert_eq!(matches[0].byte_offset_end, 9);
    }

    #[test]
    fn search_walks_directory() {
        let dir = create_test_dir(&[
            ("a.txt", "hello world\n"),
            ("b.txt", "goodbye world\n"),
            ("c.txt", "no match\n"),
        ]);
        let results = search_directory(dir.path(), "world", MatchMode::Literal);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_respects_gitignore() {
        let dir = create_test_dir(&[
            (".gitignore", "ignored.txt\n"),
            ("included.txt", "hello\n"),
            ("ignored.txt", "hello\n"),
        ]);
        // The ignore crate needs a .git directory to recognize the repo root
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let results = search_directory(dir.path(), "hello", MatchMode::Literal);
        assert_eq!(results.len(), 1);
        assert!(results[0].path.ends_with("included.txt"));
    }
}
