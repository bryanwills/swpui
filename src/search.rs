use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender},
    },
};

use ignore::{WalkBuilder, WalkState};

use crate::{
    replace::hash_content,
    types::{ContextLine, FileMatches, MatchInfo, MatchMode, SearchRequest, SearchResult},
};

pub const CONTEXT_LINES: usize = 3;

pub fn find_matches_in_content(
    content: &str,
    pattern: &str,
    mode: MatchMode,
) -> anyhow::Result<Vec<MatchInfo>> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }

    let byte_ranges: Vec<(usize, usize)> = match mode {
        MatchMode::Literal => memchr::memmem::find_iter(content.as_bytes(), pattern.as_bytes())
            .map(|pos| (pos, pos + pattern.len()))
            .collect(),
        MatchMode::Regex => {
            let re = regex::Regex::new(pattern)?;
            re.find_iter(content)
                .map(|m| (m.start(), m.end()))
                .collect()
        }
    };

    let mut line_starts: Vec<usize> = std::iter::once(0)
        .chain(memchr::memchr_iter(b'\n', content.as_bytes()).map(|i| i + 1))
        .collect();
    // remove trailing empty line
    if line_starts.last() == Some(&content.len()) {
        line_starts.pop();
    }
    let num_lines = line_starts.len();

    let get_line = |idx: usize| -> &str {
        let start = line_starts[idx];
        let end = line_starts.get(idx + 1).map_or(content.len(), |&s| s - 1);
        content[start..end].trim_end_matches('\n')
    };

    let mut matches = Vec::with_capacity(byte_ranges.len());
    let mut line_idx = 0;
    for (byte_start, byte_end) in byte_ranges {
        while line_starts
            .get(line_idx + 1)
            .is_some_and(|&offset| offset <= byte_start)
        {
            line_idx += 1;
        }
        let line_number = line_idx + 1;

        let context_before: Vec<ContextLine> = (line_idx.saturating_sub(CONTEXT_LINES)..line_idx)
            .map(|i| ContextLine {
                line_number: i + 1,
                content: get_line(i).to_string(),
            })
            .collect();

        let context_after: Vec<ContextLine> = ((line_idx + 1)
            ..=(line_idx + CONTEXT_LINES).min(num_lines.saturating_sub(1)))
            .map(|i| ContextLine {
                line_number: i + 1,
                content: get_line(i).to_string(),
            })
            .collect();

        let line_start_byte = line_starts[line_idx];
        let line_str = get_line(line_idx);
        let match_col_start = byte_start - line_start_byte;
        let match_col_end = (byte_end - line_start_byte).min(line_str.len());

        matches.push(MatchInfo {
            byte_offset_start: byte_start,
            byte_offset_end: byte_end,
            line_number,
            matched_text: content[byte_start..byte_end].to_string(),
            line_content: line_str.to_string(),
            match_col_start,
            match_col_end,
            context_before,
            context_after,
            skip: false,
        });
    }

    Ok(matches)
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
        if request.mode == MatchMode::Regex
            && let Err(e) = regex::Regex::new(&request.pattern)
        {
            let _ = self
                .result_tx
                .send(SearchResult::Error(request.generation, e.to_string()));
            return;
        }

        let walker = WalkBuilder::new(&self.root)
            .filter_entry(|entry| {
                !(entry.path().is_dir() && entry.path().file_name().unwrap_or_default() == ".git")
            })
            .hidden(false)
            .build_parallel();
        let cancelled = &self.cancelled;
        let result_tx = &self.result_tx;
        walker.run(|| {
            let result_tx = result_tx.clone();
            Box::new(move |entry| {
                if cancelled.load(Ordering::Relaxed) {
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

                let Ok(matches) = find_matches_in_content(&content, &request.pattern, request.mode)
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
                    .send(SearchResult::FileMatches(request.generation, file_matches))
                    .is_err()
                {
                    return WalkState::Quit;
                }

                WalkState::Continue
            })
        });
        let _ = self
            .result_tx
            .send(SearchResult::Complete(request.generation));
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used)]
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
                SearchResult::FileMatches(generation, fm) => {
                    assert_eq!(generation, 1);
                    assert_eq!(fm.matches.len(), 2);
                    got_file = true;
                }
                SearchResult::Complete(generation) => {
                    assert_eq!(generation, 1);
                    break;
                }
                SearchResult::Error(_, _) => panic!("unexpected error"),
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

        // Send first request then immediately cancel and send second
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

        // Drain results — we should eventually get Complete(2)
        let mut got_gen2_complete = false;
        loop {
            match result_rx.recv_timeout(Duration::from_secs(2)) {
                Ok(SearchResult::Complete(2)) => {
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
