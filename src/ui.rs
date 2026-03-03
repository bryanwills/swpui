use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;

pub fn render(_app: &App, frame: &mut Frame) {
    frame.render_widget(Paragraph::new("swp - loading UI..."), frame.area());
}
