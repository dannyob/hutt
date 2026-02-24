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
mod splits;
mod tui;
mod undo;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Load config
    let mut config = config::Config::load()?;

    // Parse CLI flags
    let mut initial_folder = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--no-background-servers" => config.background_servers = false,
            "--background-servers" => config.background_servers = true,
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                std::process::exit(1);
            }
            arg => initial_folder = Some(arg.to_string()),
        }
        i += 1;
    }

    // Determine starting account and its muhome
    let default_idx = config.default_account_index();
    let muhome = config.effective_muhome(default_idx);

    // Determine initial folder: CLI arg > account's inbox > "/Inbox"
    let initial_folder = initial_folder.unwrap_or_else(|| {
        config.accounts.get(default_idx)
            .map(|a| a.folders.inbox.clone())
            .unwrap_or_else(|| "/Inbox".to_string())
    });

    // Ensure mu database exists (auto-init for new accounts)
    if let Some(account) = config.accounts.get(default_idx) {
        mu_client::ensure_mu_database(muhome.as_deref(), &account.maildir).await?;
    }

    // Start mu server
    let mu = mu_client::MuClient::start(muhome.as_deref()).await?;
    let mut app = tui::App::new(mu, config).await?;
    app.current_folder = initial_folder;
    tui::run(app).await
}
