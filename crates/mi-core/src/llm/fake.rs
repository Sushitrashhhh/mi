use std::pin::Pin;
use std::sync::Mutex;

use futures::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::types::{Chunk, FunctionCall, Message, ModelInfo};

#[derive(Debug, Clone)]
pub struct ScriptedReply {
    pub text: String,
    pub function_calls: Vec<FunctionCall>,
}

impl ScriptedReply {
    pub fn text(s: &str) -> Self {
        Self {
            text: s.to_string(),
            function_calls: vec![],
        }
    }

    pub fn with_calls(calls: Vec<FunctionCall>) -> Self {
        Self {
            text: String::new(),
            function_calls: calls,
        }
    }
}

pub struct FakeProvider {
    replies: Mutex<Vec<ScriptedReply>>,
    pub models: Vec<ModelInfo>,
    pub calls: Mutex<Vec<Vec<Message>>>,
}

impl FakeProvider {
    pub fn new(replies: Vec<ScriptedReply>) -> Self {
        Self {
            replies: Mutex::new(replies),
            models: vec![],
            calls: Mutex::new(vec![]),
        }
    }

    pub fn with_models(mut self, models: Vec<ModelInfo>) -> Self {
        self.models = models;
        self
    }

    pub fn stream(
        &self,
        messages: &[Message],
        _model: &str,
        _system: Option<&str>,
        _tools: Option<&[serde_json::Value]>,
    ) -> Pin<Box<dyn Stream<Item = Result<Chunk, super::gemini::GeminiError>> + Send>> {
        self.calls
            .lock()
            .unwrap()
            .push(messages.to_vec());

        let reply = {
            let mut replies = self.replies.lock().unwrap();
            if replies.len() > 1 {
                replies.remove(0)
            } else {
                replies[0].clone()
            }
        };

        let (tx, rx) = mpsc::channel(16);

        tokio::spawn(async move {
            if !reply.function_calls.is_empty() {
                let _ = tx
                    .send(Ok(Chunk {
                        function_calls: reply.function_calls,
                        ..Default::default()
                    }))
                    .await;
            }
            if !reply.text.is_empty() {
                for word in reply.text.split(' ') {
                    let _ = tx
                        .send(Ok(Chunk {
                            text: format!("{word} "),
                            ..Default::default()
                        }))
                        .await;
                }
            }
            let _ = tx
                .send(Ok(Chunk {
                    done: true,
                    ..Default::default()
                }))
                .await;
        });

        Box::pin(ReceiverStream::new(rx))
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>, super::gemini::GeminiError> {
        Ok(self.models.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn test_fake_provider_text() {
        let provider = FakeProvider::new(vec![ScriptedReply::text("hello world")]);
        let msgs = vec![Message::user("hi")];
        let mut stream = provider.stream(&msgs, "test", None, None);

        let mut text = String::new();
        let mut got_done = false;
        while let Some(Ok(chunk)) = stream.next().await {
            text.push_str(&chunk.text);
            if chunk.done {
                got_done = true;
            }
        }
        assert_eq!(text.trim(), "hello world");
        assert!(got_done);
        assert_eq!(provider.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_fake_provider_function_calls() {
        let fc = FunctionCall::new("system.time", serde_json::json!({}), "c1");
        let provider = FakeProvider::new(vec![ScriptedReply::with_calls(vec![fc])]);
        let msgs = vec![Message::user("what time")];
        let mut stream = provider.stream(&msgs, "test", None, None);

        let mut got_fc = false;
        while let Some(Ok(chunk)) = stream.next().await {
            if !chunk.function_calls.is_empty() {
                assert_eq!(chunk.function_calls[0].name, "system.time");
                got_fc = true;
            }
        }
        assert!(got_fc);
    }
}
