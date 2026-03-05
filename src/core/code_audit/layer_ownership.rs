//! Layer ownership rules for architecture-level audit constraints.
//!
//! Rules are optional and loaded from either:
//! - `.homeboy/audit-rules.json`
//! - `homeboy.json` under `audit_rules`

use std::path::Path;

use glob_match::glob_match;

use super::conventions::DeviationKind;
use super::findings::{Finding, Severity};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuditRulesConfig {
    #[serde(default)]
    pub layer_rules: Vec<LayerRule>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LayerRule {
    pub name: String,
    pub forbid: LayerForbid,
    #[serde(default)]
    pub allow: Option<LayerAllow>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LayerForbid {
    pub glob: String,
    #[serde(default)]
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LayerAllow {
    pub glob: String,
}

pub fn analyze_layer_ownership(root: &Path) -> Vec<Finding> {
    let Some(config) = load_rules_config(root) else {
        return Vec::new();
    };

    let files = match super::walker::walk_source_files(root) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut findings = Vec::new();

    for file in files {
        let relative = match file.strip_prefix(root) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        let normalized = relative.replace('\\', "/");

        for rule in &config.layer_rules {
            if !glob_match(&rule.forbid.glob, &normalized) {
                continue;
            }

            if let Some(allow) = &rule.allow {
                if glob_match(&allow.glob, &normalized) {
                    continue;
                }
            }

            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };

            for pattern in &rule.forbid.patterns {
                if content.contains(pattern) {
                    findings.push(Finding {
                        convention: "layer_ownership".to_string(),
                        severity: Severity::Warning,
                        file: normalized.clone(),
                        description: format!(
                            "Rule '{}' violated: forbidden pattern '{}' matched",
                            rule.name, pattern
                        ),
                        suggestion: format!(
                            "Move this responsibility to the owning layer for rule '{}'",
                            rule.name
                        ),
                        kind: DeviationKind::LayerOwnershipViolation,
                    });
                }
            }
        }
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn load_rules_config(root: &Path) -> Option<AuditRulesConfig> {
    let rules_path = root.join(".homeboy").join("audit-rules.json");
    if let Ok(content) = std::fs::read_to_string(&rules_path) {
        if let Ok(cfg) = serde_json::from_str::<AuditRulesConfig>(&content) {
            return Some(cfg);
        }
    }

    let homeboy_json = root.join("homeboy.json");
    let content = std::fs::read_to_string(homeboy_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let audit_rules = value.get("audit_rules")?.clone();
    serde_json::from_value::<AuditRulesConfig>(audit_rules).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_violation_from_audit_rules_file() {
        let dir = tempfile::tempdir().unwrap();
        let homeboy_dir = dir.path().join(".homeboy");
        let steps_dir = dir.path().join("inc/Core/Steps");
        std::fs::create_dir_all(&homeboy_dir).unwrap();
        std::fs::create_dir_all(&steps_dir).unwrap();

        std::fs::write(
            homeboy_dir.join("audit-rules.json"),
            r#"{
              "layer_rules": [
                {
                  "name": "engine-owns-terminal-status",
                  "forbid": {
                    "glob": "inc/Core/Steps/**/*.php",
                    "patterns": ["JobStatus::", "datamachine_fail_job"]
                  },
                  "allow": {"glob": "inc/Abilities/Engine/**/*.php"}
                }
              ]
            }"#,
        )
        .unwrap();

        std::fs::write(
            steps_dir.join("agent_ping.php"),
            "<?php\n$status = JobStatus::FAILED;\n",
        )
        .unwrap();

        let findings = analyze_layer_ownership(dir.path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].convention, "layer_ownership");
        assert_eq!(findings[0].kind, DeviationKind::LayerOwnershipViolation);
    }

    #[test]
    fn supports_homeboy_json_audit_rules() {
        let dir = tempfile::tempdir().unwrap();
        let steps_dir = dir.path().join("inc/Core/Steps");
        std::fs::create_dir_all(&steps_dir).unwrap();

        std::fs::write(
            dir.path().join("homeboy.json"),
            r#"{
              "audit_rules": {
                "layer_rules": [
                  {
                    "name": "engine-owns-terminal-status",
                    "forbid": {
                      "glob": "inc/Core/Steps/**/*.php",
                      "patterns": ["datamachine_fail_job"]
                    }
                  }
                ]
              }
            }"#,
        )
        .unwrap();

        std::fs::write(
            steps_dir.join("agent_ping.php"),
            "<?php\ndatamachine_fail_job($job_id);\n",
        )
        .unwrap();

        let findings = analyze_layer_ownership(dir.path());
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn no_config_means_no_findings() {
        let dir = tempfile::tempdir().unwrap();
        let findings = analyze_layer_ownership(dir.path());
        assert!(findings.is_empty());
    }
}
