pub mod fake;
pub mod gemini;
pub mod types;

use std::pin::Pin;

use futures::{Stream, StreamExt};

pub enum AnyProvider {
    Gemini(gemini::GeminiProvider),
    Fake(fake::FakeProvider),
}

impl AnyProvider {
    pub fn stream(
        &self,
        messages: &[types::Message],
        model: &str,
        system: Option<&str>,
        tools: Option<&[serde_json::Value]>,
    ) -> Pin<Box<dyn Stream<Item = Result<types::Chunk, gemini::GeminiError>> + Send>> {
        match self {
            AnyProvider::Gemini(p) => p.stream(messages, model, system, tools),
            AnyProvider::Fake(p) => p.stream(messages, model, system, tools),
        }
    }

    /// Non-streaming generate: collects the full text response.
    /// Used for cheap utility calls (memory extraction, summarization).
    pub async fn generate(
        &self,
        messages: &[types::Message],
        model: &str,
        system: Option<&str>,
        tools: Option<&[serde_json::Value]>,
    ) -> Result<String, gemini::GeminiError> {
        let mut stream = self.stream(messages, model, system, tools);
        let mut text = String::new();
        while let Some(result) = stream.next().await {
            let chunk = result?;
            if chunk.done {
                break;
            }
            text.push_str(&chunk.text);
        }
        Ok(text)
    }
}
