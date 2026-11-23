use mi_core::events::ApprovalDecision;

#[derive(Debug, Clone)]
pub enum UiCommand {
    SendMessage(String),
    ApprovalResponse {
        #[allow(dead_code)]
        approval_id: String,
        decision: ApprovalDecision,
    },
    CancelTurn,
    PttStart,
    PttStop,
    Quit,
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Delta { text: String },
    ToolStarted { tool: String, #[allow(dead_code)] args_preview: String },
    ToolFinished { tool: String, ok: bool, #[allow(dead_code)] result_preview: String },
    ApprovalRequest {
        approval_id: String,
        tool: String,
        tier: String,
        summary: String,
        args: serde_json::Value,
    },
    Done { text: String },
    Error { message: String },
    Cancelled,
    VoiceListening,
    VoiceTranscribing,
    VoiceTranscript { text: String },
    VoiceSpeaking,
    VoiceDone,
}
