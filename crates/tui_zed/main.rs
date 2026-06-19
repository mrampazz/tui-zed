use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = tui_zed::app::App::new();

    // Open file if provided as argument
    if let Some(path) = std::env::args().nth(1) {
        app.open_file(std::path::Path::new(&path))?;
    }

    let mut terminal = ratatui::init();
    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}
