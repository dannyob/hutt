mod envelope;
mod keymap;
mod mime_render;
mod mu_client;
mod mu_sexp;
mod tui;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Optional: first arg is initial folder or query
    let initial_folder = if args.len() > 1 {
        args[1].clone()
    } else {
        "/Inbox".to_string()
    };

    // Start mu server
    let mu = mu_client::MuClient::start().await?;
    let mut app = tui::App::new(mu).await?;
    app.current_folder = initial_folder;
    tui::run(app).await
}
