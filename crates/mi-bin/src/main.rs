mod instance_lock;
mod overlay;
mod service;

use std::io::Write;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use colored::Colorize;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use mi_core::agent::loop_runner::{AgentConfig, AgentError, AgentEvent, AgentLoop, ApprovalCallback, EventCallback};
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
use mi_core::voice::pipeline::VoicePipeline;

use overlay::state::{BackendEvent, UiCommand};

const SYSTEM_PROMPT: &str = include_str!("../../../prompts/system.md");

#[derive(Parser)]
#[command(name = "mi", about = "mi — your system-level assistant", version = mi_core::VERSION)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the assistant (tray icon + overlay)
    Start,
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
    /// Register mi to start at login
    InstallService,
    /// Remove mi from login autostart
    UninstallService,
}

#[derive(Subcommand)]
enum MemoryCommands {
    /// List stored memories
    List {
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Search memories by keyword
    Search {
        query: String,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// Forget (soft-delete) a memory by ID
    Forget { id: String },
    /// Pin/unpin a memory
    Pin {
        id: String,
        #[arg(long)]
        unpin: bool,
    },
    /// Export all memories as JSON
    Export {
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
        key: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = ensure_mi_dir() {
        eprintln!("{} failed to create ~/.mi: {}", "error:".red().bold(), e);
        std::process::exit(1);
    }

    match cli.command {
        None | Some(Commands::Start) => cmd_start(),
        Some(Commands::InstallService) => cmd_install_service(),
        Some(Commands::UninstallService) => cmd_uninstall_service(),
        Some(Commands::Version) => cmd_version(),
        Some(Commands::Auth { command }) => match command {
            AuthCommands::SetGeminiKey { key } => cmd_set_gemini_key(key),
        },
        Some(Commands::Logs { tail }) => cmd_logs(tail),
        Some(Commands::Memory { command }) => cmd_memory(command),
        Some(async_cmd) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(async {
                match async_cmd {
                    Commands::Ask { question } => cmd_ask(&question).await,
                    Commands::Chat => cmd_chat().await,
                    Commands::Doctor { fix } => cmd_doctor(fix).await,
                    Commands::Serve => cmd_serve().await,
                    _ => unreachable!(),
                }
            });
        }
    }
}

// ─── Phase 3: Start (tray + overlay + backend) ───

fn cmd_start() {
    let _lock = match instance_lock::InstanceLock::acquire() {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };

    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<UiCommand>();
    let (event_tx, event_rx) = std::sync::mpsc::channel::<BackendEvent>();
    let (ctx_tx, ctx_rx) = std::sync::mpsc::sync_channel::<eframe::egui::Context>(1);

    // Spawn backend thread with its own tokio runtime
    let event_tx_backend = event_tx.clone();
    std::thread::spawn(move || {
        let egui_ctx = ctx_rx.recv().expect("failed to receive egui context");
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(backend_loop(cmd_rx, event_tx_backend, egui_ctx));
    });

    // Create tray icon on main thread (must be before eframe for winit compatibility)
    let _tray = overlay::tray::create_tray();

    // Register global hotkeys on main thread
    let (_hotkey_manager, hotkey_ids) = match overlay::hotkeys::register_hotkeys() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{} failed to register hotkeys: {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    };

    // Run overlay (blocks main thread)
    overlay::app::run_overlay(cmd_tx, event_rx, hotkey_ids, ctx_tx);
}

async fn backend_loop(
    cmd_rx: std::sync::mpsc::Receiver<UiCommand>,
    event_tx: std::sync::mpsc::Sender<BackendEvent>,
    egui_ctx: eframe::egui::Context,
) {
    let api_key = match secrets::get_gemini_key() {
        Ok(Some(key)) if !key.is_empty() => key,
        _ => {
            let msg = "No Gemini API key. Run: mi auth set-gemini-key";
            overlay::notify::show_error(msg);
            let _ = event_tx.send(BackendEvent::Error {
                message: msg.into(),
            });
            egui_ctx.request_repaint();
            return;
        }
    };

    let config = MiConfig::load().unwrap_or_default();
    let model = config.active_model().to_string();
    if model.is_empty() {
        let _ = event_tx.send(BackendEvent::Error {
            message: "No model configured. Run: mi doctor --fix".into(),
        });
        egui_ctx.request_repaint();
        return;
    }

    let approval_slot: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<ApprovalDecision>>>> =
        Arc::new(std::sync::Mutex::new(None));
    let cancel_slot: Arc<std::sync::Mutex<Option<CancellationToken>>> =
        Arc::new(std::sync::Mutex::new(None));

    // Initialize voice pipeline
    let mut voice = VoicePipeline::new(&config.voice);
    match voice.init().await {
        Ok(()) => tracing::info!("voice pipeline initialized"),
        Err(e) => tracing::warn!("voice pipeline init failed (voice disabled): {}", e),
    }
    let voice = Arc::new(tokio::sync::Mutex::new(voice));

    #[derive(Debug)]
    enum BackendMsg {
        Text(String),
        PttStart,
        PttStop,
    }

    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::channel::<BackendMsg>(8);

    // Command processor: blocking thread dispatches commands
    let approval_cmd = approval_slot.clone();
    let cancel_cmd = cancel_slot.clone();
    let event_tx_cmd = event_tx.clone();
    let egui_ctx_cmd = egui_ctx.clone();
    tokio::task::spawn_blocking(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                UiCommand::SendMessage(text) => {
                    let _ = msg_tx.blocking_send(BackendMsg::Text(text));
                }
                UiCommand::ApprovalResponse { decision, .. } => {
                    if let Some(tx) = approval_cmd.lock().unwrap().take() {
                        let _ = tx.send(decision);
                    }
                }
                UiCommand::CancelTurn => {
                    if let Some(token) = cancel_cmd.lock().unwrap().as_ref() {
                        token.cancel();
                    }
                    let _ = event_tx_cmd.send(BackendEvent::Cancelled);
                    egui_ctx_cmd.request_repaint();
                }
                UiCommand::PttStart => {
                    let _ = msg_tx.blocking_send(BackendMsg::PttStart);
                }
                UiCommand::PttStop => {
                    let _ = msg_tx.blocking_send(BackendMsg::PttStop);
                }
                UiCommand::Quit => break,
            }
        }
    });

    // Message handler: runs agent loop per message (text or voice)
    while let Some(msg) = msg_rx.recv().await {
        match msg {
            BackendMsg::PttStart => {
                let voice_guard = voice.lock().await;
                if !voice_guard.is_stt_ready() {
                    drop(voice_guard);
                    let _ = event_tx.send(BackendEvent::VoiceListening);
                    egui_ctx.request_repaint();

                    let mut voice_mut = voice.lock().await;
                    if let Err(e) = voice_mut.ensure_stt().await {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: format!("STT model: {}", e),
                        });
                        egui_ctx.request_repaint();
                        continue;
                    }
                    if let Err(e) = voice_mut.start_listening().await {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: format!("mic: {}", e),
                        });
                        egui_ctx.request_repaint();
                    }
                } else {
                    let _ = event_tx.send(BackendEvent::VoiceListening);
                    egui_ctx.request_repaint();
                    if let Err(e) = voice_guard.start_listening().await {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: format!("mic: {}", e),
                        });
                        egui_ctx.request_repaint();
                    }
                }
            }
            BackendMsg::PttStop => {
                let _ = event_tx.send(BackendEvent::VoiceTranscribing);
                egui_ctx.request_repaint();

                let voice_guard = voice.lock().await;
                let text = match voice_guard.stop_listening_and_transcribe().await {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: format!("transcription: {}", e),
                        });
                        egui_ctx.request_repaint();
                        continue;
                    }
                };
                drop(voice_guard);

                if text.is_empty() {
                    let _ = event_tx.send(BackendEvent::VoiceDone);
                    egui_ctx.request_repaint();
                    continue;
                }

                let _ = event_tx.send(BackendEvent::VoiceTranscript { text: text.clone() });
                egui_ctx.request_repaint();

                // Run the agent loop with the transcribed text
                let cancel_token = CancellationToken::new();
                *cancel_slot.lock().unwrap() = Some(cancel_token.clone());

                let mut agent = create_agent_loop(&api_key, &config, &model);
                agent.cancel_token = Some(cancel_token);

                let on_event = make_gui_event_callback(event_tx.clone(), egui_ctx.clone());
                let approve = make_gui_approval_callback(
                    event_tx.clone(),
                    egui_ctx.clone(),
                    approval_slot.clone(),
                );

                let mut messages = vec![Message::user(&text)];

                match agent.run(&mut messages, &on_event, &approve).await {
                    Ok(response_text) => {
                        // Speak the response via TTS
                        let voice_guard = voice.lock().await;
                        if voice_guard.is_tts_ready() && !response_text.is_empty() {
                            let _ = event_tx.send(BackendEvent::VoiceSpeaking);
                            egui_ctx.request_repaint();

                            match voice_guard.speak(&response_text).await {
                                Ok(wav_data) if !wav_data.is_empty() => {
                                    let _ = tokio::task::spawn_blocking(move || {
                                        play_wav_bytes(&wav_data);
                                    }).await;
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    tracing::warn!("TTS playback failed: {}", e);
                                }
                            }
                        }

                        let _ = event_tx.send(BackendEvent::Done { text: response_text });
                        egui_ctx.request_repaint();
                    }
                    Err(AgentError::Cancelled) => {}
                    Err(e) => {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: e.to_string(),
                        });
                        egui_ctx.request_repaint();
                    }
                }

                *cancel_slot.lock().unwrap() = None;
            }
            BackendMsg::Text(text) => {
                let cancel_token = CancellationToken::new();
                *cancel_slot.lock().unwrap() = Some(cancel_token.clone());

                let mut agent = create_agent_loop(&api_key, &config, &model);
                agent.cancel_token = Some(cancel_token);

                let on_event = make_gui_event_callback(event_tx.clone(), egui_ctx.clone());
                let approve = make_gui_approval_callback(
                    event_tx.clone(),
                    egui_ctx.clone(),
                    approval_slot.clone(),
                );

                let mut messages = vec![Message::user(&text)];

                match agent.run(&mut messages, &on_event, &approve).await {
                    Ok(text) => {
                        let _ = event_tx.send(BackendEvent::Done { text });
                        egui_ctx.request_repaint();
                    }
                    Err(AgentError::Cancelled) => {}
                    Err(e) => {
                        let _ = event_tx.send(BackendEvent::Error {
                            message: e.to_string(),
                        });
                        egui_ctx.request_repaint();
                    }
                }

                *cancel_slot.lock().unwrap() = None;
            }
        }
    }
}

