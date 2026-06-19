use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = tui_zed::app::App::new();
    let result = app.run(&mut terminal).await;
    ratatui::restore();
    result
}
