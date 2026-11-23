use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn mi_dir() -> PathBuf {
    dirs::home_dir()
        .expect("could not find home directory")
        .join(".mi")
}

pub fn mi_config_path() -> PathBuf {
    mi_dir().join("mi.toml")
}

pub fn mi_token_path() -> PathBuf {
    mi_dir().join("token")
}

pub fn mi_logs_dir() -> PathBuf {
    mi_dir().join("logs")
}

pub fn mi_quarantine_dir() -> PathBuf {
    mi_dir().join("quarantine")
}

pub fn mi_policy_path() -> PathBuf {
    mi_dir().join("policy.toml")
}

pub fn mi_db_path() -> PathBuf {
    mi_dir().join("mi.db")
}

pub fn mi_models_dir() -> PathBuf {
    mi_dir().join("models")
}

pub fn ensure_mi_dir() -> std::io::Result<()> {
    let dir = mi_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    let logs = mi_logs_dir();
    if !logs.exists() {
        std::fs::create_dir_all(&logs)?;
    }
    let models = mi_models_dir();
    if !models.exists() {
        std::fs::create_dir_all(&models)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_autonomy")]
    pub autonomy: Autonomy,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            autonomy: default_autonomy(),
            max_steps: default_max_steps(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    Normal,
    DryRun,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    #[serde(default)]
    pub fast: String,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub deep: String,
    #[serde(default)]
    pub vision: String,
    #[serde(default)]
    pub live: String,
    #[serde(default)]
    pub tts: String,
    #[serde(default)]
    pub embed: String,
    #[serde(default)]
    pub local: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            fast: String::new(),
            default: String::new(),
            deep: String::new(),
            vision: String::new(),
            live: String::new(),
            tts: String::new(),
            embed: String::new(),
            local: "llama3.2:3b".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default = "default_auto_save")]
    pub auto_save: AutoSave,
    #[serde(default = "default_embed_dim")]
    pub embed_dim: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            top_k: default_top_k(),
            auto_save: default_auto_save(),
            embed_dim: default_embed_dim(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutoSave {
    Off,
    StatedOnly,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    #[serde(default = "default_voice_pipeline")]
    pub pipeline: VoicePipeline,
    #[serde(default = "default_stt_model")]
    pub stt_model: String,
    #[serde(default = "default_silence_timeout")]
    pub silence_timeout_s: f32,
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f32,
    #[serde(default)]
    pub barge_in: bool,
    #[serde(default = "default_tts_backend")]
    pub tts_backend: TtsBackend,
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    #[serde(default = "default_kelados_url")]
    pub kelados_url: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            pipeline: default_voice_pipeline(),
            stt_model: default_stt_model(),
            silence_timeout_s: default_silence_timeout(),
            vad_threshold: default_vad_threshold(),
            barge_in: false,
            tts_backend: default_tts_backend(),
            tts_voice: default_tts_voice(),
            kelados_url: default_kelados_url(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoicePipeline {
    Pipeline,
    Live,
}

/// Which text-to-speech engine to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsBackend {
    /// Native Windows WinRT SpeechSynthesizer (local, no extra process).
    Windows,
    /// Kelados local server (Kokoro voices) over HTTP; needs `npx kelados serve`.
    Kelados,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MiConfig {
    #[serde(default)]
    pub core: CoreConfig,
    #[serde(default)]
    pub models: ModelsConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub voice: VoiceConfig,
}

impl MiConfig {
    pub fn load() -> anyhow::Result<Self> {
        let path = mi_config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        let config: MiConfig = toml::from_str(&text)?;
        Ok(config)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        ensure_mi_dir()?;
        let text = toml::to_string_pretty(self)?;
        std::fs::write(mi_config_path(), text)?;
        Ok(())
    }

    pub fn active_model(&self) -> &str {
        if !self.models.default.is_empty() {
            &self.models.default
        } else if !self.models.fast.is_empty() {
            &self.models.fast
        } else {
            ""
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".into()
}
fn default_port() -> u16 {
    7777
}
fn default_autonomy() -> Autonomy {
    Autonomy::Normal
}
fn default_max_steps() -> u32 {
    12
}
fn default_top_k() -> usize {
    6
}
fn default_auto_save() -> AutoSave {
    AutoSave::StatedOnly
}
fn default_embed_dim() -> usize {
    768
}
fn default_voice_pipeline() -> VoicePipeline {
    VoicePipeline::Pipeline
}
fn default_stt_model() -> String {
    "tiny".into()
}
fn default_silence_timeout() -> f32 {
    8.0
}
fn default_vad_threshold() -> f32 {
    0.02
}
fn default_tts_backend() -> TtsBackend {
    TtsBackend::Kelados
}
fn default_tts_voice() -> String {
    "af_heart".into()
}
fn default_kelados_url() -> String {
    "http://127.0.0.1:8880".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MiConfig::default();
        assert_eq!(config.core.host, "127.0.0.1");
        assert_eq!(config.core.port, 7777);
        assert_eq!(config.core.autonomy, Autonomy::Normal);
        assert_eq!(config.core.max_steps, 12);
        assert_eq!(config.models.local, "llama3.2:3b");
    }

    #[test]
    fn test_parse_config_toml() {
        let toml_str = r#"
[core]
host = "127.0.0.1"
port = 7777
autonomy = "dry_run"

[models]
default = "gemini-2.5-flash"
"#;
        let config: MiConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.core.autonomy, Autonomy::DryRun);
        assert_eq!(config.models.default, "gemini-2.5-flash");
    }

    #[test]
    fn test_active_model() {
        let mut config = MiConfig::default();
        assert_eq!(config.active_model(), "");

        config.models.default = "gemini-2.5-flash".into();
        assert_eq!(config.active_model(), "gemini-2.5-flash");
    }
}