fn play_wav_bytes(wav_data: &[u8]) {
    use rodio::DeviceSinkBuilder;
    use std::io::Cursor;

    let handle = match DeviceSinkBuilder::open_default_sink() {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("no audio output device: {}", e);
            return;
        }
    };
    let mixer = handle.mixer();
    let cursor = Cursor::new(wav_data.to_vec());
    match rodio::stream::play(mixer, cursor) {
        Ok(player) => {
            player.sleep_until_end();
        }
        Err(e) => tracing::warn!("failed to decode TTS audio: {}", e),
    }
}

fn make_gui_event_callback(
    event_tx: std::sync::mpsc::Sender<BackendEvent>,
    ctx: eframe::egui::Context,
) -> EventCallback {
    Box::new(move |event| {
        let be = match event {
            AgentEvent::Delta { text, .. } => BackendEvent::Delta { text },
            AgentEvent::ToolStarted {
                tool, args_preview, ..
            } => BackendEvent::ToolStarted { tool, args_preview },
            AgentEvent::ToolFinished {
                tool,
                ok,
                result_preview,
                ..
            } => BackendEvent::ToolFinished {
                tool,
                ok,
                result_preview,
            },
            AgentEvent::ApprovalRequest { .. } => return,
            AgentEvent::Done { .. } => return,
        };
        let _ = event_tx.send(be);
        ctx.request_repaint();
    })
}

