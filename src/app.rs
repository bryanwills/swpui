use std::{
    fs,
    path::PathBuf,
    slice,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use rat_widget::{
    event::TextOutcome,
    list::ListState,
    scrolled::ScrollState,
    text_input::{self, TextInputState},
};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
};

use crate::{
    replace,
    search::SearchWorker,
    spinner::SpinnerState,
    types::{FileMatches, MatchMode, Pane, SearchRequest, SearchResult},
    ui,
};

const DEBOUNCE: Duration = Duration::from_millis(100);
const POLL_TIMEOUT: Duration = Duration::from_millis(16);

pub struct App {
    pub root: PathBuf,
    pub search_input: TextInputState,
    pub replace_input: TextInputState,
    pub match_mode: MatchMode,
    pub results: Vec<FileMatches>,
    pub focused_pane: Pane,
    pub file_list: ListState,
    pub selected_match: usize,
    pub preview_scroll: ScrollState,
    pub status_message: Option<String>,
    pub searching: bool,
    pub truncated: bool,
    pub spinner: SpinnerState,
    pub confirm_apply_all: bool,
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

        let worker = SearchWorker::new(root.clone(), cmd_rx, result_tx, Arc::clone(&cancelled));
        thread::spawn(move || worker.run());

        Self {
            root,
            search_input: TextInputState::new(),
            replace_input: TextInputState::new(),
            match_mode: MatchMode::default(),
            results: Vec::new(),
            focused_pane: Pane::default(),
            file_list: ListState::default(),
            selected_match: 0,
            preview_scroll: ScrollState::new(),
            status_message: None,
            searching: false,
            truncated: false,
            spinner: SpinnerState::default(),
            confirm_apply_all: false,
            exit: false,
            generation: 0,
            last_keystroke: None,
            pending_search: false,
            cmd_tx,
            result_rx,
            cancelled,
        }
    }

    pub fn selected_file(&self) -> usize {
        self.file_list.selected().unwrap_or_default()
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.poll_events()?;
            self.poll_search_results();
            self.maybe_send_search();
            if self.searching {
                self.spinner.tick();
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        ui::render(self, frame);
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
                SearchResult::FileMatches(generation, file_matches)
                    if generation == self.generation =>
                {
                    self.results.push(file_matches);
                }
                SearchResult::Complete(generation, truncated) if generation == self.generation => {
                    self.results.sort_by(|a, b| a.path.cmp(&b.path));
                    self.searching = false;
                    self.truncated = truncated;
                    if truncated {
                        let total: usize = self.results.iter().map(|fm| fm.matches.len()).sum();
                        self.status_message = Some(format!("Results capped at {total} matches"));
                    }
                }
                SearchResult::Error(generation, msg) if generation == self.generation => {
                    self.status_message = Some(msg);
                    self.searching = false;
                    self.search_input.set_invalid(true);
                }
                SearchResult::FileMatches(..)
                | SearchResult::Complete(..)
                | SearchResult::Error(..) => {}
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
        self.results.clear();
        self.status_message = None;
        self.truncated = false;
        self.search_input.set_invalid(false);
        self.cancelled.store(true, Ordering::Relaxed); // cancel any ongoing search
        self.generation += 1;
        let pattern = self.search_input.text();
        if pattern.is_empty() {
            self.searching = false;
            return;
        }
        self.searching = true;
        self.file_list.select(Some(0));
        self.selected_match = 0;
        self.preview_scroll.clear();
        let _ = self.cmd_tx.send(SearchRequest {
            pattern: pattern.to_string(),
            mode: self.match_mode,
            generation: self.generation,
        });
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Confirmation modal intercepts all keys
        if self.confirm_apply_all {
            match key.code {
                KeyCode::Char('y') => {
                    self.confirm_apply_all = false;
                    self.apply_all();
                }
                _ => self.confirm_apply_all = false,
            }
            return;
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.exit = true;
                return;
            }
            KeyCode::BackTab => {
                self.focused_pane = self.focused_pane.prev();
                while self.searching && !self.focused_pane.is_input() {
                    self.focused_pane = self.focused_pane.prev();
                }
                return;
            }
            KeyCode::Tab => {
                self.focused_pane = self.focused_pane.next();
                while self.searching && !self.focused_pane.is_input() {
                    self.focused_pane = self.focused_pane.next();
                }
                return;
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.match_mode = self.match_mode.toggle();
                if !self.search_input.text().is_empty() {
                    self.dispatch_search();
                }
                return;
            }
            KeyCode::Esc if self.focused_pane.is_input() && !self.searching => {
                self.focused_pane = Pane::FileList;
                return;
            }
            _ => {}
        }

        match self.focused_pane {
            Pane::SearchInput => {
                let outcome =
                    text_input::handle_events(&mut self.search_input, true, &Event::Key(key));
                if outcome == TextOutcome::TextChanged {
                    self.schedule_search();
                }
            }
            Pane::ReplaceInput => {
                text_input::handle_events(&mut self.replace_input, true, &Event::Key(key));
            }
            Pane::FileList if !self.searching => self.handle_file_list_key(key),
            Pane::Preview if !self.searching => self.handle_preview_key(key),
            _ => {}
        }
    }

    fn handle_non_input_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Char('s') => self.toggle_skip_file(),
            KeyCode::Char('a') if !self.results.is_empty() => self.confirm_apply_all = true,
            KeyCode::Char('f') => self.apply_file(),
            _ => {}
        }
    }

    fn handle_file_list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if !self.results.is_empty() => {
                let next = (self.selected_file() + 1).min(self.results.len() - 1);
                self.file_list.select(Some(next));
                self.selected_match = 0;
                self.preview_scroll.clear();
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let prev = self.selected_file().saturating_sub(1);
                self.file_list.select(Some(prev));
                self.selected_match = 0;
                self.preview_scroll.clear();
                return;
            }
            KeyCode::Char('l') | KeyCode::Enter | KeyCode::Right if !self.results.is_empty() => {
                self.focused_pane = Pane::Preview;
                return;
            }
            _ => {}
        }
        self.handle_non_input_key(key);
    }

    fn handle_preview_key(&mut self, key: KeyEvent) {
        let sel = self.selected_file();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(fm) = self.results.get(sel)
                    && !fm.matches.is_empty()
                {
                    self.selected_match = (self.selected_match + 1).min(fm.matches.len() - 1);
                }
                return;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_match = self.selected_match.saturating_sub(1);
                return;
            }
            KeyCode::Char(' ') => {
                if let Some(fm) = self.results.get_mut(sel)
                    && let Some(m) = fm.matches.get_mut(self.selected_match)
                {
                    m.skip = !m.skip;
                }
                return;
            }
            KeyCode::Enter => {
                self.apply_single_match();
                return;
            }
            KeyCode::Char('h') | KeyCode::Esc | KeyCode::Left => {
                self.focused_pane = Pane::FileList;
                return;
            }
            _ => {}
        }
        self.handle_non_input_key(key);
    }

    fn toggle_skip_file(&mut self) {
        let sel = self.selected_file();
        let Some(fm) = self.results.get_mut(sel) else {
            return;
        };
        let all_skipped = fm.matches.iter().all(|m| m.skip);
        for m in &mut fm.matches {
            m.skip = !all_skipped;
        }
    }

    fn apply_all(&mut self) {
        let replacement = self.replace_input.text().to_string();
        let mut indices_to_remove = Vec::with_capacity(self.results.len());
        for (i, fm) in self.results.iter().enumerate() {
            if replace::has_overlapping_matches(&fm.matches) {
                self.status_message = Some(format!(
                    "Overlapping matches in {}, skipping",
                    fm.path.display()
                ));
                continue;
            }
            if let Err(e) = Self::apply_to_file(fm, &replacement) {
                self.status_message = Some(format!("{}: {e}", fm.path.display()));
            } else {
                indices_to_remove.push(i);
            }
        }
        if indices_to_remove.len() == self.results.len() {
            // happy path, all replacements worked
            self.results.clear();
        } else {
            for i in indices_to_remove.into_iter().rev() {
                self.results.swap_remove(i);
            }
        }
        self.clamp_selection();
    }

    fn apply_file(&mut self) {
        let sel = self.selected_file();
        let replacement = self.replace_input.text().to_string();
        let Some(fm) = self.results.get(sel) else {
            return;
        };
        if replace::has_overlapping_matches(&fm.matches) {
            self.status_message = Some(format!("Overlapping matches in {}", fm.path.display()));
            return;
        }
        if let Err(e) = Self::apply_to_file(fm, &replacement) {
            self.status_message = Some(e.to_string());
        } else {
            self.results.remove(sel);
            self.clamp_selection();
        }
    }

    fn apply_single_match(&mut self) {
        let sel = self.selected_file();
        let replacement = self.replace_input.text().to_string();
        let Some(fm) = self.results.get_mut(sel) else {
            return;
        };
        let Some(m) = fm.matches.get(self.selected_match) else {
            return;
        };
        if m.skip {
            return;
        }
        let content = match fs::read_to_string(&fm.path) {
            Ok(c) => c,
            Err(e) => {
                self.status_message = Some(format!("{}: {e}", fm.path.display()));
                return;
            }
        };
        let new_content = replace::apply_replacements(&content, slice::from_ref(m), &replacement);
        if let Err(e) = replace::write_file(&fm.path, &new_content) {
            self.status_message = Some(format!("{}: {e}", fm.path.display()));
            return;
        }
        // remove this match from the results
        // if no matches left, remove the file
        fm.matches.remove(self.selected_match);
        if fm.matches.is_empty() {
            self.results.remove(sel);
        }
        self.clamp_selection();
    }

    fn apply_to_file(fm: &FileMatches, replacement: &str) -> anyhow::Result<()> {
        if replace::is_file_stale(&fm.path, fm.content_hash)? {
            anyhow::bail!("file modified externally, skipping");
        }
        let content = fs::read_to_string(&fm.path)?;
        let new_content = replace::apply_replacements(&content, &fm.matches, replacement);
        replace::write_file(&fm.path, &new_content)?;
        Ok(())
    }

    fn clamp_selection(&mut self) {
        if self.results.is_empty() {
            self.file_list.select(Some(0));
            self.selected_match = 0;
            self.preview_scroll.clear();
            self.focused_pane = Pane::FileList;
        } else {
            let clamped = self.selected_file().min(self.results.len() - 1);
            self.file_list.select(Some(clamped));
            let match_count = self.results[clamped].matches.len();
            self.selected_match = self.selected_match.min(match_count.saturating_sub(1));
        }
    }
}
