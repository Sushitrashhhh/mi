use std::io::Write;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use colored::Colorize;
use tokio::sync::RwLock;

use mi_core::agent::loop_runner::{AgentConfig, AgentEvent, AgentLoop, ApprovalCallback, EventCallback};
use mi_core::config::{MiConfig, ensure_mi_dir, mi_db_path, mi_logs_dir, mi_quarantine_dir, mi_token_path};
use mi_core::events::ApprovalDecision;
use mi_core::llm::gemini::GeminiProvider;
use mi_core::llm::types::Message;
use mi_core::llm::AnyProvider;
use mi_core::memory::embed::EmbeddingClient;
use mi_core::memory::extractor::MemoryExtractor;
use mi_core::memory::retrieve::MemoryRetriever;
use mi_core::memory::store::MemoryStore;
use mi_core::permissions::audit::AuditLog;
use mi_core::permissions::policy::{PolicyConfig, PolicyEngine};
use mi_core::secrets;
use mi_core::server::ws::{self, AppState};
use mi_core::tools::builtin;
use mi_core::tools::os_adapt;
use mi_core::tools::registry::ToolRegistry;
use mi_core::tools::ToolExecutor;

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/system.md");

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
    /// View the audit log
    Logs {
        /// Show last N entries
        #[arg(long, default_value = "20")]
        tail: usize,
    },
    /// Manage mi's memory
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
}

