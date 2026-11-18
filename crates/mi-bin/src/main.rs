use std::io::Write;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use colored::Colorize;
use futures::StreamExt;
use tokio::sync::RwLock;

use mi_core::config::{MiConfig, ensure_mi_dir, mi_token_path};
use mi_core::llm::gemini::GeminiProvider;
use mi_core::llm::types::Message;
use mi_core::secrets;
use mi_core::server::ws::{self, AppState};

#[derive(Parser)]
#[command(name = "mi", about = "mi — your system-level assistant", version = mi_core::VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// One-shot question, streams the answer
    Ask {
        /// The question to ask mi
        question: String,
    },
    /// Interactive chat REPL
    Chat,
    /// Check configuration, API key, and model availability
    Doctor {
        /// Auto-fix: pick models if none configured
        #[arg(long)]
        fix: bool,
    },
    /// Start the WebSocket server
    Serve,
    /// Show version
    Version,
    /// Manage authentication
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },
}

#[derive(Subcommand)]
enum AuthCommands {
    /// Store your Gemini API key in the OS keyring
    SetGeminiKey {
        /// The API key (if omitted, you'll be prompted)
        key: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = ensure_mi_dir() {
        eprintln!("{} failed to create ~/.mi: {}", "error:".red().bold(), e);
        std::process::exit(1);
    }

    match cli.command {
        Some(Commands::Ask { question }) => cmd_ask(&question).await,
        Some(Commands::Chat) => cmd_chat().await,
        Some(Commands::Doctor { fix }) => cmd_doctor(fix).await,
        Some(Commands::Serve) => cmd_serve().await,
        Some(Commands::Version) => cmd_version(),
        Some(Commands::Auth { command }) => match command {
            AuthCommands::SetGeminiKey { key } => cmd_set_gemini_key(key),
        },
        None => cmd_version(),
    }
}

fn cmd_version() {
    println!("mi v{}", mi_core::VERSION);
}

fn cmd_set_gemini_key(provided_key: Option<String>) {
    let key = if let Some(k) = provided_key {
        k
    } else {
        print!("Paste your Gemini API key: ");
        std::io::stdout().flush().unwrap();
        rpassword::read_password().unwrap_or_default()
    };
    let key = key.trim();
    if key.is_empty() {
        eprintln!("{} no key provided", "error:".red().bold());
        std::process::exit(1);
    }
    match secrets::set_gemini_key(key) {
        Ok(()) => println!("{} Gemini API key saved to OS keyring.", "ok:".green().bold()),
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}

fn get_key_or_exit() -> String {
    match secrets::get_gemini_key() {
        Ok(Some(key)) if !key.is_empty() => key,
        _ => {
            eprintln!(
                "{} No Gemini API key found. Run: {}",
                "error:".red().bold(),
                "mi auth set-gemini-key".cyan()
            );
            std::process::exit(1);
        }
    }
}

fn get_model_or_exit(config: &MiConfig) -> String {
    let model = config.active_model();
    if model.is_empty() {
        eprintln!(
            "{} No model configured. Run: {}",
            "error:".red().bold(),
            "mi doctor --fix".cyan()
        );
        std::process::exit(1);
    }
    model.to_string()
}

async fn cmd_ask(question: &str) {
    let api_key = get_key_or_exit();
    let config = MiConfig::load().unwrap_or_default();
    let model = get_model_or_exit(&config);

    let provider = GeminiProvider::new(api_key);
    let messages = vec![Message::user(question)];
    let mut stream = provider.stream(&messages, &model, None, None);

    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => {
                if chunk.done {
                    break;
                }
                if !chunk.text.is_empty() {
                    print!("{}", chunk.text);
                    std::io::stdout().flush().unwrap();
                }
            }
            Err(e) => {
                eprintln!("\n{} {}", "error:".red().bold(), e);
                std::process::exit(1);
            }
        }
    }
    println!();
}

async fn cmd_chat() {
    let api_key = get_key_or_exit();
    let config = MiConfig::load().unwrap_or_default();
    let model = get_model_or_exit(&config);

    println!(
        "{} (type {} to quit)",
        "mi chat".cyan().bold(),
        "exit".yellow()
    );

    let provider = GeminiProvider::new(api_key);
    let mut history: Vec<Message> = Vec::new();

    loop {
        print!("{} ", "you:".green().bold());
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            break;
        }

        history.push(Message::user(input));

        print!("{} ", "mi:".cyan().bold());
        std::io::stdout().flush().unwrap();

        let mut stream = provider.stream(&history, &model, None, None);
        let mut response_text = String::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(chunk) => {
                    if chunk.done {
                        break;
                    }
                    if !chunk.text.is_empty() {
                        print!("{}", chunk.text);
                        std::io::stdout().flush().unwrap();
                        response_text.push_str(&chunk.text);
                    }
                }
                Err(e) => {
                    eprintln!("\n{} {}", "error:".red().bold(), e);
                    break;
                }
            }
        }
        println!();

        if !response_text.is_empty() {
            history.push(Message::assistant(&response_text));
        }
    }
}

