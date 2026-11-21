use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params, functions::FunctionFlags};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Fact,
    Preference,
    Episode,
    Task,
    Person,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Preference => "preference",
            Self::Episode => "episode",
            Self::Task => "task",
            Self::Person => "person",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "fact" => Some(Self::Fact),
            "preference" => Some(Self::Preference),
            "episode" => Some(Self::Episode),
            "task" => Some(Self::Task),
            "person" => Some(Self::Person),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    UserStated,
    Extracted,
    Imported,
}

impl MemorySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserStated => "user_stated",
            Self::Extracted => "extracted",
            Self::Imported => "imported",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "user_stated" => Some(Self::UserStated),
            "extracted" => Some(Self::Extracted),
            "imported" => Some(Self::Imported),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub kind: MemoryKind,
    pub text: String,
    pub source: MemorySource,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub channel: String,
    pub started_at: String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub ts: String,
}

pub struct MemoryStore {
    conn: Mutex<Connection>,
    embed_dim: usize,
}

impl MemoryStore {
    pub fn open(path: &Path, embed_dim: usize) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        Self::init_schema(&conn)?;
        Self::register_functions(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            embed_dim,
        })
    }

    pub fn open_in_memory(embed_dim: usize) -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_schema(&conn)?;
        Self::register_functions(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            embed_dim,
        })
    }

    fn init_schema(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK(kind IN ('fact','preference','episode','task','person')),
                text TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'extracted',
                confidence REAL DEFAULT 1.0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                last_used_at TEXT,
                pinned INTEGER DEFAULT 0,
                deleted INTEGER DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
                embedding BLOB NOT NULL
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                channel TEXT NOT NULL,
                started_at TEXT NOT NULL,
                summary TEXT
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL REFERENCES conversations(id),
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                ts TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS working_context (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind) WHERE deleted = 0;
            CREATE INDEX IF NOT EXISTS idx_memories_pinned ON memories(pinned) WHERE deleted = 0 AND pinned = 1;
            CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, ts);",
        )?;

        // FTS5 for full-text search on memories (may fail if FTS5 not compiled in)
        let fts_result = conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(memory_id UNINDEXED, text);"
        );
        if let Err(e) = fts_result {
            tracing::warn!("FTS5 not available, falling back to LIKE search: {}", e);
        }

        Ok(())
    }

    fn register_functions(conn: &Connection) -> anyhow::Result<()> {
        conn.create_scalar_function(
            "cosine_similarity",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let blob1 = ctx.get_raw(0).as_blob().map_err(|e| {
                    rusqlite::Error::UserFunctionError(Box::new(e))
                })?;
                let blob2 = ctx.get_raw(1).as_blob().map_err(|e| {
                    rusqlite::Error::UserFunctionError(Box::new(e))
                })?;
                if blob1.len() != blob2.len() || blob1.len() % 4 != 0 {
                    return Ok(-1.0_f64);
                }
                let v1 = bytes_to_f32(blob1);
                let v2 = bytes_to_f32(blob2);
                Ok(cosine_similarity(&v1, &v2))
            },
        )?;
        Ok(())
    }

    // --- Memory CRUD ---

    pub fn save_memory(&self, memory: &Memory) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memories (id, kind, text, source, confidence, created_at, updated_at, last_used_at, pinned, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
             ON CONFLICT(id) DO UPDATE SET
                text = excluded.text,
                confidence = excluded.confidence,
                updated_at = excluded.updated_at,
                last_used_at = excluded.last_used_at,
                pinned = excluded.pinned",
            params![
                memory.id,
                memory.kind.as_str(),
                memory.text,
                memory.source.as_str(),
                memory.confidence,
                memory.created_at,
                memory.updated_at,
                memory.last_used_at,
                memory.pinned as i32,
            ],
        )?;

        // Sync FTS (virtual tables don't support ON CONFLICT, so delete+insert)
        let _ = conn.execute("DELETE FROM memory_fts WHERE memory_id = ?1", params![memory.id]);
        let _ = conn.execute(
            "INSERT INTO memory_fts (memory_id, text) VALUES (?1, ?2)",
            params![memory.id, memory.text],
        );

        Ok(())
    }

    pub fn get_memory(&self, id: &str) -> anyhow::Result<Option<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, text, source, confidence, created_at, updated_at, last_used_at, pinned
             FROM memories WHERE id = ?1 AND deleted = 0",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_memory(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_memories(&self, limit: usize, offset: usize) -> anyhow::Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, text, source, confidence, created_at, updated_at, last_used_at, pinned
             FROM memories WHERE deleted = 0
             ORDER BY pinned DESC, updated_at DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
            row_to_memory(row)
        })?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    pub fn get_pinned(&self) -> anyhow::Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, text, source, confidence, created_at, updated_at, last_used_at, pinned
             FROM memories WHERE deleted = 0 AND pinned = 1
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_memory)?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    pub fn forget(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let affected = conn.execute(
            "UPDATE memories SET deleted = 1, updated_at = ?2 WHERE id = ?1 AND deleted = 0",
            params![id, now],
        )?;
        // Remove from FTS
        let _ = conn.execute("DELETE FROM memory_fts WHERE memory_id = ?1", params![id]);
        // Remove embedding
        let _ = conn.execute("DELETE FROM memory_embeddings WHERE memory_id = ?1", params![id]);
        Ok(affected > 0)
    }

    pub fn pin(&self, id: &str, pinned: bool) -> anyhow::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let affected = conn.execute(
            "UPDATE memories SET pinned = ?2, updated_at = ?3 WHERE id = ?1 AND deleted = 0",
            params![id, pinned as i32, now],
        )?;
        Ok(affected > 0)
    }

    pub fn touch_memory(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET last_used_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(())
    }

    pub fn wipe(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "DELETE FROM memory_embeddings;
             DELETE FROM memory_fts;
             DELETE FROM memories;
             DELETE FROM messages;
             DELETE FROM conversations;
             DELETE FROM working_context;",
        )?;
        Ok(())
    }

    pub fn export_all(&self) -> anyhow::Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, text, source, confidence, created_at, updated_at, last_used_at, pinned
             FROM memories WHERE deleted = 0
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], row_to_memory)?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    // --- Embeddings ---

    pub fn save_embedding(&self, memory_id: &str, embedding: &[f32]) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let blob = f32_to_bytes(embedding);
        conn.execute(
            "INSERT INTO memory_embeddings (memory_id, embedding) VALUES (?1, ?2)
             ON CONFLICT(memory_id) DO UPDATE SET embedding = excluded.embedding",
            params![memory_id, blob],
        )?;
        Ok(())
    }

    pub fn search_similar(&self, query_embedding: &[f32], limit: usize) -> anyhow::Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().unwrap();
        let query_blob = f32_to_bytes(query_embedding);
        let mut stmt = conn.prepare(
            "SELECT e.memory_id, cosine_similarity(e.embedding, ?1) as score
             FROM memory_embeddings e
             JOIN memories m ON m.id = e.memory_id
             WHERE m.deleted = 0
             ORDER BY score DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query_blob, limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        let mut result = Vec::new();
        for r in rows {
            result.push(r?);
        }
        Ok(result)
    }

    // --- FTS text search ---

    pub fn search_text(&self, query: &str, limit: usize) -> anyhow::Result<Vec<(String, f64)>> {
        let conn = self.conn.lock().unwrap();

        // Try FTS5 first
        let fts_result: Result<Vec<(String, f64)>, _> = (|| {
            let mut stmt = conn.prepare(
                "SELECT memory_id, rank FROM memory_fts WHERE memory_fts MATCH ?1
                 ORDER BY rank LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![query, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut result = Vec::new();
            for r in rows {
                result.push(r?);
            }
            Ok::<_, rusqlite::Error>(result)
        })();

        match fts_result {
            Ok(results) => Ok(results),
            Err(_) => {
                // Fallback: LIKE search
                let pattern = format!("%{}%", query);
                let mut stmt = conn.prepare(
                    "SELECT id, 1.0 as score FROM memories
                     WHERE deleted = 0 AND text LIKE ?1
                     ORDER BY pinned DESC, updated_at DESC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![pattern, limit as i64], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })?;
                let mut result = Vec::new();
                for r in rows {
                    result.push(r?);
                }
                Ok(result)
            }
        }
    }

    // --- Conversations ---

    pub fn save_conversation(&self, conv: &Conversation) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO conversations (id, channel, started_at, summary) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET summary = excluded.summary",
            params![conv.id, conv.channel, conv.started_at, conv.summary],
        )?;
        Ok(())
    }

    pub fn save_message(&self, msg: &ConversationMessage) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO messages (id, conversation_id, role, content, ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg.id, msg.conversation_id, msg.role, msg.content, msg.ts],
        )?;
        Ok(())
    }

    pub fn update_conversation_summary(&self, id: &str, summary: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE conversations SET summary = ?2 WHERE id = ?1",
            params![id, summary],
        )?;
        Ok(())
    }

    // --- Working context ---

    pub fn set_working_context(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO working_context (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now],
        )?;
        Ok(())
    }

    pub fn get_working_context(&self) -> anyhow::Result<std::collections::HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM working_context")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let (k, v) = r?;
            map.insert(k, v);
        }
        Ok(map)
    }

    pub fn embed_dim(&self) -> usize {
        self.embed_dim
    }

    pub fn memory_count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE deleted = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<Memory> {
    let kind_str: String = row.get(1)?;
    let source_str: String = row.get(3)?;
    let pinned_int: i32 = row.get(8)?;
    Ok(Memory {
        id: row.get(0)?,
        kind: MemoryKind::from_str(&kind_str).unwrap_or(MemoryKind::Fact),
        text: row.get(2)?,
        source: MemorySource::from_str(&source_str).unwrap_or(MemorySource::Extracted),
        confidence: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_used_at: row.get(7)?,
        pinned: pinned_int != 0,
    })
}

