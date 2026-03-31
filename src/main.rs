use std::{env, fs::File};

use swpui::app::App;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    if matches!(env::var("DEBUG").as_deref(), Ok("1" | "true")) {
        let log_file = File::create("swpui.log")?;
        tracing_subscriber::fmt()
            .with_writer(log_file)
            .with_ansi(false)
            .with_env_filter(EnvFilter::new("swpui=debug"))
            .init();
    }

    let root = env::current_dir()?;
    ratatui::run(|term| App::new(root)?.run(term))?;
    Ok(())
}
