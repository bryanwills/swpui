pub mod app;
pub mod input;
pub mod replace;
pub mod search;
pub mod types;
pub mod ui;

fn main() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    ratatui::run(|term| app::App::new(root).run(term))?;
    Ok(())
}