fn f32_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for i in 0..a.len() {
        let ai = a[i] as f64;
        let bi = b[i] as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        return 0.0;
    }
    dot / denom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_str() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    fn test_memory(id: &str, text: &str) -> Memory {
        let now = now_str();
        Memory {
            id: id.to_string(),
            kind: MemoryKind::Fact,
            text: text.to_string(),
            source: MemorySource::UserStated,
            confidence: 1.0,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
            pinned: false,
        }
    }

    #[test]
    fn test_save_and_get_memory() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        let mem = test_memory("m1", "User likes short answers");
        store.save_memory(&mem).unwrap();

        let got = store.get_memory("m1").unwrap().unwrap();
        assert_eq!(got.text, "User likes short answers");
        assert_eq!(got.kind, MemoryKind::Fact);
    }

    #[test]
    fn test_forget_memory() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.save_memory(&test_memory("m1", "secret fact")).unwrap();

        assert!(store.forget("m1").unwrap());
        assert!(store.get_memory("m1").unwrap().is_none());
        // forgotten memory never resurfaces in list
        let all = store.list_memories(100, 0).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn test_pin_and_get_pinned() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.save_memory(&test_memory("m1", "pinned fact")).unwrap();
        store.save_memory(&test_memory("m2", "other fact")).unwrap();

        store.pin("m1", true).unwrap();
        let pinned = store.get_pinned().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].id, "m1");
    }

    #[test]
    fn test_text_search() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.save_memory(&test_memory("m1", "User edits in Premiere Pro")).unwrap();
        store.save_memory(&test_memory("m2", "User likes cats")).unwrap();

        let results = store.search_text("Premiere", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "m1");
    }

    #[test]
    fn test_embedding_search() {
        let store = MemoryStore::open_in_memory(4).unwrap();
        store.save_memory(&test_memory("m1", "fact one")).unwrap();
        store.save_memory(&test_memory("m2", "fact two")).unwrap();

        store.save_embedding("m1", &[1.0, 0.0, 0.0, 0.0]).unwrap();
        store.save_embedding("m2", &[0.0, 1.0, 0.0, 0.0]).unwrap();

        let results = store.search_similar(&[0.9, 0.1, 0.0, 0.0], 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "m1"); // most similar to [1,0,0,0]
        assert!(results[0].1 > results[1].1);
    }

    #[test]
    fn test_cosine_similarity_fn() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 1e-6);
    }

    #[test]
    fn test_wipe() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.save_memory(&test_memory("m1", "fact")).unwrap();
        assert_eq!(store.memory_count().unwrap(), 1);

        store.wipe().unwrap();
        assert_eq!(store.memory_count().unwrap(), 0);
    }

    #[test]
    fn test_working_context() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.set_working_context("active_window", "Chrome - YouTube").unwrap();

        let ctx = store.get_working_context().unwrap();
        assert_eq!(ctx.get("active_window").unwrap(), "Chrome - YouTube");
    }

    #[test]
    fn test_conversations() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        let conv = Conversation {
            id: "c1".into(),
            channel: "cli".into(),
            started_at: now_str(),
            summary: None,
        };
        store.save_conversation(&conv).unwrap();

        let msg = ConversationMessage {
            id: "msg1".into(),
            conversation_id: "c1".into(),
            role: "user".into(),
            content: "hello".into(),
            ts: now_str(),
        };
        store.save_message(&msg).unwrap();

        store.update_conversation_summary("c1", "User said hello").unwrap();
    }

    #[test]
    fn test_export() {
        let store = MemoryStore::open_in_memory(768).unwrap();
        store.save_memory(&test_memory("m1", "fact one")).unwrap();
        store.save_memory(&test_memory("m2", "fact two")).unwrap();

        let exported = store.export_all().unwrap();
        assert_eq!(exported.len(), 2);
    }
}
