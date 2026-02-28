pub mod app;
pub mod input;
pub mod search;
pub mod types;

fn main() -> anyhow::Result<()> {
    ratatui::run(|term| app::App::default().run(term))?;
    Ok(())
}