#[derive(Subcommand)]
enum MemoryCommands {
    /// List stored memories
    List {
        /// Max number to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Search memories by keyword
    Search {
        /// Search query
        query: String,
        /// Max results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Forget (soft-delete) a memory by ID
    Forget {
        /// Memory ID
        id: String,
    },
    /// Pin/unpin a memory
    Pin {
        /// Memory ID
        id: String,
        /// Unpin instead of pin
        #[arg(long)]
        unpin: bool,
    },
    /// Export all memories as JSON
    Export {
        /// Output file path (default: stdout)
        #[arg(long)]
        output: Option<String>,
    },
    /// Wipe all memories (irreversible!)
    Wipe,
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
        Some(Commands::Logs { tail }) => cmd_logs(tail),
        Some(Commands::Memory { command }) => cmd_memory(command),
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

fn open_memory_store(config: &MiConfig) -> Arc<MemoryStore> {
    Arc::new(
        MemoryStore::open(&mi_db_path(), config.memory.embed_dim)
            .expect("failed to open memory database"),
    )
}

fn create_embedding_client(api_key: &str, config: &MiConfig) -> Arc<EmbeddingClient> {
    let embed_model = if config.models.embed.is_empty() {
        "gemini-embedding-2".to_string()
    } else {
        config.models.embed.clone()
    };
    Arc::new(EmbeddingClient::new(
        api_key,
        &embed_model,
        config.memory.embed_dim,
    ))
}

fn create_agent_loop(api_key: &str, config: &MiConfig, model: &str) -> AgentLoop {
    let provider = Arc::new(AnyProvider::Gemini(GeminiProvider::new(
        api_key.to_string(),
    )));
    let mut registry = ToolRegistry::new();
    builtin::register_all(&mut registry);

    let store = open_memory_store(config);
    let embedder = create_embedding_client(api_key, config);

    let memory_retriever = Arc::new(MemoryRetriever::new(
        store.clone(),
        embedder.clone(),
        config.memory.top_k,
    ));

    let extraction_model = if config.models.fast.is_empty() {
        model.to_string()
    } else {
        config.models.fast.clone()
    };
    let memory_extractor = Arc::new(MemoryExtractor::new(
        provider.clone(),
        extraction_model,
        store,
        embedder,
        config.memory.auto_save.clone(),
    ));

    AgentLoop {
        provider,
        registry: Arc::new(registry),
        executor: Arc::new(ToolExecutor::new(
            os_adapt::create_adapter(),
            mi_quarantine_dir(),
        )),
        policy: Arc::new(PolicyEngine::new(PolicyConfig::load())),
        audit: Arc::new(AuditLog::new(mi_logs_dir().join("audit.jsonl"))),
        config: AgentConfig {
            model: model.to_string(),
            system_prompt: SYSTEM_PROMPT.to_string(),
            max_steps: config.core.max_steps,
            session_id: uuid::Uuid::new_v4().to_string(),
        },
        memory_retriever: Some(memory_retriever),
        memory_extractor: Some(memory_extractor),
    }
}

fn make_event_callback() -> EventCallback {
    Box::new(|event| match event {
        AgentEvent::Delta { text, .. } => {
            print!("{}", text);
            let _ = std::io::stdout().flush();
        }
        AgentEvent::ToolStarted {
            tool, args_preview, ..
        } => {
            eprintln!(
                "\n  {} {} {}",
                "tool:".dimmed(),
                tool.yellow(),
                args_preview.dimmed()
            );
        }
        AgentEvent::ToolFinished {
            tool,
            ok,
            result_preview,
            ..
        } => {
            let icon = if ok { "ok" } else { "err" };
            eprintln!(
                "  {} {} {}",
                if ok {
                    icon.green().to_string()
                } else {
                    icon.red().to_string()
                },
                tool.dimmed(),
                result_preview.dimmed()
            );
        }
        AgentEvent::ApprovalRequest { .. } => {}
        AgentEvent::Done { .. } => {}
    })
}

fn make_approval_callback() -> ApprovalCallback {
    Box::new(|_tool, tier, summary, _args| {
        Box::pin(async move {
            eprintln!();
            eprintln!("  {} {} [{}]", "action:".yellow().bold(), summary, tier);
            eprint!("  Approve? [y/n/a(lways)]: ");
            let _ = std::io::stderr().flush();

            let input = tokio::task::spawn_blocking(|| {
                let mut s = String::new();
                std::io::stdin().read_line(&mut s).ok();
                s
            })
            .await
            .unwrap_or_default();

            match input.trim().to_lowercase().as_str() {
                "y" | "yes" => ApprovalDecision::Approve,
                "a" | "always" => ApprovalDecision::ApproveAlways,
                _ => ApprovalDecision::Deny,
            }
        })
    })
}

async fn cmd_ask(question: &str) {
    let api_key = get_key_or_exit();
    let config = MiConfig::load().unwrap_or_default();
    let model = get_model_or_exit(&config);

    let agent = create_agent_loop(&api_key, &config, &model);
    let mut messages = vec![Message::user(question)];

    match agent
        .run(&mut messages, &make_event_callback(), &make_approval_callback())
        .await
    {
        Ok(_) => println!(),
        Err(e) => {
            eprintln!("\n{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    }
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

    let agent = create_agent_loop(&api_key, &config, &model);
    let mut messages: Vec<Message> = Vec::new();
    let on_event = make_event_callback();
    let approve = make_approval_callback();

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

        messages.push(Message::user(input));

        print!("{} ", "mi:".cyan().bold());
        std::io::stdout().flush().unwrap();

        match agent.run(&mut messages, &on_event, &approve).await {
            Ok(text) => {
                println!();
                if !text.is_empty() {
                    messages.push(Message::assistant(&text));
                }
            }
            Err(e) => {
                eprintln!("\n{} {}", "error:".red().bold(), e);
            }
        }
    }
}

fn cmd_logs(tail: usize) {
    let path = mi_logs_dir().join("audit.jsonl");
    if !path.exists() {
        println!("No audit log found. Run {} first.", "mi ask".cyan());
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(tail);
    for line in &lines[start..] {
        println!("{}", line);
    }
}

fn cmd_memory(command: MemoryCommands) {
    let config = MiConfig::load().unwrap_or_default();
    let store = match MemoryStore::open(&mi_db_path(), config.memory.embed_dim) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} failed to open memory DB: {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };

    match command {
        MemoryCommands::List { limit } => {
            let memories = store.list_memories(limit, 0).unwrap_or_default();
            if memories.is_empty() {
                println!("No memories stored yet.");
                return;
            }
            for mem in &memories {
                let pin = if mem.pinned { " 📌" } else { "" };
                println!(
                    "  {} [{}] {}{}",
                    mem.id.chars().take(8).collect::<String>().dimmed(),
                    mem.kind.as_str().cyan(),
                    mem.text,
                    pin,
                );
            }
            println!(
                "\n  {} total memories",
                store.memory_count().unwrap_or(0).to_string().bold()
            );
        }
        MemoryCommands::Search { query, limit } => {
            let results = store.search_text(&query, limit).unwrap_or_default();
            if results.is_empty() {
                println!("No memories match \"{}\".", query);
                return;
            }
            for (id, _score) in &results {
                if let Ok(Some(mem)) = store.get_memory(id) {
                    println!(
                        "  {} [{}] {}",
                        mem.id.chars().take(8).collect::<String>().dimmed(),
                        mem.kind.as_str().cyan(),
                        mem.text,
                    );
                }
            }
        }
        MemoryCommands::Forget { id } => {
            // Support both short (8-char prefix) and full IDs
            let full_id = resolve_memory_id(&store, &id);
            match store.forget(&full_id) {
                Ok(true) => println!("{} Forgotten.", "ok:".green().bold()),
                Ok(false) => println!("{} No memory with that ID.", "error:".red().bold()),
                Err(e) => eprintln!("{} {}", "error:".red().bold(), e),
            }
        }
        MemoryCommands::Pin { id, unpin } => {
            let full_id = resolve_memory_id(&store, &id);
            let pinned = !unpin;
            match store.pin(&full_id, pinned) {
                Ok(true) => {
                    let verb = if pinned { "Pinned" } else { "Unpinned" };
                    println!("{} {}.", "ok:".green().bold(), verb);
                }
                Ok(false) => println!("{} No memory with that ID.", "error:".red().bold()),
                Err(e) => eprintln!("{} {}", "error:".red().bold(), e),
            }
        }
        MemoryCommands::Export { output } => {
            let memories = store.export_all().unwrap_or_default();
            let json = serde_json::to_string_pretty(&memories).unwrap();
            if let Some(path) = output {
                std::fs::write(&path, &json).unwrap();
                println!("{} Exported {} memories to {}", "ok:".green().bold(), memories.len(), path);
            } else {
                println!("{}", json);
            }
        }
        MemoryCommands::Wipe => {
            print!("{} This will delete ALL memories. Type 'yes' to confirm: ", "warning:".yellow().bold());
            std::io::stdout().flush().unwrap();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            if input.trim() == "yes" {
                store.wipe().unwrap();
                println!("{} All memories wiped.", "ok:".green().bold());
            } else {
                println!("Cancelled.");
            }
        }
    }
}

fn resolve_memory_id(store: &MemoryStore, short_id: &str) -> String {
    // If it looks like a full UUID, use as-is
    if short_id.len() > 16 {
        return short_id.to_string();
    }
    // Otherwise search the memory list for a prefix match
    if let Ok(memories) = store.list_memories(1000, 0) {
        for mem in &memories {
            if mem.id.starts_with(short_id) {
                return mem.id.clone();
            }
        }
    }
    short_id.to_string()
}

async fn cmd_doctor(fix: bool) {
    println!("{}", "mi doctor".cyan().bold());
    println!();

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

    let mut config = MiConfig::load().unwrap_or_default();
    let provider = GeminiProvider::new(api_key);

    match provider.list_models().await {
        Ok(models) => {
            let generate_models: Vec<_> = models
                .iter()
                .filter(|m| {
                    m.supported_methods
                        .contains(&"generateContent".to_string())
                })
                .collect();
            println!(
                "  {} {} models available ({} support generateContent)",
                "✓".green(),
                models.len(),
                generate_models.len()
            );

            println!();
            println!("  {}:", "Models supporting generateContent".bold());
            for m in &generate_models {
                let limits = match (m.input_token_limit, m.output_token_limit) {
                    (Some(i), Some(o)) => format!(" (in: {}k, out: {}k)", i / 1000, o / 1000),
                    _ => String::new(),
                };
                println!("    {}{}", m.id, limits.dimmed());
            }

            let current = config.active_model().to_string();
            if current.is_empty() {
                println!();
                println!("  {} No model configured in [models]", "✗".red());
                if fix {
                    let flash = generate_models.iter().find(|m| {
                        m.id.contains("flash")
                            && !m.id.contains("lite")
                            && !m.id.contains("preview")
                    });
                    if let Some(m) = flash {
                        config.models.default = m.id.clone();
                        if let Err(e) = config.save() {
                            eprintln!("  {} Could not save config: {}", "✗".red(), e);
                        } else {
                            println!("  {} Set default model to {}", "✓".green(), m.id.cyan());
                        }
                    } else {
                        println!(
                            "  {} Could not auto-pick a model. Set [models].default in ~/.mi/mi.toml",
                            "!".yellow()
                        );
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
                        let flash = generate_models.iter().find(|m| {
                            m.id.contains("flash")
                                && !m.id.contains("lite")
                                && !m.id.contains("preview")
                        });
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
            println!("  {} Could not list models: {}", "✗".red(), e);
        }
    }

    // Phase 1: show tool count
    let mut registry = ToolRegistry::new();
    builtin::register_all(&mut registry);
    let decls = registry.gemini_declarations();
    println!();
    println!("  {} {} tools registered", "✓".green(), decls.len());

    // Phase 2: show memory stats
    let config_for_mem = MiConfig::load().unwrap_or_default();
    match MemoryStore::open(&mi_db_path(), config_for_mem.memory.embed_dim) {
        Ok(store) => {
            let count = store.memory_count().unwrap_or(0);
            println!("  {} Memory DB open, {} memories stored", "✓".green(), count);
        }
        Err(e) => {
            println!("  {} Memory DB error: {}", "✗".red(), e);
        }
    }
}

async fn cmd_serve() {
    let config = MiConfig::load().unwrap_or_default();
    let token = ws::generate_token();

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

    println!("{} listening on ws://{}", "mi server".cyan().bold(), addr);
    println!("  token: {}", token.dimmed());

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
