use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{Receiver, Sender},
    },
};

pub const MAX_MATCHES: usize = 100_000;

use ignore::{WalkBuilder, WalkState};
use regex::Regex;

use crate::{
    types::{
        ContextLine, FileMatches, MatchInfo, MatchKind, MatchMode, SearchRequest, SearchResult,
    },
    utils::hash_content,
};

pub const CONTEXT_LINES: usize = 2;

#[derive(Clone, Debug)]
pub enum Pattern<'a> {
    Empty,
    Literal(&'a [u8]),
    Regex(Regex),
}

impl<'a> Pattern<'a> {
    pub fn new(pattern: &'a str, mode: MatchMode) -> anyhow::Result<Self> {
        if pattern.is_empty() {
            return Ok(Pattern::Empty);
        }
        Ok(match mode {
            MatchMode::Literal => Pattern::Literal(pattern.as_bytes()),
            MatchMode::Regex => Pattern::Regex(regex::Regex::new(pattern)?),
            MatchMode::CaseAware => Pattern::Regex(
                regex::RegexBuilder::new(&regex::escape(pattern))
                    .case_insensitive(true)
                    .build()?,
            ),
            MatchMode::RegexMultiline => Pattern::Regex(
                regex::RegexBuilder::new(pattern)
                    .dot_matches_new_line(true)
                    .multi_line(true)
                    .crlf(true)
                    .build()?,
            ),
        })
    }
}

pub fn find_matches_in_content(
    content: &str,
    pattern: &Pattern,
    match_count: &AtomicUsize,
    max_matches: usize,
) -> anyhow::Result<Vec<MatchInfo>> {
    if matches!(pattern, Pattern::Empty) || match_count.load(Ordering::Relaxed) >= max_matches {
        return Ok(Vec::new());
    }

    let mut line_starts: Vec<usize> = std::iter::once(0)
        .chain(memchr::memchr_iter(b'\n', content.as_bytes()).map(|i| i + 1))
        .collect();
    // remove trailing empty line
    if line_starts.last() == Some(&content.len()) {
        line_starts.pop();
    }
    let num_lines = line_starts.len();

    let byte_ranges = find_byte_ranges(content, pattern);

    let mut matches = Vec::new();
    let mut line_idx = 0;
    for (byte_start, byte_end) in byte_ranges {
        if match_count.load(Ordering::Relaxed) >= max_matches {
            break;
        }

        while line_starts
            .get(line_idx + 1)
            .is_some_and(|&offset| offset <= byte_start)
        {
            line_idx += 1;
        }

        matches.push(build_match_info(
            content,
            &line_starts,
            num_lines,
            byte_start,
            byte_end,
            line_idx,
        ));
        match_count.fetch_add(1, Ordering::Relaxed);
    }

    Ok(matches)
}

fn find_byte_ranges(content: &str, pattern: &Pattern) -> Vec<(usize, usize)> {
    match pattern {
        Pattern::Empty => Vec::new(),
        Pattern::Literal(pattern) => memchr::memmem::find_iter(content.as_bytes(), pattern)
            .map(|pos| (pos, pos + pattern.len()))
            .collect(),
        Pattern::Regex(re) => re
            .find_iter(content)
            .map(|m| (m.start(), m.end()))
            .collect(),
    }
}

fn build_match_info(
    content: &str,
    line_starts: &[usize],
    num_lines: usize,
    byte_start: usize,
    byte_end: usize,
    line_idx: usize,
) -> MatchInfo {
    let get_line = |idx: usize| -> &str {
        let start = line_starts[idx];
        let end = line_starts.get(idx + 1).map_or(content.len(), |&s| s - 1);
        content[start..end].trim_end_matches('\n')
    };

    let line_number = line_idx + 1;

    let context_before: Vec<ContextLine> = (line_idx.saturating_sub(CONTEXT_LINES)..line_idx)
        .map(|i| ContextLine {
            line_number: i + 1,
            content: get_line(i).to_string(),
        })
        .collect();

    let line_idx_end = if byte_end - byte_start > 1024 {
        // for large matches: binary search
        line_starts.partition_point(|&s| s < byte_end) - 1
    } else {
        // otherwise, linear search is fine
        line_starts[line_idx + 1..]
            .iter()
            .position(|&s| s >= byte_end)
            .map_or(num_lines - 1, |pos| line_idx + pos)
    };

    let context_after: Vec<ContextLine> = ((line_idx_end + 1)
        ..=(line_idx_end + CONTEXT_LINES).min(num_lines.saturating_sub(1)))
        .map(|i| ContextLine {
            line_number: i + 1,
            content: get_line(i).to_string(),
        })
        .collect();

    let line_start_byte = line_starts[line_idx];
    let last_line_byte = line_starts[line_idx_end];
    let last_line_str = get_line(line_idx_end);
    let match_col_start = byte_start - line_start_byte;
    let match_col_end = (byte_end - last_line_byte).min(last_line_str.len());

    let kind = if line_idx_end == line_idx {
        MatchKind::SingleLine {
            line_number,
            line_content: last_line_str.to_string(),
        }
    } else {
        MatchKind::MultiLine {
            line_number_start: line_idx + 1,
            line_number_end: line_idx_end + 1,
            matched_lines: (line_idx..=line_idx_end)
                .map(|i| get_line(i).to_string())
                .collect(),
        }
    };

    MatchInfo {
        byte_offset_start: byte_start,
        byte_offset_end: byte_end,
        matched_text: content[byte_start..byte_end].to_string(),
        match_col_start,
        match_col_end,
        context_before,
        context_after,
        skip: false,
        kind,
    }
}

