use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;
use global_hotkey::GlobalHotKeyEvent;
use mi_core::events::ApprovalDecision;
use tray_icon::menu::MenuEvent;

use super::hotkeys::HotkeyIds;
use super::state::{BackendEvent, UiCommand};
use super::tray;

const BAR_WIDTH: f32 = 620.0;
const BAR_HEIGHT: f32 = 320.0;
const AUTO_HIDE_SECS: f64 = 5.0;

// Colors
const BG_DARK: egui::Color32 = egui::Color32::from_rgb(18, 18, 24);
const BG_CARD: egui::Color32 = egui::Color32::from_rgb(28, 28, 38);
const BG_INPUT: egui::Color32 = egui::Color32::from_rgb(36, 36, 48);
const BORDER: egui::Color32 = egui::Color32::from_rgb(50, 50, 65);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(99, 102, 241);
const ACCENT_LIGHT: egui::Color32 = egui::Color32::from_rgb(129, 140, 248);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(237, 237, 245);
const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(140, 140, 165);
const GREEN: egui::Color32 = egui::Color32::from_rgb(52, 211, 153);
const AMBER: egui::Color32 = egui::Color32::from_rgb(251, 191, 36);
const RED: egui::Color32 = egui::Color32::from_rgb(248, 113, 113);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Hidden,
    Input,
    Listening,
    Transcribing,
    Thinking,
    Responding,
    Approval,
    Speaking,
    Done,
}

struct PendingApproval {
    approval_id: String,
    tool: String,
    tier: String,
    summary: String,
    args: serde_json::Value,
}

#[derive(Debug, Clone)]
struct ToolChip {
    tool: String,
    ok: Option<bool>,
}

pub struct MiOverlay {
    cmd_tx: mpsc::Sender<UiCommand>,
    event_rx: mpsc::Receiver<BackendEvent>,
    hotkey_ids: HotkeyIds,

    phase: Phase,
    input_text: String,
    response_text: String,
    tool_chips: Vec<ToolChip>,
    pending_approval: Option<PendingApproval>,
    last_error: Option<String>,
    done_at: Option<Instant>,
    wants_focus: bool,
    first_frame: bool,
    frame_count: u64,
}

