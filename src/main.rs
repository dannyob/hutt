mod compose;
mod config;
mod envelope;
mod keymap;
mod links;
mod maildir;
mod mime_render;
mod mu_client;
mod mu_sexp;
mod send;
mod smart_folders;
mod splits;
mod tui;
mod undo;

use anyhow::{bail, Context, Result};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Output format for remote commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Silent,
    Sexp,
    Json,
}

fn print_help() {
    eprintln!(
        "hutt {VERSION} — a fast, keyboard-driven TUI email client

USAGE:
    hutt [OPTIONS] [FOLDER]          Launch the TUI
    hutt send --account=<NAME>       Send an email (headless, for scripts/agents)
    hutt remote <COMMAND> [ARGS]     Send command to a running instance
    hutt r <COMMAND> [ARGS]          (shorthand for remote)
    hutt server [OPTIONS]            Run as mu server proxy (drop-in replacement)
    hutt config path                 Print config file path

OPTIONS:
    -h, --help                  Show this help message
    -V, --version               Print version
    -a, --account <NAME>        Start with a specific account
    --log <PATH>                Write debug log to file (or set HUTT_LOG)
    --conversations             Start in conversations (grouped threads) mode
    --no-conversations          Start in single-message mode
    --background-servers        Spawn background mu servers for prefetch (default)
    --no-background-servers     Disable background mu servers
    --vim                       Vi-style editing in search/input fields
    --no-vim                    Emacs-style editing (default)
    --sexp                      (remote) Print results as S-expressions
    --json                      (remote) Print results as JSON (ndjson)
    --wrapped                   (remote) Wrap output as single object

REMOTE COMMANDS:
    open <MESSAGE-ID>           Open a message by Message-ID
    thread <MESSAGE-ID>         Open a thread by Message-ID
    search <QUERY>              Run a search query
    compose [--to=ADDR] [--subject=TEXT]  Open compose window
    navigate <FOLDER>           Switch to a folder
    open-url <URI>              Open any URI (mid:, message:, mailto:, hutt:)
    quit                        Quit the running instance

    All remote commands accept --account=NAME to target a specific account.

URI SCHEMES:
    mid:<message-id>                         Open message (RFC 2392)
    mid:<message-id>?view=thread             Open thread
    message:<message-id>                     Open message (Apple Mail)
    mailto:addr?subject=text                 Compose (RFC 6068)
    hutt:search?q=<query>[&account=<name>]   Search
    hutt:navigate?folder=<path>[&account=<name>]  Navigate

EXAMPLES:
    hutt                        Open default account inbox
    hutt /Sent                  Open the Sent folder
    hutt -a work /Drafts        Open Drafts on the 'work' account
    hutt r search from:alice    Search in the running instance
    hutt r search --account=work from:alice
    hutt r compose --to=bob@example.com --subject=\"Hello\"
    hutt r open-url 'mid:abc@example.com?view=thread'
    hutt r --json search from:alice     Search and output ndjson
    hutt r --sexp thread abc@host.com   Thread envelopes as sexp
    hutt r --json search q | jq '.path' Extract file paths with jq
    echo 'To: bob@example.com      Send email from CLI (stdin)
    Subject: Hi
    
    Hello!' | hutt send -a work
    hutt send -a work --file=msg.eml  Send email from file
    hutt server                     Interactive mu server proxy
    hutt server --eval '(ping)'    Single command evaluation
    hutt server --muhome ~/.mu/work Select account by muhome

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

OUTPUT FLAGS:
    --sexp                  Print results as S-expressions (one per line)
    --json                  Print results as JSON (ndjson, one per line)
    --wrapped               Wrap output in a single object/list

COMMANDS:
    open <MESSAGE-ID>           Open a message by Message-ID
    thread <MESSAGE-ID>         Open a thread by Message-ID
    search <QUERY>              Run a search query
    compose [--to=ADDR] [--subject=TEXT]  Open compose window
    navigate <FOLDER>           Switch to a folder
    open-url <URI>              Open any URI (mid:, message:, mailto:, hutt:)
    quit                        Quit the running instance

    All commands accept --account=NAME / -a NAME to target a specific account."
    );
}

