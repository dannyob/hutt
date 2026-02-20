mod compose;
mod config;
mod envelope;
mod keymap;
mod links;
mod mime_render;
mod mu_client;
mod mu_sexp;
mod send;
mod smart_folders;
mod tui;
mod undo;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let initial_folder = if args.len() > 1 {
        args[1].clone()
    } else {
        "/Inbox".to_string()
    };

    // Load config
    let config = config::Config::load()?;

    // Start mu server
    let mu = mu_client::MuClient::start().await?;
    let mut app = tui::App::new(mu, config).await?;
    app.current_folder = initial_folder;
    tui::run(app).await
}