impl MiOverlay {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cmd_tx: mpsc::Sender<UiCommand>,
        event_rx: mpsc::Receiver<BackendEvent>,
        hotkey_ids: HotkeyIds,
    ) -> Self {
        Self::setup_visuals(&cc.egui_ctx);
        Self {
            cmd_tx,
            event_rx,
            hotkey_ids,
            phase: Phase::Hidden,
            input_text: String::new(),
            response_text: String::new(),
            tool_chips: Vec::new(),
            pending_approval: None,
            last_error: None,
            done_at: None,
            wants_focus: false,
            first_frame: true,
            frame_count: 0,
        }
    }

    fn setup_visuals(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT_PRIMARY);
        visuals.widgets.noninteractive.bg_fill = BG_CARD;
        visuals.widgets.inactive.bg_fill = BG_INPUT;
        visuals.widgets.inactive.weak_bg_fill = BG_INPUT;
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(45, 45, 60);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT_LIGHT);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, BORDER);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, BORDER);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
        ctx.set_visuals(visuals);
    }

    fn show(&mut self, ctx: &egui::Context) {
        if self.phase != Phase::Hidden && self.phase != Phase::Done && self.phase != Phase::Input {
            return;
        }
        self.phase = Phase::Input;
        self.input_text.clear();
        self.response_text.clear();
        self.tool_chips.clear();
        self.pending_approval = None;
        self.last_error = None;
        self.done_at = None;
        self.wants_focus = true;

        let monitor = ctx.input(|i| {
            i.viewport()
                .monitor_size
                .unwrap_or(egui::vec2(1920.0, 1080.0))
        });
        let x = (monitor.x - BAR_WIDTH) / 2.0;
        let y = (monitor.y - BAR_HEIGHT) / 2.0 - 60.0;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide(&mut self, ctx: &egui::Context) {
        self.phase = Phase::Hidden;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn poll_tray_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id.0.as_str();
            if id == tray::MENU_TALK_ID {
                self.show(ctx);
            } else if id == tray::MENU_QUIT_ID {
                let _ = self.cmd_tx.send(UiCommand::Quit);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn poll_hotkey_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.id == self.hotkey_ids.ptt {
                match event.state {
                    global_hotkey::HotKeyState::Pressed => {
                        if self.phase == Phase::Hidden || self.phase == Phase::Done || self.phase == Phase::Input {
                            self.show(ctx);
                            self.phase = Phase::Listening;
                            self.response_text.clear();
                            self.tool_chips.clear();
                            self.last_error = None;
                            let _ = self.cmd_tx.send(UiCommand::PttStart);
                        }
                    }
                    global_hotkey::HotKeyState::Released => {
                        if self.phase == Phase::Listening {
                            self.phase = Phase::Transcribing;
                            let _ = self.cmd_tx.send(UiCommand::PttStop);
                        }
                    }
                }
            } else if event.state == global_hotkey::HotKeyState::Pressed {
                if event.id == self.hotkey_ids.type_mode {
                    if self.phase == Phase::Hidden || self.phase == Phase::Done {
                        self.show(ctx);
                    } else {
                        self.hide(ctx);
                    }
                } else if event.id == self.hotkey_ids.kill_switch {
                    let _ = self.cmd_tx.send(UiCommand::CancelTurn);
                    if self.phase == Phase::Thinking || self.phase == Phase::Responding || self.phase == Phase::Speaking {
                        self.last_error = Some("cancelled".into());
                        self.phase = Phase::Done;
                        self.done_at = Some(Instant::now());
                    }
                }
            }
        }
    }

    fn poll_backend_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                BackendEvent::Delta { text } => {
                    self.phase = Phase::Responding;
                    self.response_text.push_str(&text);
                }
                BackendEvent::ToolStarted { tool, .. } => {
                    self.phase = Phase::Thinking;
                    self.tool_chips.push(ToolChip { tool, ok: None });
                }
                BackendEvent::ToolFinished { tool, ok, .. } => {
                    if let Some(chip) = self
                        .tool_chips
                        .iter_mut()
                        .rev()
                        .find(|c| c.tool == tool && c.ok.is_none())
                    {
                        chip.ok = Some(ok);
                    }
                }
                BackendEvent::ApprovalRequest {
                    approval_id,
                    tool,
                    tier,
                    summary,
                    args,
                } => {
                    self.phase = Phase::Approval;
                    self.pending_approval = Some(PendingApproval {
                        approval_id,
                        tool,
                        tier,
                        summary,
                        args,
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                BackendEvent::Done { text } => {
                    self.phase = Phase::Done;
                    if !text.is_empty() && self.response_text.is_empty() {
                        self.response_text = text;
                    }
                    self.done_at = Some(Instant::now());
                }
                BackendEvent::Error { message } => {
                    self.last_error = Some(message);
                    self.phase = Phase::Done;
                    self.done_at = Some(Instant::now());
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
                BackendEvent::Cancelled => {
                    self.last_error = Some("cancelled".into());
                    self.phase = Phase::Done;
                    self.done_at = Some(Instant::now());
                }
                BackendEvent::VoiceListening => {
                    self.phase = Phase::Listening;
                }
                BackendEvent::VoiceTranscribing => {
                    self.phase = Phase::Transcribing;
                }
                BackendEvent::VoiceTranscript { text } => {
                    self.input_text = text;
                }
                BackendEvent::VoiceSpeaking => {
                    self.phase = Phase::Speaking;
                }
                BackendEvent::VoiceDone => {
                    self.phase = Phase::Done;
                    self.done_at = Some(Instant::now());
                }
            }
            ctx.request_repaint();
        }
    }

    fn render_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.frame_count = self.frame_count.wrapping_add(1);

        egui::Frame::new()
            .fill(BG_DARK)
            .corner_radius(16.0)
            .inner_margin(egui::Margin::same(20))
            .stroke(egui::Stroke::new(1.0, BORDER))
            .shadow(egui::epaint::Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 4,
                color: egui::Color32::from_black_alpha(80),
            })
            .show(ui, |ui| {
                ui.set_width(BAR_WIDTH - 40.0);

                // Header
                self.render_header(ui);
                ui.add_space(12.0);

                // Input field
                if self.phase == Phase::Input {
                    self.render_input(ui, &ctx);
                    ui.add_space(8.0);
                }

                // Listening animation
                if self.phase == Phase::Listening {
                    self.render_listening(ui);
                }

                // Transcribing indicator
                if self.phase == Phase::Transcribing {
                    self.render_transcribing(ui);
                }

                // Tool chips
                self.render_tool_chips(ui);

                // Thinking animation
                if self.phase == Phase::Thinking && self.tool_chips.is_empty() {
                    self.render_thinking(ui);
                }

                // Speaking indicator
                if self.phase == Phase::Speaking {
                    self.render_speaking(ui);
                }

                // Response
                if !self.response_text.is_empty() {
                    self.render_response(ui);
                }

                // Error
                if let Some(err) = &self.last_error {
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(40, 20, 20))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(10))
                        .stroke(egui::Stroke::new(0.5, RED))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(format!("\u{26A0} {}", err))
                                    .color(RED)
                                    .size(12.5),
                            );
                        });
                }

                // Approval card
                self.render_approval_card(ui);

                // Footer hint
                ui.add_space(8.0);
                self.render_footer(ui);
            });
    }

    fn render_header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Logo dot
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(10.0, 10.0),
                egui::Sense::hover(),
            );
            let color = match self.phase {
                Phase::Input => ACCENT,
                Phase::Listening => RED,
                Phase::Transcribing => AMBER,
                Phase::Thinking => AMBER,
                Phase::Responding => ACCENT_LIGHT,
                Phase::Approval => AMBER,
                Phase::Speaking => GREEN,
                Phase::Done => GREEN,
                Phase::Hidden => ACCENT,
            };
            ui.painter().circle_filled(rect.center(), 5.0, color);

            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("mi")
                    .color(TEXT_PRIMARY)
                    .size(14.0)
                    .strong(),
            );

            ui.add_space(6.0);

            let status = match self.phase {
                Phase::Input => "ready",
                Phase::Listening => "listening...",
                Phase::Transcribing => "transcribing...",
                Phase::Thinking => "thinking...",
                Phase::Responding => "responding",
                Phase::Approval => "needs approval",
                Phase::Speaking => "speaking...",
                Phase::Done => "done",
                Phase::Hidden => "",
            };
            ui.label(egui::RichText::new(status).color(TEXT_DIM).size(12.0));
        });
    }

    fn render_input(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Frame::new()
            .fill(BG_INPUT)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::symmetric(14, 10))
            .stroke(egui::Stroke::new(1.0, if self.wants_focus { ACCENT } else { BORDER }))
            .show(ui, |ui| {
                ui.set_width(BAR_WIDTH - 68.0);

                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.input_text)
                        .hint_text(
                            egui::RichText::new("Ask mi anything...")
                                .color(TEXT_DIM)
                                .size(14.0),
                        )
                        .desired_width(BAR_WIDTH - 100.0)
                        .text_color(TEXT_PRIMARY)
                        .frame(egui::Frame::NONE)
                        .font(egui::FontId::proportional(14.0)),
                );

                if self.wants_focus {
                    response.request_focus();
                    self.wants_focus = false;
                }

                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.input_text.trim().is_empty()
                {
                    let msg = self.input_text.trim().to_string();
                    self.input_text.clear();
                    self.phase = Phase::Thinking;
                    let _ = self.cmd_tx.send(UiCommand::SendMessage(msg));
                }

                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.hide(ctx);
                }
            });
    }

    fn render_thinking(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let t = ui.input(|i| i.time);
            for i in 0..3 {
                let phase = t * 3.0 + i as f64 * 0.8;
                let alpha = ((phase.sin() * 0.5 + 0.5) * 180.0 + 75.0) as u8;
                let color = egui::Color32::from_rgba_unmultiplied(
                    ACCENT.r(), ACCENT.g(), ACCENT.b(), alpha,
                );
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(6.0, 6.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(rect.center(), 3.0, color);
                ui.add_space(2.0);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("thinking...").color(TEXT_DIM).size(12.0));
        });
        ui.ctx().request_repaint();
    }

    fn render_tool_chips(&self, ui: &mut egui::Ui) {
        if self.tool_chips.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
            for chip in &self.tool_chips {
                let (bg, fg, icon) = match chip.ok {
                    None => (
                        egui::Color32::from_rgb(40, 36, 20),
                        AMBER,
                        "\u{23F3}",
                    ),
                    Some(true) => (
                        egui::Color32::from_rgb(20, 40, 30),
                        GREEN,
                        "\u{2713}",
                    ),
                    Some(false) => (
                        egui::Color32::from_rgb(40, 20, 20),
                        RED,
                        "\u{2717}",
                    ),
                };
                egui::Frame::new()
                    .fill(bg)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 3))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("{} {}", icon, chip.tool))
                                .color(fg)
                                .size(11.0),
                        );
                    });
            }
        });
        ui.add_space(4.0);
    }

    fn render_response(&self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(BG_CARD)
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.set_width(BAR_WIDTH - 88.0);
                        ui.label(
                            egui::RichText::new(&self.response_text)
                                .color(TEXT_PRIMARY)
                                .size(13.5)
                                .line_height(Some(20.0)),
                        );
                    });
            });
    }

    fn render_approval_card(&mut self, ui: &mut egui::Ui) {
        let (approval_id, tool_name, tier_str, summary_text, args_val) =
            match &self.pending_approval {
                Some(a) => (
                    a.approval_id.clone(),
                    a.tool.clone(),
                    a.tier.clone(),
                    a.summary.clone(),
                    a.args.clone(),
                ),
                None => return,
            };

        ui.add_space(8.0);

        egui::Frame::new()
            .fill(egui::Color32::from_rgb(35, 30, 18))
            .corner_radius(10.0)
            .inner_margin(egui::Margin::same(14))
            .stroke(egui::Stroke::new(1.0, AMBER))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("\u{1F6E1}")
                            .size(14.0),
                    );
                    ui.label(
                        egui::RichText::new(tool_name.to_string())
                            .color(AMBER)
                            .size(13.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!("[{}]", tier_str))
                            .color(TEXT_DIM)
                            .size(11.0),
                    );
                });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(&summary_text)
                        .color(TEXT_PRIMARY)
                        .size(12.5),
                );
                ui.add_space(4.0);

                ui.collapsing(
                    egui::RichText::new("show details").color(TEXT_DIM).size(11.0),
                    |ui| {
                        let args_str = serde_json::to_string_pretty(&args_val)
                            .unwrap_or_else(|_| args_val.to_string());
                        ui.label(
                            egui::RichText::new(args_str)
                                .color(TEXT_DIM)
                                .size(11.0)
                                .monospace(),
                        );
                    },
                );

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                    let approve_btn = egui::Button::new(
                        egui::RichText::new("\u{2713} Approve")
                            .color(egui::Color32::WHITE)
                            .size(12.0),
                    )
                    .fill(GREEN.gamma_multiply(0.8))
                    .corner_radius(8.0)
                    .min_size(egui::vec2(80.0, 28.0));

                    if ui.add(approve_btn).clicked() {
                        let _ = self.cmd_tx.send(UiCommand::ApprovalResponse {
                            approval_id: approval_id.clone(),
                            decision: ApprovalDecision::Approve,
                        });
                        self.pending_approval = None;
                        self.phase = Phase::Thinking;
                    }

                    let deny_btn = egui::Button::new(
                        egui::RichText::new("\u{2717} Deny")
                            .color(egui::Color32::WHITE)
                            .size(12.0),
                    )
                    .fill(RED.gamma_multiply(0.7))
                    .corner_radius(8.0)
                    .min_size(egui::vec2(80.0, 28.0));

                    if ui.add(deny_btn).clicked() {
                        let _ = self.cmd_tx.send(UiCommand::ApprovalResponse {
                            approval_id: approval_id.clone(),
                            decision: ApprovalDecision::Deny,
                        });
                        self.pending_approval = None;
                        self.phase = Phase::Thinking;
                    }

                    let always_btn = egui::Button::new(
                        egui::RichText::new("Always")
                            .color(TEXT_PRIMARY)
                            .size(12.0),
                    )
                    .fill(BG_INPUT)
                    .corner_radius(8.0)
                    .min_size(egui::vec2(60.0, 28.0));

                    if ui.add(always_btn).clicked() {
                        let _ = self.cmd_tx.send(UiCommand::ApprovalResponse {
                            approval_id: approval_id.clone(),
                            decision: ApprovalDecision::ApproveAlways,
                        });
                        self.pending_approval = None;
                        self.phase = Phase::Thinking;
                    }
                });
            });
    }

    fn render_listening(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let t = ui.input(|i| i.time);
            for i in 0..5 {
                let phase = t * 4.0 + i as f64 * 0.6;
                let height = (phase.sin() * 0.5 + 0.5) * 16.0 + 4.0;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(4.0, 20.0),
                    egui::Sense::hover(),
                );
                let bar_rect = egui::Rect::from_center_size(
                    rect.center(),
                    egui::vec2(3.0, height as f32),
                );
                ui.painter().rect_filled(bar_rect, 1.5, RED);
                ui.add_space(1.0);
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("listening...")
                    .color(RED)
                    .size(13.0)
                    .strong(),
            );
        });
        ui.ctx().request_repaint();
    }

    fn render_transcribing(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let t = ui.input(|i| i.time);
            for i in 0..3 {
                let phase = t * 3.0 + i as f64 * 0.8;
                let alpha = ((phase.sin() * 0.5 + 0.5) * 180.0 + 75.0) as u8;
                let color = egui::Color32::from_rgba_unmultiplied(
                    AMBER.r(), AMBER.g(), AMBER.b(), alpha,
                );
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(6.0, 6.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(rect.center(), 3.0, color);
                ui.add_space(2.0);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("transcribing...").color(AMBER).size(12.0));
        });
        ui.ctx().request_repaint();
    }

    fn render_speaking(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let t = ui.input(|i| i.time);
            for i in 0..4 {
                let phase = t * 2.5 + i as f64 * 0.7;
                let scale = (phase.sin() * 0.3 + 0.7) as f32;
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(6.0, 6.0),
                    egui::Sense::hover(),
                );
                ui.painter().circle_filled(rect.center(), 3.0 * scale, GREEN);
                ui.add_space(2.0);
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("speaking...").color(GREEN).size(12.0));
        });
        ui.ctx().request_repaint();
    }

    fn render_footer(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let hints = match self.phase {
                Phase::Input => "Enter to send \u{00B7} Ctrl+Space to talk \u{00B7} Esc to close",
                Phase::Listening => "release Ctrl+Space to send",
                Phase::Transcribing => "transcribing speech...",
                Phase::Thinking | Phase::Responding => "Ctrl+Alt+Esc to cancel",
                Phase::Approval => "choose an action above",
                Phase::Speaking => "Ctrl+Alt+Esc to stop",
                Phase::Done => "Ctrl+Shift+M to reopen \u{00B7} Ctrl+Space to talk",
                Phase::Hidden => "",
            };
            ui.label(egui::RichText::new(hints).color(TEXT_DIM).size(10.5));
        });
    }
}