async fn cmd_doctor(fix: bool) {
    println!("{}", "mi doctor".cyan().bold());
    println!();

    // Check API key
    let api_key = match secrets::get_gemini_key() {
        Ok(Some(key)) if !key.is_empty() => {
            println!("  {} Gemini API key found in keyring", "✓".green());
            key
        }
        _ => {
            println!(
                "  {} No Gemini API key. Run: {}",
                "✗".red(),
                "mi auth set-gemini-key".cyan()
            );
            return;
        }
    };

    // Check models
    let mut config = MiConfig::load().unwrap_or_default();
    let provider = GeminiProvider::new(api_key);

    match provider.list_models().await {
        Ok(models) => {
            let generate_models: Vec<_> = models
                .iter()
                .filter(|m| m.supported_methods.contains(&"generateContent".to_string()))
                .collect();
            println!(
                "  {} {} models available ({} support generateContent)",
                "✓".green(),
                models.len(),
                generate_models.len()
            );

            // Show some models
            println!();
            println!("  {}:", "Models supporting generateContent".bold());
            for m in &generate_models {
                let limits = match (m.input_token_limit, m.output_token_limit) {
                    (Some(i), Some(o)) => format!(" (in: {}k, out: {}k)", i / 1000, o / 1000),
                    _ => String::new(),
                };
                println!("    {}{}", m.id, limits.dimmed());
            }

            // Check configured model
            let current = config.active_model().to_string();
            if current.is_empty() {
                println!();
                println!(
                    "  {} No model configured in [models]",
                    "✗".red()
                );
                if fix {
                    // Pick a sensible default
                    let flash = generate_models.iter().find(|m| m.id.contains("flash") && !m.id.contains("lite") && !m.id.contains("preview"));
                    if let Some(m) = flash {
                        config.models.default = m.id.clone();
                        if let Err(e) = config.save() {
                            eprintln!("  {} Could not save config: {}", "✗".red(), e);
                        } else {
                            println!(
                                "  {} Set default model to {}",
                                "✓".green(),
                                m.id.cyan()
                            );
                        }
                    } else {
                        println!("  {} Could not auto-pick a model. Set [models].default in ~/.mi/mi.toml", "!".yellow());
                    }
                } else {
                    println!("  Run {} to auto-pick", "mi doctor --fix".cyan());
                }
            } else {
                let exists = generate_models.iter().any(|m| m.id == current);
                if exists {
                    println!();
                    println!(
                        "  {} Configured model {} is valid",
                        "✓".green(),
                        current.cyan()
                    );
                } else {
                    println!();
                    println!(
                        "  {} Configured model {} not found",
                        "✗".red(),
                        current.cyan()
                    );
                    if fix {
                        let flash = generate_models.iter().find(|m| m.id.contains("flash") && !m.id.contains("lite") && !m.id.contains("preview"));
                        if let Some(m) = flash {
                            config.models.default = m.id.clone();
                            if let Err(e) = config.save() {
                                eprintln!("  {} Could not save config: {}", "✗".red(), e);
                            } else {
                                println!(
                                    "  {} Updated default model to {}",
                                    "✓".green(),
                                    m.id.cyan()
                                );
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!(
                "  {} Could not list models: {}",
                "✗".red(),
                e
            );
        }
    }
}

async fn cmd_serve() {
    let config = MiConfig::load().unwrap_or_default();
    let token = ws::generate_token();

    // Write token to ~/.mi/token
    if let Err(e) = std::fs::write(mi_token_path(), &token) {
        eprintln!("{} could not write token: {}", "error:".red().bold(), e);
        std::process::exit(1);
    }

    let state = AppState {
        config: Arc::new(RwLock::new(config.clone())),
        token: token.clone(),
        session_id: uuid::Uuid::new_v4().to_string(),
    };

    let app = ws::create_app(state);
    let addr = format!("{}:{}", config.core.host, config.core.port);

    println!(
        "{} listening on ws://{}",
        "mi server".cyan().bold(),
        addr
    );
    println!("  token: {}", token.dimmed());

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
