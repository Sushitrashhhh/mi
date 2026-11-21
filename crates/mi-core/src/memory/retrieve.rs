use std::collections::HashMap;
use std::sync::Arc;

use super::embed::EmbeddingClient;
use super::store::{Memory, MemoryStore};

pub struct MemoryRetriever {
    store: Arc<MemoryStore>,
    embedder: Arc<EmbeddingClient>,
    top_k: usize,
}

impl MemoryRetriever {
    pub fn new(store: Arc<MemoryStore>, embedder: Arc<EmbeddingClient>, top_k: usize) -> Self {
        Self {
            store,
            embedder,
            top_k,
        }
    }

    /// Retrieve the most relevant memories for a query using hybrid search:
    /// vector similarity + FTS keyword search, merged with Reciprocal Rank Fusion.
    pub async fn retrieve(&self, query: &str) -> anyhow::Result<Vec<Memory>> {
        // Always include pinned memories
        let pinned = self.store.get_pinned()?;
        let pinned_ids: std::collections::HashSet<String> =
            pinned.iter().map(|m| m.id.clone()).collect();

        // Vector search
        let vector_results = match self.embedder.embed(query).await {
            Ok(embedding) => self.store.search_similar(&embedding, self.top_k * 2)?,
            Err(e) => {
                tracing::warn!("embedding failed, using text search only: {}", e);
                vec![]
            }
        };

        // FTS/keyword search
        let text_results = self.store.search_text(query, self.top_k * 2)?;

        // Reciprocal Rank Fusion to merge both rankings
        let merged = reciprocal_rank_fusion(&vector_results, &text_results);

        // Collect top-k unique memory IDs (excluding already-pinned)
        let mut result_ids: Vec<String> = Vec::new();
        for (id, _score) in &merged {
            if result_ids.len() >= self.top_k {
                break;
            }
            if !pinned_ids.contains(id) && !result_ids.contains(id) {
                result_ids.push(id.clone());
            }
        }

        // Fetch full memories and touch their last_used_at
        let mut result = pinned;
        for id in &result_ids {
            if let Some(mem) = self.store.get_memory(id)? {
                let _ = self.store.touch_memory(id);
                result.push(mem);
            }
        }

        Ok(result)
    }

    /// Build the context string to inject into the system prompt.
    pub async fn build_context_block(&self, query: &str) -> anyhow::Result<String> {
        let memories = self.retrieve(query).await?;
        let working_ctx = self.store.get_working_context()?;

        if memories.is_empty() && working_ctx.is_empty() {
            return Ok(String::new());
        }

        let mut block = String::new();

        if !memories.is_empty() {
            block.push_str("\n## Your memories about this user\n");
            for mem in &memories {
                let pin_marker = if mem.pinned { " (pinned)" } else { "" };
                block.push_str(&format!(
                    "- [{}] {}{}\n",
                    mem.kind.as_display(),
                    mem.text,
                    pin_marker,
                ));
            }
        }

        if !working_ctx.is_empty() {
            block.push_str("\n## Current context\n");
            for (key, value) in &working_ctx {
                block.push_str(&format!("- {}: {}\n", key, value));
            }
        }

        Ok(block)
    }
}

/// Reciprocal Rank Fusion: merges two ranked lists into one.
/// RRF score for a document = sum over lists of 1/(k + rank), where k=60 (standard constant).
fn reciprocal_rank_fusion(
    list_a: &[(String, f64)],
    list_b: &[(String, f64)],
) -> Vec<(String, f64)> {
    const K: f64 = 60.0;
    let mut scores: HashMap<String, f64> = HashMap::new();

    for (rank, (id, _)) in list_a.iter().enumerate() {
        *scores.entry(id.clone()).or_default() += 1.0 / (K + rank as f64 + 1.0);
    }
    for (rank, (id, _)) in list_b.iter().enumerate() {
        *scores.entry(id.clone()).or_default() += 1.0 / (K + rank as f64 + 1.0);
    }

    let mut merged: Vec<(String, f64)> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged
}

impl super::store::MemoryKind {
    fn as_display(&self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Episode => "episode",
            Self::Task => "task",
            Self::Person => "person",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rrf_merges_rankings() {
        let list_a = vec![
            ("a".into(), 0.9),
            ("b".into(), 0.8),
            ("c".into(), 0.7),
        ];
        let list_b = vec![
            ("b".into(), 0.95),
            ("a".into(), 0.85),
            ("d".into(), 0.75),
        ];
        let merged = reciprocal_rank_fusion(&list_a, &list_b);
        // "a" appears in both lists (rank 1 in A, rank 2 in B)
        // "b" appears in both lists (rank 2 in A, rank 1 in B)
        // Both should be tied, but "b" = 1/62 + 1/61, "a" = 1/61 + 1/62 — same
        assert!(merged.len() >= 3);
        // All items present
        let ids: Vec<&str> = merged.iter().map(|(id, _)| id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"b"));
        assert!(ids.contains(&"c"));
        assert!(ids.contains(&"d"));
    }

    #[test]
    fn test_rrf_single_list() {
        let list_a = vec![("x".into(), 1.0), ("y".into(), 0.5)];
        let list_b: Vec<(String, f64)> = vec![];
        let merged = reciprocal_rank_fusion(&list_a, &list_b);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].0, "x"); // higher rank
    }
}