/// Parse --account=name or --account name from args, returning the value and remaining args.
fn extract_account(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut account = None;
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--account=") {
            account = Some(v.to_string());
        } else if args[i] == "--account" || args[i] == "-a" {
            i += 1;
            if i < args.len() {
                account = Some(args[i].clone());
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    (account, rest)
}

/// Extract --sexp, --json, --wrapped from args, returning (format, wrapped, remaining).
fn extract_output_flags(args: &[String]) -> Result<(OutputFormat, bool, Vec<String>)> {
    let mut format = OutputFormat::Silent;
    let mut wrapped = false;
    let mut rest = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--sexp" => {
                if format == OutputFormat::Json {
                    bail!("--sexp and --json are mutually exclusive");
                }
                format = OutputFormat::Sexp;
            }
            "--json" => {
                if format == OutputFormat::Sexp {
                    bail!("--sexp and --json are mutually exclusive");
                }
                format = OutputFormat::Json;
            }
            "--wrapped" => wrapped = true,
            _ => rest.push(arg.clone()),
        }
    }

    Ok((format, wrapped, rest))
}

fn run_config(args: &[String]) -> Result<()> {
    let sub = args.first().map(|s| s.as_str()).unwrap_or("path");
    match sub {
        "path" => {
            match config::Config::locate() {
                Some(path) => println!("{}", path.display()),
                None => {
                    eprintln!("no config file found");
                    std::process::exit(1);
                }
            }
        }
        "-h" | "--help" | "help" => {
            eprintln!(
                "hutt config — config file utilities

USAGE:
    hutt config path            Print config file path"
            );
        }
        other => bail!("unknown config command: '{}'\nRun 'hutt config --help' for usage", other),
    }
    Ok(())
}

