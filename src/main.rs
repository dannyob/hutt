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

use anyhow::{bail, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    eprintln!(
        "hutt {VERSION} — a fast, keyboard-driven TUI email client

USAGE:
    hutt [OPTIONS] [FOLDER]          Launch the TUI
    hutt remote <COMMAND> [ARGS]     Send command to a running instance
    hutt r <COMMAND> [ARGS]          (shorthand for remote)

OPTIONS:
    -h, --help                  Show this help message
    -V, --version               Print version
    -a, --account <NAME>        Start with a specific account
    --log <PATH>                Write debug log to file (or set HUTT_LOG)
    --conversations             Start in conversations (grouped threads) mode
    --no-conversations          Start in single-message mode
    --background-servers        Spawn background mu servers for prefetch (default)
    --no-background-servers     Disable background mu servers

REMOTE COMMANDS:
    open <MESSAGE-ID>           Open a message by Message-ID
    thread <MESSAGE-ID>         Open a thread by Message-ID
    search <QUERY>              Run a search query
    compose [--to=ADDR] [--subject=TEXT]  Open compose window
    navigate <FOLDER>           Switch to a folder
    quit                        Quit the running instance

EXAMPLES:
    hutt                        Open default account inbox
    hutt /Sent                  Open the Sent folder
    hutt -a work /Drafts        Open Drafts on the 'work' account
    hutt r search from:alice    Search in the running instance
    hutt r compose --to=bob@example.com --subject=\"Hello\"

ENVIRONMENT:
    HUTT_LOG=<path>             Debug log file (same as --log)
    HUTT_CONFIG=<path>          Config file override"
    );
}

fn print_remote_help() {
    eprintln!(
        "hutt remote — send commands to a running hutt instance

USAGE:
    hutt remote <COMMAND> [ARGS]

COMMANDS:
    open <MESSAGE-ID>           Open a message by Message-ID
    thread <MESSAGE-ID>         Open a thread by Message-ID
    search <QUERY>              Run a search query
    compose [--to=ADDR] [--subject=TEXT]  Open compose window
    navigate <FOLDER>           Switch to a folder
    quit                        Quit the running instance"
    );
}

async fn run_remote(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_remote_help();
        std::process::exit(1);
    }

    let cmd = match args[0].as_str() {
        "open" => {
            let id = args.get(1).ok_or_else(|| anyhow::anyhow!("open requires a message-id"))?;
            links::IpcCommand::Open(links::HuttUrlSerde::Message { id: id.clone() })
        }
        "thread" => {
            let id = args.get(1).ok_or_else(|| anyhow::anyhow!("thread requires a message-id"))?;
            links::IpcCommand::Open(links::HuttUrlSerde::Thread { id: id.clone() })
        }
        "search" => {
            let query = args[1..].join(" ");
            if query.is_empty() {
                bail!("search requires a query");
            }
            links::IpcCommand::Open(links::HuttUrlSerde::Search { query })
        }
        "compose" => {
            let mut to = String::new();
            let mut subject = String::new();
            for arg in &args[1..] {
                if let Some(v) = arg.strip_prefix("--to=") {
                    to = v.to_string();
                } else if let Some(v) = arg.strip_prefix("--subject=") {
                    subject = v.to_string();
                } else {
                    bail!("compose: unknown argument '{}'", arg);
                }
            }
            links::IpcCommand::Open(links::HuttUrlSerde::Compose { to, subject })
        }
        "navigate" | "nav" => {
            let folder = args.get(1).ok_or_else(|| anyhow::anyhow!("navigate requires a folder"))?;
            links::IpcCommand::Navigate { folder: folder.clone() }
        }
        "quit" => links::IpcCommand::Quit,
        "-h" | "--help" | "help" => {
            print_remote_help();
            return Ok(());
        }
        other => bail!("unknown remote command: '{}'\nRun 'hutt remote --help' for usage", other),
    };

    links::send_ipc_command(&cmd).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // Load config
    let mut config = config::Config::load()?;

    // Parse CLI flags
    let mut initial_folder = None;
    let mut account_name: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            // Remote subcommand
            "remote" | "r" => {
                return run_remote(&args[i + 1..]).await;
            }
            // Help/version
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("hutt {}", VERSION);
                return Ok(());
            }
            // Account selection
            "-a" | "--account" => {
                i += 1;
                account_name = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--account requires a name"))?
                        .clone(),
                );
            }
            // Log file
            "--log" => {
                i += 1;
                let path = args
                    .get(i)
                    .ok_or_else(|| anyhow::anyhow!("--log requires a path"))?;
                std::env::set_var("HUTT_LOG", path);
            }
            // Conversations mode
            "--conversations" => config.conversations = true,
            "--no-conversations" => config.conversations = false,
            // Background servers
            "--no-background-servers" => config.background_servers = false,
            "--background-servers" => config.background_servers = true,
            // Unknown flag
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {}", arg);
                eprintln!("Run 'hutt --help' for usage");
                std::process::exit(1);
            }
            // Positional: folder path
            arg => initial_folder = Some(arg.to_string()),
        }
        i += 1;
    }

    // Resolve account index
    let default_idx = if let Some(ref name) = account_name {
        config
            .accounts
            .iter()
            .position(|a| a.name == *name)
            .ok_or_else(|| {
                let names: Vec<&str> = config.accounts.iter().map(|a| a.name.as_str()).collect();
                anyhow::anyhow!(
                    "unknown account '{}'. Available: {}",
                    name,
                    names.join(", ")
                )
            })?
    } else {
        config.default_account_index()
    };

    let muhome = config.effective_muhome(default_idx);

    // Determine initial folder: CLI arg > account's inbox > "/Inbox"
    let initial_folder = initial_folder.unwrap_or_else(|| {
        config
            .accounts
            .get(default_idx)
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
    app.active_account = default_idx;
    app.current_folder = initial_folder;
    tui::run(app).await
}
