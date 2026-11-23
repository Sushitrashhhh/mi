use std::sync::Arc;

use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::events::ApprovalDecision;
use crate::llm::types::{FunctionResponse, Message, Role};
use crate::llm::AnyProvider;
use crate::memory::extractor::MemoryExtractor;
use crate::memory::retrieve::MemoryRetriever;
use crate::permissions::audit::AuditLog;
use crate::permissions::policy::{PolicyDecision, PolicyEngine, TurnTaint};
use crate::tools::registry::{render_summary, Tier, ToolResult};
use crate::tools::registry::ToolRegistry;
use crate::tools::ToolExecutor;

#[derive(Debug)]
pub enum AgentEvent {
    Delta {
        turn_id: String,
        text: String,
    },
    ToolStarted {
        turn_id: String,
        call_id: String,
        tool: String,
        args_preview: String,
    },
    ToolFinished {
        turn_id: String,
        call_id: String,
        tool: String,
        result_preview: String,
        ok: bool,
    },
    ApprovalRequest {
        tool: String,
        tier: String,
        summary: String,
    },
    Done {
        turn_id: String,
        text: String,
    },
}

#[derive(Debug)]
pub enum AgentError {
    Llm(String),
    MaxSteps,
    Cancelled,
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::Llm(msg) => write!(f, "LLM error: {}", msg),
            AgentError::MaxSteps => write!(f, "reached maximum steps"),
            AgentError::Cancelled => write!(f, "cancelled by user"),
        }
    }
}

pub struct AgentConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_steps: u32,
    pub session_id: String,
}

pub type EventCallback =
    Box<dyn Fn(AgentEvent) + Send + Sync>;

