use crate::app::App;

pub mod app;
pub mod db;
pub mod ui;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let app = App::new().with_db().await?;
    let result = app.run(terminal).await;
    ratatui::restore();
    result
}
