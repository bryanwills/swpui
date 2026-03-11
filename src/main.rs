fn main() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    ratatui::run(|term| swpui::app::App::new(root).run(term))?;
    Ok(())
}