pub struct SearchWorker {
    root: PathBuf,
    cmd_rx: Receiver<SearchRequest>,
    result_tx: Sender<SearchResult>,
    cancelled: Arc<AtomicBool>,
}

impl SearchWorker {
    pub fn new(
        root: PathBuf,
        cmd_rx: Receiver<SearchRequest>,
        result_tx: Sender<SearchResult>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            root,
            cmd_rx,
            result_tx,
            cancelled,
        }
    }

    pub fn run(self) {
        while let Ok(mut request) = self.cmd_rx.recv() {
            // skip to the latest queued request in case there are multiple
            // this makes cancellation faster
            while let Ok(newer) = self.cmd_rx.try_recv() {
                request = newer;
            }
            self.cancelled.store(false, Ordering::Relaxed);
            self.execute_search(&request);
        }
    }

    fn execute_search(&self, request: &SearchRequest) {
        // validate regex upfront before walking files
        let pattern = match Pattern::new(&request.pattern, request.mode) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                let _ = self.result_tx.send(SearchResult::Error {
                    generation: request.generation,
                    message: e.to_string(),
                });
                return;
            }
        };

        let walker = WalkBuilder::new(&self.root)
            .filter_entry(|entry| {
                !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
            })
            .hidden(false)
            .build_parallel();
        let cancelled = &self.cancelled;
        let result_tx = &self.result_tx;
        let match_count = &AtomicUsize::new(0);
        walker.run(|| {
            let result_tx = result_tx.clone();
            let pattern = Arc::clone(&pattern);
            Box::new(move |entry| {
                if cancelled.load(Ordering::Relaxed) {
                    return WalkState::Quit;
                }
                if match_count.load(Ordering::Relaxed) >= MAX_MATCHES {
                    return WalkState::Quit;
                }

                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };

                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return WalkState::Continue;
                }

                let Ok(content) = fs::read_to_string(entry.path()) else {
                    return WalkState::Continue;
                };

                let Ok(matches) =
                    find_matches_in_content(&content, &pattern, match_count, MAX_MATCHES)
                else {
                    return WalkState::Continue;
                };

                if matches.is_empty() {
                    return WalkState::Continue;
                }

                let content_hash = hash_content(&mut content.as_bytes());
                let file_matches = FileMatches {
                    path: entry.path().to_path_buf(),
                    matches,
                    content_hash,
                };
                if result_tx
                    .send(SearchResult::FileMatches {
                        generation: request.generation,
                        file_matches,
                    })
                    .is_err()
                {
                    return WalkState::Quit;
                }

                WalkState::Continue
            })
        });
        let truncated = match_count.load(Ordering::Relaxed) >= MAX_MATCHES;
        let _ = self.result_tx.send(SearchResult::Complete {
            generation: request.generation,
            truncated,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write as _, sync::mpsc, thread, time::Duration};
    use tempfile::TempDir;

    fn create_test_dir(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
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
    fn literal_search_finds_matches() {
        let content = "line one\nfoo bar\nline three\nfoo again\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("foo", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "foo");
        assert!(matches!(
            matches[0].kind,
            MatchKind::SingleLine { line_number: 2, .. }
        ));
        assert!(matches!(
            matches[1].kind,
            MatchKind::SingleLine { line_number: 4, .. }
        ));
    }

    #[test]
    fn regex_search_finds_matches() {
        let content = "hello world\nhello rust\ngoodbye\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new(r"hello \w+", MatchMode::Regex).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].matched_text, "hello world");
        assert_eq!(matches[1].matched_text, "hello rust");
    }

    #[test]
    fn context_lines_are_captured() {
        let content = "a\nb\nc\nmatch\nd\ne\nf\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("match", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert!(matches!(
            m.kind,
            MatchKind::SingleLine { line_number: 4, .. }
        ));
        assert_eq!(m.context_before.len(), CONTEXT_LINES);
        assert_eq!(m.context_before[0].content, "b");
        assert_eq!(m.context_before[1].content, "c");
        assert_eq!(m.context_after.len(), CONTEXT_LINES);
        assert_eq!(m.context_after[0].content, "d");
        assert_eq!(m.context_after[1].content, "e");
    }

    #[test]
    fn context_lines_at_file_start() {
        let content = "match\na\nb\nc\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("match", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches[0].context_before.len(), 0);
        assert_eq!(matches[0].context_after.len(), CONTEXT_LINES);
    }

    #[test]
    fn context_lines_at_file_end() {
        let content = "a\nb\nc\nmatch\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("match", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches[0].context_before.len(), CONTEXT_LINES);
        assert_eq!(matches[0].context_after.len(), 0);
    }

    #[test]
    fn empty_pattern_returns_no_matches() {
        let content = "hello world\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert!(matches.is_empty());
    }

    #[test]
    fn invalid_regex_returns_error() {
        assert!(Pattern::new("[invalid", MatchMode::Regex).is_err());
    }

    #[test]
    fn multiline_match_produces_multiline_kind() {
        let content = "foo\nbar\nbaz\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new(r"foo\nbar", MatchMode::RegexMultiline).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches!(
            matches[0].kind,
            MatchKind::MultiLine {
                line_number_start: 1,
                line_number_end: 2,
                ..
            }
        ));
        if let MatchKind::MultiLine { matched_lines, .. } = &matches[0].kind {
            assert_eq!(matched_lines, &["foo", "bar"]);
        }
    }

    #[test]
    fn multiline_match_context_after_uses_end_line() {
        // match spans lines 1-2; context_after should be lines 3+
        let content = "foo\nbar\nbaz\nqux\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new(r"foo\nbar", MatchMode::RegexMultiline).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].context_after.len(), CONTEXT_LINES);
        assert_eq!(matches[0].context_after[0].content, "baz");
        assert_eq!(matches[0].context_after[0].line_number, 3);
    }

    #[test]
    fn single_line_regex_multiline_produces_singleline_kind() {
        let content = "hello world\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("hello", MatchMode::RegexMultiline).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches!(matches[0].kind, MatchKind::SingleLine { .. }));
    }

    #[test]
    fn byte_offsets_are_correct() {
        let content = "hello foo world\n";
        let matches = find_matches_in_content(
            content,
            &Pattern::new("foo", MatchMode::Literal).unwrap(),
            &AtomicUsize::new(0),
            usize::MAX,
        )
        .unwrap();
        assert_eq!(matches[0].byte_offset_start, 6);
        assert_eq!(matches[0].byte_offset_end, 9);
    }

    #[test]
    fn search_worker_sends_results() {
        let dir = create_test_dir(&[("test.txt", "foo bar foo\n")]);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = SearchWorker::new(dir.path().to_path_buf(), cmd_rx, result_tx, cancelled);
        let handle = thread::spawn(move || worker.run());

        cmd_tx
            .send(SearchRequest {
                pattern: "foo".to_string(),
                mode: MatchMode::Literal,
                generation: 1,
            })
            .unwrap();

        let mut got_file = false;
        loop {
            match result_rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                SearchResult::FileMatches {
                    generation,
                    file_matches: fm,
                } => {
                    assert_eq!(generation, 1);
                    assert_eq!(fm.matches.len(), 2);
                    got_file = true;
                }
                SearchResult::Complete { generation, .. } => {
                    assert_eq!(generation, 1);
                    break;
                }
                SearchResult::Error { .. } => panic!("unexpected error"),
            }
        }
        assert!(got_file);

        drop(cmd_tx);
        handle.join().unwrap();
    }

    #[test]
    fn search_worker_cancellation() {
        let dir = create_test_dir(&[
            ("a.txt", "needle\n"),
            ("b.txt", "needle\n"),
            ("c.txt", "needle\n"),
        ]);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = SearchWorker::new(
            dir.path().to_path_buf(),
            cmd_rx,
            result_tx,
            cancelled.clone(),
        );
        let handle = thread::spawn(move || worker.run());

        // send first request then immediately cancel and send second
        cmd_tx
            .send(SearchRequest {
                pattern: "needle".to_string(),
                mode: MatchMode::Literal,
                generation: 1,
            })
            .unwrap();

        cancelled.store(true, Ordering::Relaxed);

        cmd_tx
            .send(SearchRequest {
                pattern: "needle".to_string(),
                mode: MatchMode::Literal,
                generation: 2,
            })
            .unwrap();

        // drain results; we should eventually get Complete(2)
        let mut got_gen2_complete = false;
        loop {
            match result_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(SearchResult::Complete { generation: 2, .. }) => {
                    got_gen2_complete = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(got_gen2_complete);

        drop(cmd_tx);
        handle.join().unwrap();
    }
}
