//! Predefined permission profile templates.
//!
//! Profiles provide named bundles of permission rules that users can select as
//! defaults instead of hand-tuning individual settings. Three built-in profiles
//! are offered — Strict, Balanced, and Permissive — alongside a Custom variant
//! for user-defined profiles.

use serde::{Deserialize, Serialize};

/// Predefined permission profile templates.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PermissionProfile {
    /// Maximum safety: all write/delete/bash operations require approval.
    Strict,
    /// Balanced: auto-approve reads, require approval for writes/bash/destructive ops.
    Balanced,
    /// Permissive: auto-approve everything except system-critical operations.
    Permissive,
    /// Custom profile loaded from config by name.
    Custom(String),
}

/// Rules that a profile translates into for the permission system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRules {
    /// Auto-approve read-only tool calls.
    pub auto_approve_read: bool,
    /// Auto-approve file write operations.
    pub auto_approve_write: bool,
    /// Auto-approve shell command execution.
    pub auto_approve_bash: bool,
    /// Auto-approve file deletion operations.
    pub auto_approve_delete: bool,
    /// Auto-approve network access (fetch, search).
    pub auto_approve_network: bool,
    /// Tool names that are always denied under this profile.
    pub deny_destructive: Vec<String>,
}

impl PermissionProfile {
    /// Return the rule set for this profile.
    pub fn rules(&self) -> ProfileRules {
        match self {
            Self::Strict => ProfileRules {
                auto_approve_read: true,
                auto_approve_write: false,
                auto_approve_bash: false,
                auto_approve_delete: false,
                auto_approve_network: false,
                deny_destructive: vec![
                    "Write".to_string(),
                    "Bash".to_string(),
                    "MultiEdit".to_string(),
                ],
            },
            Self::Balanced => ProfileRules {
                auto_approve_read: true,
                auto_approve_write: false,
                auto_approve_bash: false,
                auto_approve_delete: false,
                auto_approve_network: false,
                deny_destructive: vec![],
            },
            Self::Permissive => ProfileRules {
                auto_approve_read: true,
                auto_approve_write: true,
                auto_approve_bash: true,
                auto_approve_delete: false,
                auto_approve_network: false,
                deny_destructive: vec![],
            },
            // Custom profiles default to balanced-level rules.
            // The caller can override individual rules via config.
            Self::Custom(_) => ProfileRules {
                auto_approve_read: true,
                auto_approve_write: false,
                auto_approve_bash: false,
                auto_approve_delete: false,
                auto_approve_network: false,
                deny_destructive: vec![],
            },
        }
    }

    /// Return all built-in profile names and their short descriptions.
    pub fn all_profiles() -> Vec<(&'static str, &'static str)> {
        vec![
            ("strict", "Maximum safety: approve reads only, deny destructive tools"),
            ("balanced", "Auto-approve reads, ask for writes/bash/delete"),
            ("permissive", "Auto-approve reads, writes, bash; still deny system-critical ops"),
        ]
    }

    /// Human-readable description of this profile.
    pub fn description(&self) -> &str {
        match self {
            Self::Strict => "Maximum safety: approve reads only, deny destructive tools",
            Self::Balanced => "Auto-approve reads, ask for writes/bash/delete",
            Self::Permissive => "Auto-approve reads, writes, bash; still deny system-critical ops",
            Self::Custom(name) => name,
        }
    }
}

impl std::fmt::Display for PermissionProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Strict => write!(f, "strict"),
            Self::Balanced => write!(f, "balanced"),
            Self::Permissive => write!(f, "permissive"),
            Self::Custom(name) => write!(f, "custom:{name}"),
        }
    }
}

