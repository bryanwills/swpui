use swpui::app::App;

fn main() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    ratatui::run(|term| App::new(root)?.run(term))?;
    Ok(())
}