/// Format and print IPC response according to output flags.
fn print_ipc_output(resp: &links::IpcResponse, format: OutputFormat, wrapped: bool) {
    match resp {
        links::IpcResponse::Ok => {
            if wrapped {
                match format {
                    OutputFormat::Sexp => println!("(:found 0)"),
                    OutputFormat::Json => println!("{{\"found\":0}}"),
                    OutputFormat::Silent => {}
                }
            }
        }
        links::IpcResponse::Error { message } => {
            match format {
                OutputFormat::Sexp => {
                    println!(
                        "(:error \"{}\")",
                        message.replace('\\', "\\\\").replace('"', "\\\"")
                    );
                }
                OutputFormat::Json => {
                    let obj = serde_json::json!({"error": message});
                    println!("{}", obj);
                }
                OutputFormat::Silent => {}
            }
        }
        links::IpcResponse::MuFrames { frames } => {
            match format {
                OutputFormat::Silent => {}
                OutputFormat::Sexp => {
                    if wrapped {
                        let joined = frames.join(" ");
                        println!("(:headers ({}) :found {})", joined, frames.len());
                    } else {
                        for frame in frames {
                            println!("{}", frame);
                        }
                    }
                }
                OutputFormat::Json => {
                    if wrapped {
                        let json_vals: Vec<serde_json::Value> = frames
                            .iter()
                            .filter_map(|s| mu_sexp::sexp_to_json(s).ok())
                            .collect();
                        let obj = serde_json::json!({
                            "headers": json_vals,
                            "found": frames.len(),
                        });
                        println!("{}", obj);
                    } else {
                        for frame in frames {
                            if let Ok(json) = mu_sexp::sexp_to_json(frame) {
                                println!("{}", json);
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn run_remote(args: &[String]) -> Result<()> {
    if args.is_empty() {
        print_remote_help();
        std::process::exit(1);
    }

    let (format, wrapped, args) = extract_output_flags(args)?;

    if args.is_empty() {
        print_remote_help();
        std::process::exit(1);
    }

    let cmd = match args[0].as_str() {
        // Open any URI (mid:, message:, mailto:, hutt:)
        "open-url" | "url" => {
            let url = args.get(1).ok_or_else(|| anyhow::anyhow!("open-url requires a URI"))?;
            // Try navigate URLs first (they're not HuttUrls)
            if let Some((folder, account)) = links::parse_navigate_url(url) {
                links::IpcCommand::Navigate { folder, account }
            } else if let Some(parsed) = links::parse_url(url) {
                links::IpcCommand::Open(parsed.into())
            } else {
                bail!("unrecognized URI: {}", url);
            }
        }
        "open" => {
            let (account, rest) = extract_account(&args[1..]);
            let id = rest.first().ok_or_else(|| anyhow::anyhow!("open requires a message-id"))?;
            links::IpcCommand::Open(links::HuttUrlSerde::Message { id: id.clone(), account })
        }
        "thread" => {
            let (account, rest) = extract_account(&args[1..]);
            let id = rest.first().ok_or_else(|| anyhow::anyhow!("thread requires a message-id"))?;
            links::IpcCommand::Open(links::HuttUrlSerde::Thread { id: id.clone(), account })
        }
        "search" => {
            let (account, rest) = extract_account(&args[1..]);
            let query = rest.join(" ");
            if query.is_empty() {
                bail!("search requires a query");
            }
            links::IpcCommand::Open(links::HuttUrlSerde::Search { query, account })
        }
        "compose" => {
            let mut to = String::new();
            let mut subject = String::new();
            let mut account = None;
            for arg in &args[1..] {
                if let Some(v) = arg.strip_prefix("--to=") {
                    to = v.to_string();
                } else if let Some(v) = arg.strip_prefix("--subject=") {
                    subject = v.to_string();
                } else if let Some(v) = arg.strip_prefix("--account=") {
                    account = Some(v.to_string());
                } else {
                    bail!("compose: unknown argument '{}'", arg);
                }
            }
            links::IpcCommand::Open(links::HuttUrlSerde::Compose { to, subject, account })
        }
        "navigate" | "nav" => {
            let (account, rest) = extract_account(&args[1..]);
            let folder = rest.first().ok_or_else(|| anyhow::anyhow!("navigate requires a folder"))?;
            links::IpcCommand::Navigate { folder: folder.clone(), account }
        }
        "quit" => links::IpcCommand::Quit,
        "-h" | "--help" | "help" => {
            print_remote_help();
            return Ok(());
        }
        other => bail!("unknown remote command: '{}'\nRun 'hutt remote --help' for usage", other),
    };

    let resp = links::send_ipc_command(&cmd).await?;

    // Print structured output if requested
    print_ipc_output(&resp, format, wrapped);

    match &resp {
        links::IpcResponse::Error { message } => {
            if format == OutputFormat::Silent {
                bail!("hutt: {}", message);
            }
            // Already printed structured error above
            std::process::exit(1);
        }
        _ => Ok(()),
    }
}

fn print_server_help() {
    eprintln!(
        "hutt server — mu server proxy (drop-in replacement for mu server)

USAGE:
    hutt server [OPTIONS]

OPTIONS:
    -h, --help              Show help
    --commands              List available mu commands
    --eval TEXT             Evaluate a single mu server expression
    --allow-temp-file       Accepted for compatibility (ignored)
    --muhome <dir>          Select account by muhome path
    --account <name>        Select account by name
    -a <name>               (same as --account)

When hutt is running, proxies through its mu server. Falls back
to standalone mu server otherwise."
    );
}

async fn run_server(args: &[String]) -> Result<()> {
    let mut muhome: Option<String> = None;
    let mut account: Option<String> = None;
    let mut eval: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_server_help();
                return Ok(());
            }
            "--commands" => {
                println!("commands: compose contacts find index move ping quit remove");
                return Ok(());
            }
            "--eval" => {
                i += 1;
                eval = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--eval requires TEXT"))?
                        .clone(),
                );
            }
            "--allow-temp-file" => {}
            "--muhome" => {
                i += 1;
                muhome = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--muhome requires a path"))?
                        .clone(),
                );
            }
            "--account" | "-a" => {
                i += 1;
                account = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--account requires a name"))?
                        .clone(),
                );
            }
            other => {
                eprintln!("Unknown option: {}", other);
                print_server_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let hutt_available = links::socket_path().exists();

    if let Some(ref eval_sexp) = eval {
        if hutt_available {
            return run_server_eval(eval_sexp, account, muhome).await;
        }
        return run_mu_fallback(args).await;
    }

    if hutt_available {
        run_server_interactive(account, muhome).await
    } else {
        run_mu_fallback(args).await
    }
}

async fn run_server_eval(
    sexp: &str,
    account: Option<String>,
    muhome: Option<String>,
) -> Result<()> {
    let cmd = links::IpcCommand::MuCommand {
        sexp: sexp.to_string(),
        account,
        muhome,
    };
    let resp = links::send_ipc_command(&cmd).await?;
    match resp {
        links::IpcResponse::MuFrames { frames } => {
            use std::io::Write;
            for frame in &frames {
                let encoded = mu_sexp::encode_frame(frame);
                std::io::stdout().write_all(&encoded)?;
            }
            std::io::stdout().flush()?;
            Ok(())
        }
        links::IpcResponse::Error { message } => bail!("{}", message),
        links::IpcResponse::Ok => Ok(()),
    }
}

async fn run_server_interactive(
    account: Option<String>,
    muhome: Option<String>,
) -> Result<()> {
    use std::io::{BufRead, Write};

    // Synthetic pong greeting
    let greeting = mu_sexp::encode_frame(
        &format!("(:pong \"hutt\" :props (:version \"{}\"))", VERSION),
    );
    std::io::stdout().write_all(&greeting)?;
    std::io::stdout().flush()?;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        let sexp = line.trim().to_string();
        if sexp.is_empty() {
            continue;
        }
        if sexp == "(quit)" {
            break;
        }

        let cmd = links::IpcCommand::MuCommand {
            sexp,
            account: account.clone(),
            muhome: muhome.clone(),
        };

        let write_error = |msg: &str| -> Result<()> {
            let err_sexp = format!(
                "(:error 1 :message \"{}\")",
                msg.replace('\\', "\\\\").replace('"', "\\\"")
            );
            let encoded = mu_sexp::encode_frame(&err_sexp);
            std::io::stdout().write_all(&encoded)?;
            std::io::stdout().flush()?;
            Ok(())
        };

        match links::send_ipc_command(&cmd).await {
            Ok(resp) => match resp {
                links::IpcResponse::MuFrames { frames } => {
                    for frame in &frames {
                        let encoded = mu_sexp::encode_frame(frame);
                        std::io::stdout().write_all(&encoded)?;
                    }
                    std::io::stdout().flush()?;
                }
                links::IpcResponse::Error { message } => {
                    write_error(&message)?;
                }
                links::IpcResponse::Ok => {}
            },
            Err(e) => {
                write_error(&e.to_string())?;
            }
        }
    }

    Ok(())
}

async fn run_mu_fallback(args: &[String]) -> Result<()> {
    let mut mu_args = vec!["server".to_string()];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--account" | "-a" => { i += 1; } // skip (not a mu flag)
            other => mu_args.push(other.to_string()),
        }
        i += 1;
    }

    let status = tokio::process::Command::new("mu")
        .args(&mu_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await
        .context("failed to run mu server")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// hutt send — headless email sending from CLI / scripts / agents
// ---------------------------------------------------------------------------

fn print_send_help() {
    eprintln!(
        "hutt send — send an email from the command line

USAGE:
    hutt send --account=<NAME> [--file=<PATH>]
    <message> | hutt send --account=<NAME>

The message uses the same format as the compose editor: RFC 2822-style
headers (To, Subject, Cc, etc.) separated from the body by a blank line.
The From header is auto-filled from the account config if not provided.

OPTIONS:
    -a, --account <NAME>    Account to send from (required)
    --file <PATH>           Read message from file (default: stdin)
    --no-save               Don't save a copy to the Sent folder
    -h, --help              Show this help message

EXAMPLES:
    echo 'To: bob@example.com
    Subject: Hello

    Hi Bob!' | hutt send -a work

    hutt send --account=personal --file=message.eml"
    );
}

async fn run_send(args: &[String], config: &config::Config) -> Result<()> {
    let mut account_name: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut save_to_sent = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_send_help();
                return Ok(());
            }
            "-a" => {
                i += 1;
                account_name = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("-a requires an account name"))?
                        .clone(),
                );
            }
            "--no-save" => {
                save_to_sent = false;
            }
            arg if arg.starts_with("--account=") => {
                account_name = Some(arg.strip_prefix("--account=").unwrap().to_string());
            }
            arg if arg.starts_with("--file=") => {
                file_path = Some(arg.strip_prefix("--file=").unwrap().to_string());
            }
            "--file" => {
                i += 1;
                file_path = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--file requires a path"))?
                        .clone(),
                );
            }
            "--account" => {
                i += 1;
                account_name = Some(
                    args.get(i)
                        .ok_or_else(|| anyhow::anyhow!("--account requires a name"))?
                        .clone(),
                );
            }
            other => bail!("hutt send: unknown argument '{}'\nRun 'hutt send --help' for usage", other),
        }
        i += 1;
    }

    let account_name = account_name
        .ok_or_else(|| {
            let names: Vec<&str> = config.accounts.iter().map(|a| a.name.as_str()).collect();
            anyhow::anyhow!(
                "hutt send requires --account=<NAME>\nAvailable accounts: {}",
                names.join(", ")
            )
        })?;

    let account = config
        .accounts
        .iter()
        .find(|a| a.name == account_name)
        .ok_or_else(|| {
            let names: Vec<&str> = config.accounts.iter().map(|a| a.name.as_str()).collect();
            anyhow::anyhow!(
                "unknown account '{}'. Available: {}",
                account_name,
                names.join(", ")
            )
        })?;

    // Read the message content
    let raw_message = if let Some(ref path) = file_path {
        std::fs::read_to_string(path)
            .with_context(|| format!("failed to read message file: {}", path))?
    } else {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read message from stdin")?;
        buf
    };

    if raw_message.trim().is_empty() {
        bail!("empty message");
    }

    // Auto-fill From: header if not present
    let message = if !raw_message
        .lines()
        .take_while(|l| !l.is_empty())
        .any(|l| l.to_lowercase().starts_with("from:"))
    {
        format!("From: {}\n{}", account.email, raw_message)
    } else {
        raw_message
    };

    // Send via SMTP
    let formatted = send::send_message(&message, &account.smtp)
        .await
        .context("failed to send message")?;

    // Save to Sent folder
    if save_to_sent {
        if let Err(e) = maildir::save_to_sent(&account.maildir, &account.folders.sent, &formatted) {
            eprintln!("Warning: sent but failed to save to Sent folder: {}", e);
        }
    }

    eprintln!("Message sent via {}", account_name);
    Ok(())
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
            // Config subcommand
            "config" => {
                return run_config(&args[i + 1..]);
            }
            // Server subcommand (drop-in mu server replacement)
            "server" => {
                return run_server(&args[i + 1..]).await;
            }
            // Send subcommand (headless email sending)
            "send" => {
                return run_send(&args[i + 1..], &config).await;
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
            // Vim mode for input fields
            "--vim" => config.vim_mode = true,
            "--no-vim" => config.vim_mode = false,
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
