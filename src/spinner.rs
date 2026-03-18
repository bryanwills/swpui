use ratatui::{buffer::Buffer, layout::Rect, style::Style, text::Span, widgets::StatefulWidget};

const FRAMES: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

#[derive(Debug, Default)]
pub struct SpinnerState {
    tick: usize,
}

impl SpinnerState {
    pub fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    #[must_use]
    pub fn frame(&self) -> char {
        FRAMES[(self.tick / 2) % FRAMES.len()]
    }
}

#[derive(Default)]
pub struct Spinner {
    style: Style,
}

impl Spinner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl StatefulWidget for Spinner {
    type State = SpinnerState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let span = Span::styled(state.frame().to_string(), self.style);
        buf.set_span(area.x, area.y, &span, area.width);
    }
}