fn make_gui_approval_callback(
    event_tx: std::sync::mpsc::Sender<BackendEvent>,
    ctx: eframe::egui::Context,
    slot: Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<ApprovalDecision>>>>,
) -> ApprovalCallback {
    Box::new(move |tool, tier, summary, args| {
        let event_tx = event_tx.clone();
        let ctx = ctx.clone();
        let slot = slot.clone();
        Box::pin(async move {
            let approval_id = uuid::Uuid::new_v4().to_string();
            let (tx, rx) = tokio::sync::oneshot::channel();
            *slot.lock().unwrap() = Some(tx);

            overlay::notify::show_approval_needed(&tool, &summary);
            let _ = event_tx.send(BackendEvent::ApprovalRequest {
                approval_id,
                tool,
                tier,
                summary,
                args,
            });
            ctx.request_repaint();

            rx.await.unwrap_or(ApprovalDecision::Deny)
        })
    })
}

fn cmd_install_service() {
    match service::install_autostart() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}

fn cmd_uninstall_service() {
    match service::uninstall_autostart() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{} {}", "error:".red().bold(), e);
            std::process::exit(1);
        }
    }
}

// ─── Existing commands ───

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
        cancel_token: None,
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
                let pin = if mem.pinned { " (pinned)" } else { "" };
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
                println!(
                    "{} Exported {} memories to {}",
                    "ok:".green().bold(),
                    memories.len(),
                    path
                );
            } else {
                println!("{}", json);
            }
        }
        MemoryCommands::Wipe => {
            print!(
                "{} This will delete ALL memories. Type 'yes' to confirm: ",
                "warning:".yellow().bold()
            );
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
    if short_id.len() > 16 {
        return short_id.to_string();
    }
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
            println!("  {} Gemini API key found in keyring", "ok".green());
            key
        }
        _ => {
            println!(
                "  {} No Gemini API key. Run: {}",
                "err".red(),
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
                "ok".green(),
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
                println!("  {} No model configured in [models]", "err".red());
                if fix {
                    let flash = generate_models.iter().find(|m| {
                        m.id.contains("flash")
                            && !m.id.contains("lite")
                            && !m.id.contains("preview")
                    });
                    if let Some(m) = flash {
                        config.models.default = m.id.clone();
                        if let Err(e) = config.save() {
                            eprintln!("  {} Could not save config: {}", "err".red(), e);
                        } else {
                            println!("  {} Set default model to {}", "ok".green(), m.id.cyan());
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
                        "ok".green(),
                        current.cyan()
                    );
                } else {
                    println!();
                    println!(
                        "  {} Configured model {} not found",
                        "err".red(),
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
                                eprintln!("  {} Could not save config: {}", "err".red(), e);
                            } else {
                                println!(
                                    "  {} Updated default model to {}",
                                    "ok".green(),
                                    m.id.cyan()
                                );
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("  {} Could not list models: {}", "err".red(), e);
        }
    }

    let mut registry = ToolRegistry::new();
    builtin::register_all(&mut registry);
    let decls = registry.gemini_declarations();
    println!();
    println!("  {} {} tools registered", "ok".green(), decls.len());

    let config_for_mem = MiConfig::load().unwrap_or_default();
    match MemoryStore::open(&mi_db_path(), config_for_mem.memory.embed_dim) {
        Ok(store) => {
            let count = store.memory_count().unwrap_or(0);
            println!("  {} Memory DB open, {} memories stored", "ok".green(), count);
        }
        Err(e) => {
            println!("  {} Memory DB error: {}", "err".red(), e);
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
