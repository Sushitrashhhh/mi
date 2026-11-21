use serde::{Deserialize, Serialize};

const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    content: EmbedContent,
    #[serde(rename = "outputDimensionality")]
    output_dimensionality: usize,
}

#[derive(Debug, Serialize)]
struct EmbedContent {
    parts: Vec<EmbedPart>,
}

#[derive(Debug, Serialize)]
struct EmbedPart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embedding: Option<EmbedValues>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct EmbedValues {
    values: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

#[derive(Debug, Serialize)]
struct BatchEmbedRequest {
    requests: Vec<EmbedRequest>,
}

#[derive(Debug, Deserialize)]
struct BatchEmbedResponse {
    embeddings: Option<Vec<EmbedValues>>,
    error: Option<ApiError>,
}

pub struct EmbeddingClient {
    client: reqwest::Client,
    api_key: String,
    model: String,
    dimensions: usize,
}

impl EmbeddingClient {
    pub fn new(api_key: &str, model: &str, dimensions: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            dimensions,
        }
    }

    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!(
            "{}/models/{}:embedContent?key={}",
            GEMINI_BASE, self.model, self.api_key
        );

        let body = EmbedRequest {
            model: format!("models/{}", self.model),
            content: EmbedContent {
                parts: vec![EmbedPart {
                    text: text.to_string(),
                }],
            },
            output_dimensionality: self.dimensions,
        };

        let resp: EmbedResponse = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.error {
            anyhow::bail!("Gemini embedding error: {}", err.message);
        }

        resp.embedding
            .map(|e| e.values)
            .ok_or_else(|| anyhow::anyhow!("no embedding in response"))
    }

    pub async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        if texts.len() == 1 {
            return Ok(vec![self.embed(texts[0]).await?]);
        }

        let url = format!(
            "{}/models/{}:batchEmbedContents?key={}",
            GEMINI_BASE, self.model, self.api_key
        );

        let requests: Vec<EmbedRequest> = texts
            .iter()
            .map(|t| EmbedRequest {
                model: format!("models/{}", self.model),
                content: EmbedContent {
                    parts: vec![EmbedPart {
                        text: t.to_string(),
                    }],
                },
                output_dimensionality: self.dimensions,
            })
            .collect();

        let body = BatchEmbedRequest { requests };

        let resp: BatchEmbedResponse = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if let Some(err) = resp.error {
            anyhow::bail!("Gemini batch embedding error: {}", err.message);
        }

        resp.embeddings
            .map(|embs| embs.into_iter().map(|e| e.values).collect())
            .ok_or_else(|| anyhow::anyhow!("no embeddings in batch response"))
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_request_serialization() {
        let req = EmbedRequest {
            model: "models/gemini-embedding-2".into(),
            content: EmbedContent {
                parts: vec![EmbedPart {
                    text: "hello".into(),
                }],
            },
            output_dimensionality: 768,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "models/gemini-embedding-2");
        assert_eq!(json["outputDimensionality"], 768);
        assert_eq!(json["content"]["parts"][0]["text"], "hello");
    }

    #[test]
    fn test_embed_response_deserialization() {
        let json = r#"{"embedding": {"values": [0.1, 0.2, 0.3]}}"#;
        let resp: EmbedResponse = serde_json::from_str(json).unwrap();
        let values = resp.embedding.unwrap().values;
        assert_eq!(values.len(), 3);
        assert!((values[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error": {"message": "invalid key"}}"#;
        let resp: EmbedResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_some());
        assert!(resp.embedding.is_none());
    }
}
