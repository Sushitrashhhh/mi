use std::path::PathBuf;

use crate::config::mi_models_dir;

use super::VoiceError;

const WHISPER_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn whisper_model_path(model_size: &str) -> PathBuf {
    mi_models_dir().join(format!("ggml-{}.bin", model_size))
}

pub fn whisper_model_url(model_size: &str) -> String {
    format!("{}/ggml-{}.bin", WHISPER_BASE_URL, model_size)
}

pub fn is_model_available(model_size: &str) -> bool {
    whisper_model_path(model_size).exists()
}

pub async fn ensure_whisper_model(model_size: &str) -> Result<PathBuf, VoiceError> {
    let path = whisper_model_path(model_size);
    if path.exists() {
        return Ok(path);
    }

    let url = whisper_model_url(model_size);
    tracing::info!("downloading whisper model '{}' from {}", model_size, url);

    download_file(&url, &path).await?;

    Ok(path)
}

async fn download_file(url: &str, dest: &PathBuf) -> Result<(), VoiceError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| VoiceError::Download(format!("failed to create dir: {}", e)))?;
    }

    let response = reqwest::get(url)
        .await
        .map_err(|e| VoiceError::Download(format!("HTTP request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(VoiceError::Download(format!(
            "HTTP {}: {}",
            response.status(),
            url
        )));
    }

    let total_size = response.content_length();
    if let Some(size) = total_size {
        let mb = size as f64 / 1_048_576.0;
        tracing::info!("download size: {:.1} MB", mb);
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| VoiceError::Download(format!("failed to read body: {}", e)))?;

    let tmp_path = dest.with_extension("part");
    std::fs::write(&tmp_path, &bytes)
        .map_err(|e| VoiceError::Download(format!("failed to write file: {}", e)))?;

    std::fs::rename(&tmp_path, dest)
        .map_err(|e| VoiceError::Download(format!("failed to rename: {}", e)))?;

    tracing::info!("model saved to {}", dest.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_path() {
        let path = whisper_model_path("tiny");
        assert!(path.to_string_lossy().contains("ggml-tiny.bin"));
    }

    #[test]
    fn test_model_url() {
        let url = whisper_model_url("tiny");
        assert!(url.contains("ggml-tiny.bin"));
        assert!(url.starts_with("https://"));
    }
}
