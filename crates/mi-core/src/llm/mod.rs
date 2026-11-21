pub mod fake;
pub mod gemini;
pub mod types;

use std::pin::Pin;

use futures::Stream;

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
}