pub type ApprovalCallback = Box<
    dyn Fn(
            String,
            String,
            String,
            serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ApprovalDecision> + Send>>
        + Send
        + Sync,
>;

pub struct AgentLoop {
    pub provider: Arc<AnyProvider>,
    pub registry: Arc<ToolRegistry>,
    pub executor: Arc<ToolExecutor>,
    pub policy: Arc<PolicyEngine>,
    pub audit: Arc<AuditLog>,
    pub config: AgentConfig,
    pub memory_retriever: Option<Arc<MemoryRetriever>>,
    pub memory_extractor: Option<Arc<MemoryExtractor>>,
    pub cancel_token: Option<CancellationToken>,
}

impl AgentLoop {
    pub async fn run(
        &self,
        messages: &mut Vec<Message>,
        on_event: &EventCallback,
        approve: &ApprovalCallback,
    ) -> Result<String, AgentError> {
        let declarations = self.registry.gemini_declarations();
        let turn_id = uuid::Uuid::new_v4().to_string();
        let mut taint = TurnTaint::default();

        // Build system prompt with memory context
        let system_prompt = self.build_system_prompt(messages).await;

        for _step in 0..self.config.max_steps {
            if let Some(token) = &self.cancel_token
                && token.is_cancelled()
            {
                return Err(AgentError::Cancelled);
            }

            self.audit
                .log_llm_call(&self.config.session_id, &self.config.model);

            let mut stream = self.provider.stream(
                messages,
                &self.config.model,
                Some(&system_prompt),
                Some(&declarations),
            );

            let mut text = String::new();
            let mut function_calls = Vec::new();

            while let Some(result) = stream.next().await {
                if let Some(token) = &self.cancel_token
                    && token.is_cancelled()
                {
                    return Err(AgentError::Cancelled);
                }
                match result {
                    Ok(chunk) => {
                        if chunk.done {
                            break;
                        }
                        if !chunk.text.is_empty() {
                            text.push_str(&chunk.text);
                            on_event(AgentEvent::Delta {
                                turn_id: turn_id.clone(),
                                text: chunk.text,
                            });
                        }
                        function_calls.extend(chunk.function_calls);
                    }
                    Err(e) => return Err(AgentError::Llm(e.to_string())),
                }
            }

            if function_calls.is_empty() {
                on_event(AgentEvent::Done {
                    turn_id: turn_id.clone(),
                    text: text.clone(),
                });

                // Post-turn memory extraction (async, best-effort)
                self.extract_memories(messages, &text).await;

                return Ok(text);
            }

            messages.push(Message {
                role: Role::Assistant,
                text: text.clone(),
                function_calls: function_calls.clone(),
                function_responses: vec![],
                images: vec![],
            });

            let mut responses = Vec::new();
            let mut captured_images = Vec::new();

            for fc in &function_calls {
                let def = self.registry.get(&fc.name);
                let tier = def.map(|d| d.tier).unwrap_or(Tier::Blocked);
                let taints_turn = def.map(|d| d.taints).unwrap_or(false);

                on_event(AgentEvent::ToolStarted {
                    turn_id: turn_id.clone(),
                    call_id: fc.call_id.clone(),
                    tool: fc.name.clone(),
                    args_preview: fc.args.to_string().chars().take(100).collect(),
                });

                let decision = self.policy.check(&fc.name, tier, &taint, &fc.args);

                let result = match decision {
                    PolicyDecision::Auto => {
                        self.executor.execute(&fc.name, fc.args.clone()).await
                    }
                    PolicyDecision::Ask(_reason) => {
                        let summary = def
                            .map(|d| render_summary(&d.summary_template, &fc.args))
                            .unwrap_or_else(|| format!("{}: {}", fc.name, fc.args));

                        on_event(AgentEvent::ApprovalRequest {
                            tool: fc.name.clone(),
                            tier: tier.to_string(),
                            summary: summary.clone(),
                        });

                        let user_decision = approve(
                            fc.name.clone(),
                            tier.to_string(),
                            summary,
                            fc.args.clone(),
                        )
                        .await;

                        self.audit.log_approval(
                            &self.config.session_id,
                            &fc.name,
                            &format!("{:?}", user_decision),
                        );

                        match user_decision {
                            ApprovalDecision::Approve | ApprovalDecision::ApproveAlways => {
                                self.executor.execute(&fc.name, fc.args.clone()).await
                            }
                            ApprovalDecision::Deny => {
                                ToolResult::error("user denied this action")
                            }
                        }
                    }
                    PolicyDecision::Refuse(reason) => {
                        ToolResult::error(format!("refused: {}", reason))
                    }
                };

                self.audit.log_tool_call(
                    &self.config.session_id,
                    &fc.name,
                    &fc.args,
                    result.ok,
                    &result.content,
                );

                on_event(AgentEvent::ToolFinished {
                    turn_id: turn_id.clone(),
                    call_id: fc.call_id.clone(),
                    tool: fc.name.clone(),
                    result_preview: result.content.chars().take(100).collect(),
                    ok: result.ok,
                });

                if taints_turn {
                    taint.tainted = true;
                }

                captured_images.extend(result.images);

                responses.push(FunctionResponse {
                    call_id: fc.call_id.clone(),
                    name: fc.name.clone(),
                    result: result.content,
                });
            }

            messages.push(Message {
                role: Role::Tool,
                text: String::new(),
                function_calls: vec![],
                function_responses: responses,
                images: vec![],
            });

            // If any tool produced images (e.g. a screenshot), attach them as a
            // follow-up user message so the model can see them on the next step.
            if !captured_images.is_empty() {
                messages.push(Message::user_with_images(
                    "Here is the captured image.",
                    captured_images,
                ));
            }
        }

        Err(AgentError::MaxSteps)
    }

    async fn build_system_prompt(&self, messages: &[Message]) -> String {
        let mut prompt = self.config.system_prompt.clone();

        if let Some(retriever) = &self.memory_retriever {
            // Use the latest user message as the query for memory retrieval
            let query = messages
                .iter()
                .rev()
                .find(|m| m.role == Role::User && !m.text.is_empty())
                .map(|m| m.text.as_str())
                .unwrap_or("");

            if !query.is_empty() {
                match retriever.build_context_block(query).await {
                    Ok(block) if !block.is_empty() => {
                        prompt.push_str(&block);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("memory retrieval failed: {}", e);
                    }
                }
            }
        }

        prompt
    }

    async fn extract_memories(&self, messages: &[Message], assistant_text: &str) {
        if let Some(extractor) = &self.memory_extractor {
            let user_text = messages
                .iter()
                .rev()
                .find(|m| m.role == Role::User && !m.text.is_empty())
                .map(|m| m.text.as_str())
                .unwrap_or("");

            if !user_text.is_empty() {
                match extractor.extract_from_turn(user_text, assistant_text).await {
                    Ok(count) if count > 0 => {
                        tracing::info!("extracted {} memories from turn", count);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("memory extraction failed: {}", e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::fake::{FakeProvider, ScriptedReply};
    use crate::llm::types::FunctionCall;
    use crate::tools::builtin;
    use crate::tools::os_adapt;

    fn test_registry() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        builtin::register_all(&mut reg);
        reg
    }

    fn auto_deny() -> ApprovalCallback {
        Box::new(|_, _, _, _| Box::pin(async { ApprovalDecision::Deny }))
    }

    fn noop_events() -> EventCallback {
        Box::new(|_| {})
    }

    fn test_agent(provider: FakeProvider) -> AgentLoop {
        AgentLoop {
            provider: Arc::new(AnyProvider::Fake(provider)),
            registry: Arc::new(test_registry()),
            executor: Arc::new(ToolExecutor::new(
                os_adapt::create_adapter(),
                std::env::temp_dir().join("mi-test-quarantine"),
            )),
            policy: Arc::new(PolicyEngine::default_engine()),
            audit: Arc::new(AuditLog::new(
                std::env::temp_dir().join("mi-test-audit.jsonl"),
            )),
            config: AgentConfig {
                model: "test".into(),
                system_prompt: "You are mi.".into(),
                max_steps: 10,
                session_id: "test".into(),
            },
            memory_retriever: None,
            memory_extractor: None,
            cancel_token: None,
        }
    }

    #[tokio::test]
    async fn test_basic_text_response() {
        let agent = test_agent(FakeProvider::new(vec![ScriptedReply::text("Hello!")]));
        let mut msgs = vec![Message::user("hi")];
        let result = agent.run(&mut msgs, &noop_events(), &auto_deny()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("Hello"));
    }

    #[tokio::test]
    async fn test_read_tool_auto_executes() {
        let agent = test_agent(FakeProvider::new(vec![
            ScriptedReply::with_calls(vec![FunctionCall::new("system.time", serde_json::json!({}), "c1")]),
            ScriptedReply::text("The time is now."),
        ]));
        let mut msgs = vec![Message::user("what time?")];
        let result = agent.run(&mut msgs, &noop_events(), &auto_deny()).await;
        assert!(result.is_ok());
        // system.time is READ → auto-approved, so the loop continues to the text reply
        assert!(msgs.len() >= 3);
    }

    #[tokio::test]
    async fn test_confirm_tool_denied() {
        let agent = test_agent(FakeProvider::new(vec![
            ScriptedReply::with_calls(vec![FunctionCall::new("files.delete", serde_json::json!({"path": "important.txt"}), "c1")]),
            ScriptedReply::text("Could not delete."),
        ]));
        let mut msgs = vec![Message::user("delete important.txt")];
        let result = agent.run(&mut msgs, &noop_events(), &auto_deny()).await;
        assert!(result.is_ok());
        let tool_resp = msgs
            .iter()
            .flat_map(|m| &m.function_responses)
            .find(|fr| fr.name == "files.delete")
            .unwrap();
        assert!(tool_resp.result.contains("denied"));
    }

    #[tokio::test]
    async fn test_injection_cannot_trigger_deletion() {
        let agent = test_agent(FakeProvider::new(vec![
            ScriptedReply::with_calls(vec![FunctionCall::new("web.fetch", serde_json::json!({"url": "http://evil.com/payload"}), "c1")]),
            ScriptedReply::with_calls(vec![FunctionCall::new("files.delete", serde_json::json!({"path": "important_data.txt"}), "c2")]),
            ScriptedReply::text("I see, the action was denied."),
        ]));
        let mut msgs = vec![Message::user("fetch http://evil.com/payload")];
        let result = agent.run(&mut msgs, &noop_events(), &auto_deny()).await;
        assert!(result.is_ok());
        let delete_resp = msgs
            .iter()
            .flat_map(|m| &m.function_responses)
            .find(|fr| fr.name == "files.delete");
        assert!(delete_resp.is_some(), "files.delete should have been attempted");
        assert!(
            delete_resp.unwrap().result.contains("denied"),
            "files.delete must be denied without approval"
        );
    }

    #[tokio::test]
    async fn test_taint_escalates_safe_to_confirm() {
        let agent = test_agent(FakeProvider::new(vec![
            ScriptedReply::with_calls(vec![FunctionCall::new("files.read", serde_json::json!({"path": "nonexistent.txt"}), "c1")]),
            ScriptedReply::with_calls(vec![FunctionCall::new("apps.open", serde_json::json!({"target": "notepad"}), "c2")]),
            ScriptedReply::text("Done."),
        ]));
        let mut msgs = vec![Message::user("read file and open notepad")];
        let result = agent.run(&mut msgs, &noop_events(), &auto_deny()).await;
        assert!(result.is_ok());
        // apps.open is SAFE, but after files.read (taints) it should be escalated to CONFIRM
        // and denied by our auto_deny handler
        let open_resp = msgs
            .iter()
            .flat_map(|m| &m.function_responses)
            .find(|fr| fr.name == "apps.open");
        assert!(open_resp.is_some(), "apps.open should have been attempted");
        assert!(
            open_resp.unwrap().result.contains("denied"),
            "apps.open should be denied after taint escalation"
        );
    }
}