impl eframe::App for MiOverlay {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [18.0 / 255.0, 18.0 / 255.0, 24.0 / 255.0, 1.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_frame {
            self.first_frame = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        self.poll_tray_events(ctx);
        self.poll_hotkey_events(ctx);
        self.poll_backend_events(ctx);

        if self.phase == Phase::Done
            && let Some(done_at) = self.done_at
            && done_at.elapsed().as_secs_f64() > AUTO_HIDE_SECS
        {
            self.hide(ctx);
        }

        if self.phase == Phase::Hidden {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.phase == Phase::Hidden {
            return;
        }
        self.render_bar(ui);
    }
}

pub fn run_overlay(
    cmd_tx: mpsc::Sender<UiCommand>,
    event_rx: mpsc::Receiver<BackendEvent>,
    hotkey_ids: HotkeyIds,
    egui_ctx_tx: mpsc::SyncSender<egui::Context>,
) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([BAR_WIDTH, BAR_HEIGHT])
            .with_decorations(false)
            .with_always_on_top()
            .with_visible(false)
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "mi",
        options,
        Box::new(move |cc| {
            let _ = egui_ctx_tx.send(cc.egui_ctx.clone());
            Ok(Box::new(MiOverlay::new(cc, cmd_tx, event_rx, hotkey_ids)))
        }),
    )
    .expect("failed to run overlay");
}
