use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::tools::registry::Tier;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Auto,
    Ask(String),
    Refuse(String),
}

#[derive(Debug, Clone, Default)]
pub struct TurnTaint {
    pub tainted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub shell_allowlist: HashSet<String>,
    #[serde(default)]
    pub always_approve: HashSet<String>,
}

pub struct PolicyEngine {
    pub config: PolicyConfig,
}

impl PolicyEngine {
    pub fn new(config: PolicyConfig) -> Self {
        Self { config }
    }

    pub fn default_engine() -> Self {
        Self::new(PolicyConfig::default())
    }

    pub fn check(
        &self,
        tool_name: &str,
        tier: Tier,
        taint: &TurnTaint,
        args: &serde_json::Value,
    ) -> PolicyDecision {
        if tier == Tier::Blocked {
            return PolicyDecision::Refuse(format!("'{}' is BLOCKED", tool_name));
        }

        if self.config.always_approve.contains(tool_name) {
            return PolicyDecision::Auto;
        }

        if tool_name == "shell.run" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let base_cmd = cmd.split_whitespace().next().unwrap_or("");
                if self.config.shell_allowlist.contains(base_cmd) {
                    return PolicyDecision::Auto;
                }
            }
            return PolicyDecision::Ask("shell commands require approval".into());
        }

        let effective_tier = if taint.tainted && tier == Tier::Safe {
            Tier::Confirm
        } else {
            tier
        };

        match effective_tier {
            Tier::Read | Tier::Safe => PolicyDecision::Auto,
            Tier::Confirm => PolicyDecision::Ask(format!("'{}' requires approval", tool_name)),
            Tier::Blocked => unreachable!(),
        }
    }
}

impl PolicyConfig {
    pub fn load() -> Self {
        let path = crate::config::mi_policy_path();
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_tier_auto() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint::default();
        let decision = engine.check("system.time", Tier::Read, &taint, &serde_json::json!({}));
        assert_eq!(decision, PolicyDecision::Auto);
    }

    #[test]
    fn test_safe_tier_auto() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint::default();
        let decision = engine.check("apps.open", Tier::Safe, &taint, &serde_json::json!({}));
        assert_eq!(decision, PolicyDecision::Auto);
    }

    #[test]
    fn test_confirm_tier_asks() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint::default();
        let decision =
            engine.check("files.delete", Tier::Confirm, &taint, &serde_json::json!({}));
        assert!(matches!(decision, PolicyDecision::Ask(_)));
    }

    #[test]
    fn test_blocked_tier_refused() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint::default();
        let decision =
            engine.check("payments.send", Tier::Blocked, &taint, &serde_json::json!({}));
        assert!(matches!(decision, PolicyDecision::Refuse(_)));
    }

    #[test]
    fn test_taint_escalates_safe_to_confirm() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint { tainted: true };
        let decision = engine.check("apps.open", Tier::Safe, &taint, &serde_json::json!({}));
        assert!(matches!(decision, PolicyDecision::Ask(_)));
    }

    #[test]
    fn test_taint_does_not_affect_read() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint { tainted: true };
        let decision = engine.check("system.time", Tier::Read, &taint, &serde_json::json!({}));
        assert_eq!(decision, PolicyDecision::Auto);
    }

    #[test]
    fn test_shell_always_confirm() {
        let engine = PolicyEngine::default_engine();
        let taint = TurnTaint::default();
        let decision = engine.check(
            "shell.run",
            Tier::Confirm,
            &taint,
            &serde_json::json!({"command": "rm -rf /"}),
        );
        assert!(matches!(decision, PolicyDecision::Ask(_)));
    }

    #[test]
    fn test_shell_allowlist() {
        let mut config = PolicyConfig::default();
        config.shell_allowlist.insert("ls".into());
        let engine = PolicyEngine::new(config);
        let taint = TurnTaint::default();
        let decision = engine.check(
            "shell.run",
            Tier::Confirm,
            &taint,
            &serde_json::json!({"command": "ls -la"}),
        );
        assert_eq!(decision, PolicyDecision::Auto);
    }

    #[test]
    fn test_always_approve() {
        let mut config = PolicyConfig::default();
        config.always_approve.insert("files.delete".into());
        let engine = PolicyEngine::new(config);
        let taint = TurnTaint::default();
        let decision =
            engine.check("files.delete", Tier::Confirm, &taint, &serde_json::json!({}));
        assert_eq!(decision, PolicyDecision::Auto);
    }
}
