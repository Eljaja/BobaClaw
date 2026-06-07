use std::path::Path;

use regex::Regex;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Builtin,
    Trusted,
    Community,
    AgentCreated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardVerdict {
    Safe,
    Caution,
    Dangerous,
}

#[derive(Debug, Clone)]
pub struct GuardReport {
    pub verdict: GuardVerdict,
    pub findings: Vec<String>,
}

pub fn guard_skill_dir(path: &Path, trust: TrustLevel) -> GuardReport {
    let mut findings = Vec::new();
    let patterns: Vec<(&str, &str)> = vec![
        (r"\$HOME/\.ssh|\~/\.ssh", "ssh directory access"),
        (r"ignore\s+.*instructions", "prompt injection pattern"),
        (r"rm\s+-rf\s+/", "destructive root delete"),
        (
            r"curl\s+[^\n]*\$\{?\w*(KEY|TOKEN|SECRET)",
            "secret exfil via curl",
        ),
    ];

    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for (pat, desc) in &patterns {
            if Regex::new(pat)
                .map(|re| re.is_match(&content))
                .unwrap_or(false)
            {
                findings.push(format!("{}: {}", entry.path().display(), desc));
            }
        }
    }

    let verdict = if findings.is_empty() {
        GuardVerdict::Safe
    } else if findings.len() <= 2 {
        GuardVerdict::Caution
    } else {
        GuardVerdict::Dangerous
    };

    let _ = trust;
    GuardReport { verdict, findings }
}

pub fn should_allow_install(report: &GuardReport, trust: TrustLevel) -> bool {
    match (trust, report.verdict) {
        (TrustLevel::Builtin, _) => true,
        (TrustLevel::Trusted, GuardVerdict::Dangerous) => false,
        (TrustLevel::Trusted, _) => true,
        (TrustLevel::Community, GuardVerdict::Safe) => true,
        (TrustLevel::Community, _) => false,
        (TrustLevel::AgentCreated, GuardVerdict::Dangerous) => false,
        (TrustLevel::AgentCreated, _) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn flags_rm_rf_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("run.sh"), "rm -rf /").unwrap();
        let report = guard_skill_dir(dir.path(), TrustLevel::Community);
        assert_ne!(report.verdict, GuardVerdict::Safe);
    }

    #[test]
    fn safe_skill_is_safe() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("SKILL.md"), "# ok\n").unwrap();
        let report = guard_skill_dir(dir.path(), TrustLevel::Community);
        assert_eq!(report.verdict, GuardVerdict::Safe);
    }

    #[test]
    fn should_allow_install_matrix() {
        let safe = GuardReport {
            verdict: GuardVerdict::Safe,
            findings: vec![],
        };
        assert!(should_allow_install(&safe, TrustLevel::Community));
        let bad = GuardReport {
            verdict: GuardVerdict::Dangerous,
            findings: vec!["x".into()],
        };
        assert!(!should_allow_install(&bad, TrustLevel::Community));
    }
}
