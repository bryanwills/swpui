use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rat_widget::text_input::{self, TextInputState};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};

use crate::replace;
use crate::search::SearchWorker;
use crate::types::{FileMatches, MatchMode, Pane, SearchRequest, SearchResult};

const DEBOUNCE: Duration = Duration::from_millis(100);
const POLL_TIMEOUT: Duration = Duration::from_millis(16);

pub struct App {
    pub root: PathBuf,
    pub search_input: TextInputState,
    pub replace_input: TextInputState,
    pub match_mode: MatchMode,
    pub results: Vec<FileMatches>,
    pub focused_pane: Pane,
    pub selected_file: usize,
    pub selected_match: usize,
    pub preview_scroll: u16,
    pub status_message: Option<String>,
    exit: bool,
    generation: u64,
    last_keystroke: Option<Instant>,
    pending_search: bool,
    cmd_tx: mpsc::Sender<SearchRequest>,
    result_rx: mpsc::Receiver<SearchResult>,
    cancelled: Arc<AtomicBool>,
}

impl App {
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));

        let worker = SearchWorker::new(root.clone(), cmd_rx, result_tx, cancelled.clone());
        std::thread::spawn(move || worker.run());

        Self {
            root,
            search_input: TextInputState::new(),
            replace_input: TextInputState::new(),
            match_mode: MatchMode::default(),
            results: vec![],
            focused_pane: Pane::SearchInput,
            selected_file: 0,
            selected_match: 0,
            preview_scroll: 0,
            status_message: None,
            exit: false,
            generation: 0,
            last_keystroke: None,
            pending_search: false,
            cmd_tx,
            result_rx,
            cancelled,
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.poll_events()?;
            self.poll_search_results();
            self.maybe_send_search();
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        crate::ui::render(self, frame);
    }

    fn poll_events(&mut self) -> anyhow::Result<()> {
        if event::poll(POLL_TIMEOUT)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            self.handle_key(key);
        }
        Ok(())
    }

    fn poll_search_results(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                SearchResult::FileMatches(generation, file_matches) => {
                    if generation == self.generation {
                        self.results.push(file_matches);
                    }
                }
                SearchResult::Complete(generation) => {
                    if generation == self.generation {
                        self.results.sort_by(|a, b| a.path.cmp(&b.path));
                    }
                }
                SearchResult::Error(generation, msg) => {
                    if generation == self.generation {
                        self.status_message = Some(msg);
                    }
                }
            }
        }
    }

    fn maybe_send_search(&mut self) {
        if !self.pending_search {
            return;
        }
        if let Some(last) = self.last_keystroke
            && last.elapsed() >= DEBOUNCE
        {
            self.dispatch_search();
            self.pending_search = false;
        }
    }

    fn schedule_search(&mut self) {
        self.last_keystroke = Some(Instant::now());
        self.pending_search = true;
    }

    fn dispatch_search(&mut self) {
        let pattern = self.search_input.text().to_string();
        if pattern.is_empty() {
            self.results.clear();
            self.status_message = None;
            return;
        }
        self.generation += 1;
        self.cancelled.store(true, Ordering::Relaxed);
        self.results.clear();
        self.status_message = None;
        self.selected_file = 0;
        self.selected_match = 0;
        self.preview_scroll = 0;
        let _ = self.cmd_tx.send(SearchRequest {
            pattern,
            mode: self.match_mode,
            generation: self.generation,
        });
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Ctrl-c quits from anywhere
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.exit = true;
            return;
        }

        // Tab / Shift-Tab cycle focus from anywhere
        if key.code == KeyCode::BackTab {
            self.focused_pane = self.focused_pane.prev();
            return;
        }
        if key.code == KeyCode::Tab {
            self.focused_pane = self.focused_pane.next();
            return;
        }

        // Ctrl-r toggles match mode from anywhere
        if key.code == KeyCode::Char('r') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.match_mode = self.match_mode.toggle();
            if !self.search_input.text().is_empty() {
                self.dispatch_search();
            }
            return;
        }

        // Esc in input panes moves focus to file list
        if key.code == KeyCode::Esc && self.focused_pane.is_input() {
            self.focused_pane = Pane::FileList;
            return;
        }

        match self.focused_pane {
            Pane::SearchInput => {
                let outcome =
                    text_input::handle_events(&mut self.search_input, true, &Event::Key(key));
                if outcome == rat_widget::event::TextOutcome::TextChanged {
                    self.schedule_search();
                }
            }
            Pane::ReplaceInput => {
                text_input::handle_events(&mut self.replace_input, true, &Event::Key(key));
            }
            Pane::FileList => self.handle_file_list_key(key),
            Pane::Preview => self.handle_preview_key(key),
        }
    }

    fn handle_file_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Char('j') | KeyCode::Down if !self.results.is_empty() => {
                self.selected_file = (self.selected_file + 1).min(self.results.len() - 1);
                self.selected_match = 0;
                self.preview_scroll = 0;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_file = self.selected_file.saturating_sub(1);
                self.selected_match = 0;
                self.preview_scroll = 0;
            }
            KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right if !self.results.is_empty() => {
                self.focused_pane = Pane::Preview;
            }
            KeyCode::Char('s') => self.toggle_skip_file(),
            KeyCode::Char('a') => self.apply_all(),
            KeyCode::Char('f') => self.apply_file(),
            _ => {}
        }
    }

    fn handle_preview_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(fm) = self.results.get(self.selected_file)
                    && !fm.matches.is_empty()
                {
                    self.selected_match = (self.selected_match + 1).min(fm.matches.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_match = self.selected_match.saturating_sub(1);
            }
            KeyCode::Char(' ') => {
                if let Some(fm) = self.results.get_mut(self.selected_file)
                    && let Some(m) = fm.matches.get_mut(self.selected_match)
                {
                    m.skip = !m.skip;
                }
            }
            KeyCode::Enter => self.apply_single_match(),
            KeyCode::Char('h') | KeyCode::Esc | KeyCode::Left => {
                self.focused_pane = Pane::FileList;
            }
            KeyCode::Char('s') => self.toggle_skip_file(),
            KeyCode::Char('a') => self.apply_all(),
            KeyCode::Char('f') => self.apply_file(),
            _ => {}
        }
    }

    fn toggle_skip_file(&mut self) {
        let Some(fm) = self.results.get_mut(self.selected_file) else {
            return;
        };
        let all_skipped = fm.matches.iter().all(|m| m.skip);
        for m in &mut fm.matches {
            m.skip = !all_skipped;
        }
    }

    fn apply_all(&mut self) {
        let replacement = self.replace_input.text().to_string();
        let mut indices_to_remove = vec![];
        for (i, fm) in self.results.iter().enumerate() {
            if replace::has_overlapping_matches(&fm.matches) {
                self.status_message = Some(format!(
                    "Overlapping matches in {}, skipping",
                    fm.path.display()
                ));
                continue;
            }
            match Self::apply_to_file(fm, &replacement) {
                Ok(()) => indices_to_remove.push(i),
                Err(e) => {
                    self.status_message = Some(format!("{}: {e}", fm.path.display()));
                }
            }
        }
        for i in indices_to_remove.into_iter().rev() {
            self.results.remove(i);
        }
        self.clamp_selection();
    }

    fn apply_file(&mut self) {
        let replacement = self.replace_input.text().to_string();
        let Some(fm) = self.results.get(self.selected_file) else {
            return;
        };
        if replace::has_overlapping_matches(&fm.matches) {
            self.status_message = Some(format!("Overlapping matches in {}", fm.path.display()));
            return;
        }
        match Self::apply_to_file(fm, &replacement) {
            Ok(()) => {
                self.results.remove(self.selected_file);
                self.clamp_selection();
            }
            Err(e) => {
                self.status_message = Some(e.to_string());
            }
        }
    }

    fn apply_single_match(&mut self) {
        let replacement = self.replace_input.text().to_string();
        let Some(fm) = self.results.get(self.selected_file) else {
            return;
        };
        let Some(m) = fm.matches.get(self.selected_match) else {
            return;
        };
        if m.skip {
            return;
        }
        let content = match std::fs::read_to_string(&fm.path) {
            Ok(c) => c,
            Err(e) => {
                self.status_message = Some(format!("{}: {e}", fm.path.display()));
                return;
            }
        };
        let single = vec![m.clone()];
        let new_content = replace::apply_replacements(&content, &single, &replacement);
        if let Err(e) = replace::write_file_atomically(&fm.path, &new_content) {
            self.status_message = Some(format!("{}: {e}", fm.path.display()));
            return;
        }
        // Remove this match from results; if no matches left, remove the file
        let fm = &mut self.results[self.selected_file];
        fm.matches.remove(self.selected_match);
        if fm.matches.is_empty() {
            self.results.remove(self.selected_file);
        }
        self.clamp_selection();
    }

    fn apply_to_file(fm: &FileMatches, replacement: &str) -> anyhow::Result<()> {
        if replace::is_file_stale(&fm.path, fm.content_hash)? {
            anyhow::bail!("file modified externally, skipping");
        }
        let content = std::fs::read_to_string(&fm.path)?;
        let new_content = replace::apply_replacements(&content, &fm.matches, replacement);
        replace::write_file_atomically(&fm.path, &new_content)?;
        Ok(())
    }

    fn clamp_selection(&mut self) {
        if self.results.is_empty() {
            self.selected_file = 0;
            self.selected_match = 0;
            self.preview_scroll = 0;
            self.focused_pane = Pane::FileList;
        } else {
            self.selected_file = self.selected_file.min(self.results.len() - 1);
            let match_count = self.results[self.selected_file].matches.len();
            self.selected_match = self.selected_match.min(match_count.saturating_sub(1));
        }
    }
}
