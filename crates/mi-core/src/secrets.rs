use thiserror::Error;

const SERVICE_NAME: &str = "mi-assistant";
const LEGACY_SERVICE_NAME: &str = "mi-core";
const GEMINI_KEY_USER: &str = "gemini-api-key";

#[derive(Debug, Error)]
pub enum SecretsError {
    #[error("keyring error: {0}")]
    Keyring(String),
}

pub fn get_gemini_key() -> Result<Option<String>, SecretsError> {
    // Try current service name first, then legacy (Python era)
    for svc in [SERVICE_NAME, LEGACY_SERVICE_NAME] {
        let entry = keyring::Entry::new(svc, GEMINI_KEY_USER)
            .map_err(|e| SecretsError::Keyring(e.to_string()))?;
        match entry.get_password() {
            Ok(key) if !key.is_empty() => return Ok(Some(key)),
            Ok(_) | Err(keyring::Error::NoEntry) => continue,
            Err(e) => return Err(SecretsError::Keyring(e.to_string())),
        }
    }
    Ok(None)
}

pub fn set_gemini_key(key: &str) -> Result<(), SecretsError> {
    let entry = keyring::Entry::new(SERVICE_NAME, GEMINI_KEY_USER)
        .map_err(|e| SecretsError::Keyring(e.to_string()))?;
    entry
        .set_password(key)
        .map_err(|e| SecretsError::Keyring(e.to_string()))?;
    Ok(())
}

pub fn delete_gemini_key() -> Result<(), SecretsError> {
    let entry = keyring::Entry::new(SERVICE_NAME, GEMINI_KEY_USER)
        .map_err(|e| SecretsError::Keyring(e.to_string()))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretsError::Keyring(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    // Keyring tests require OS credential store; skip in CI.
    // We test the API shape here, not the OS integration.
    use super::*;

    #[test]
    fn test_service_name_constants() {
        assert_eq!(SERVICE_NAME, "mi-assistant");
        assert_eq!(GEMINI_KEY_USER, "gemini-api-key");
    }
}
