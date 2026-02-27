pub mod app;

fn main() -> anyhow::Result<()> {
    ratatui::run(|term| app::App::default().run(term))?;
    Ok(())
}
