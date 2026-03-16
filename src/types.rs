use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MatchMode {
    #[default]
    Literal,
    Regex,
}

impl MatchMode {
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Literal => Self::Regex,
            Self::Regex => Self::Literal,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ContextLine {
    pub line_number: usize,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct MatchInfo {
    pub byte_offset_start: usize,
    pub byte_offset_end: usize,
    pub line_number: usize,
    pub matched_text: String,
    pub line_content: String,
    pub match_col_start: usize,
    pub match_col_end: usize,
    pub context_before: Vec<ContextLine>,
    pub context_after: Vec<ContextLine>,
    pub skip: bool,
}

#[derive(Debug, Clone)]
pub struct FileMatches {
    pub path: PathBuf,
    pub matches: Vec<MatchInfo>,
    pub content_hash: [u8; 32],
}

impl FileMatches {
    #[must_use]
    pub fn active_match_count(&self) -> usize {
        self.matches.iter().filter(|m| !m.skip).count()
    }
}

pub struct SearchRequest {
    pub pattern: String,
    pub mode: MatchMode,
    pub generation: u64,
}

pub enum SearchResult {
    FileMatches(u64, FileMatches),
    Complete(u64),
    Error(u64, String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pane {
    #[default]
    SearchInput,
    ReplaceInput,
    FileList,
    Preview,
}

impl Pane {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::SearchInput => Self::ReplaceInput,
            Self::ReplaceInput => Self::FileList,
            Self::FileList => Self::Preview,
            Self::Preview => Self::SearchInput,
        }
    }

    #[must_use]
    pub fn prev(self) -> Self {
        match self {
            Self::SearchInput => Self::Preview,
            Self::ReplaceInput => Self::SearchInput,
            Self::FileList => Self::ReplaceInput,
            Self::Preview => Self::FileList,
        }
    }

    #[must_use]
    pub fn is_input(self) -> bool {
        matches!(self, Self::SearchInput | Self::ReplaceInput)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_mode_default_is_literal() {
        assert_eq!(MatchMode::default(), MatchMode::Literal);
    }

    #[test]
    fn match_mode_toggle() {
        let mut mode = MatchMode::Literal;
        mode = mode.toggle();
        assert_eq!(mode, MatchMode::Regex);
        mode = mode.toggle();
        assert_eq!(mode, MatchMode::Literal);
    }

    #[test]
    fn context_line_range() {
        let ctx = ContextLine {
            line_number: 10,
            content: "hello world".to_string(),
        };
        assert_eq!(ctx.line_number, 10);
        assert_eq!(ctx.content, "hello world");
    }

    #[test]
    fn match_info_default_skip_is_false() {
        let m = MatchInfo {
            byte_offset_start: 0,
            byte_offset_end: 5,
            line_number: 1,
            matched_text: "hello".to_string(),
            line_content: "hello world".to_string(),
            match_col_start: 0,
            match_col_end: 5,
            context_before: vec![],
            context_after: vec![],
            skip: false,
        };
        assert!(!m.skip);
    }

    #[test]
    fn file_matches_match_count() {
        let fm = FileMatches {
            path: std::path::PathBuf::from("test.rs"),
            matches: vec![
                MatchInfo {
                    byte_offset_start: 0,
                    byte_offset_end: 3,
                    line_number: 1,
                    matched_text: "foo".to_string(),
                    line_content: "foo bar".to_string(),
                    match_col_start: 0,
                    match_col_end: 3,
                    context_before: vec![],
                    context_after: vec![],
                    skip: false,
                },
                MatchInfo {
                    byte_offset_start: 10,
                    byte_offset_end: 13,
                    line_number: 2,
                    matched_text: "foo".to_string(),
                    line_content: "baz foo qux".to_string(),
                    match_col_start: 4,
                    match_col_end: 7,
                    context_before: vec![],
                    context_after: vec![],
                    skip: true,
                },
            ],
            content_hash: [0; 32],
        };
        assert_eq!(fm.matches.len(), 2);
        assert_eq!(fm.active_match_count(), 1);
    }

    #[test]
    fn pane_cycle_forward() {
        let mut pane = Pane::SearchInput;
        pane = pane.next();
        assert_eq!(pane, Pane::ReplaceInput);
        pane = pane.next();
        assert_eq!(pane, Pane::FileList);
        pane = pane.next();
        assert_eq!(pane, Pane::Preview);
        pane = pane.next();
        assert_eq!(pane, Pane::SearchInput);
    }

    #[test]
    fn pane_cycle_backward() {
        let mut pane = Pane::SearchInput;
        pane = pane.prev();
        assert_eq!(pane, Pane::Preview);
        pane = pane.prev();
        assert_eq!(pane, Pane::FileList);
    }
}