impl PermissionProfile {
    /// Parse a profile from a string (case-insensitive).
    ///
    /// Recognises the three built-in names and `custom:<name>` for custom
    /// profiles. Returns `None` for unrecognised strings.
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "balanced" => Some(Self::Balanced),
            "permissive" => Some(Self::Permissive),
            other => {
                if let Some(name) = other.strip_prefix("custom:") {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        return Some(Self::Custom(name));
                    }
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strict_profile_rules() {
        let rules = PermissionProfile::Strict.rules();
        assert!(rules.auto_approve_read, "strict should auto-approve reads");
        assert!(
            !rules.auto_approve_write,
            "strict should NOT auto-approve writes"
        );
        assert!(
            !rules.auto_approve_bash,
            "strict should NOT auto-approve bash"
        );
        assert!(
            !rules.auto_approve_delete,
            "strict should NOT auto-approve delete"
        );
        assert!(
            !rules.auto_approve_network,
            "strict should NOT auto-approve network"
        );
        assert_eq!(
            rules.deny_destructive,
            vec!["Write", "Bash", "MultiEdit"]
        );
    }

    #[test]
    fn test_balanced_profile_rules() {
        let rules = PermissionProfile::Balanced.rules();
        assert!(
            rules.auto_approve_read,
            "balanced should auto-approve reads"
        );
        assert!(
            !rules.auto_approve_write,
            "balanced should NOT auto-approve writes"
        );
        assert!(
            !rules.auto_approve_bash,
            "balanced should NOT auto-approve bash"
        );
        assert!(
            !rules.auto_approve_delete,
            "balanced should NOT auto-approve delete"
        );
        assert!(
            !rules.auto_approve_network,
            "balanced should NOT auto-approve network"
        );
        assert!(
            rules.deny_destructive.is_empty(),
            "balanced should have no denied tools"
        );
    }

    #[test]
    fn test_permissive_profile_rules() {
        let rules = PermissionProfile::Permissive.rules();
        assert!(
            rules.auto_approve_read,
            "permissive should auto-approve reads"
        );
        assert!(
            rules.auto_approve_write,
            "permissive should auto-approve writes"
        );
        assert!(
            rules.auto_approve_bash,
            "permissive should auto-approve bash"
        );
        assert!(
            !rules.auto_approve_delete,
            "permissive should NOT auto-approve delete"
        );
        assert!(
            !rules.auto_approve_network,
            "permissive should NOT auto-approve network"
        );
        assert!(
            rules.deny_destructive.is_empty(),
            "permissive should have no denied tools"
        );
    }

    #[test]
    fn test_custom_profile() {
        let profile = PermissionProfile::Custom("my-custom".to_string());
        let rules = profile.rules();
        assert!(
            rules.auto_approve_read,
            "custom defaults to balanced-like rules: read ok"
        );
        assert!(
            !rules.auto_approve_write,
            "custom defaults to balanced-like rules: write blocked"
        );
        assert_eq!(profile.description(), "my-custom");
        assert_eq!(profile.to_string(), "custom:my-custom");
    }

    #[test]
    fn test_all_profiles_returns_three() {
        let profiles = PermissionProfile::all_profiles();
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].0, "strict");
        assert_eq!(profiles[1].0, "balanced");
        assert_eq!(profiles[2].0, "permissive");
    }

    #[test]
    fn test_profile_serialization_roundtrip() {
        let profiles = vec![
            PermissionProfile::Strict,
            PermissionProfile::Balanced,
            PermissionProfile::Permissive,
            PermissionProfile::Custom("test".to_string()),
        ];
        for profile in &profiles {
            let json = serde_json::to_string(profile).expect("serialize");
            let roundtrip: PermissionProfile =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(&roundtrip, profile, "roundtrip failed for {profile}");
        }
    }

    #[test]
    fn test_profile_from_str() {
        assert_eq!(
            PermissionProfile::from_str_lossy("strict"),
            Some(PermissionProfile::Strict)
        );
        assert_eq!(
            PermissionProfile::from_str_lossy("BALANCED"),
            Some(PermissionProfile::Balanced)
        );
        assert_eq!(
            PermissionProfile::from_str_lossy("Permissive"),
            Some(PermissionProfile::Permissive)
        );
        assert_eq!(
            PermissionProfile::from_str_lossy("custom:my-rules"),
            Some(PermissionProfile::Custom("my-rules".to_string()))
        );
        assert_eq!(
            PermissionProfile::from_str_lossy("custom:"),
            None,
            "empty custom name should return None"
        );
        assert_eq!(
            PermissionProfile::from_str_lossy("unknown"),
            None,
            "unknown profile should return None"
        );
    }
}
