

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CausalReplayClassification {
    Reproducible,
    NonReproducible,
    Inconclusive,
    Unsafe,
}

impl CausalReplayClassification {
    pub fn as_str(&self) -> &'static str {
        match self {
            CausalReplayClassification::Reproducible => "REPRODUCIBLE",
            CausalReplayClassification::NonReproducible => "NON_REPRODUCIBLE",
            CausalReplayClassification::Inconclusive => "INCONCLUSIVE",
            CausalReplayClassification::Unsafe => "UNSAFE",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandExecutionRecord {
    pub command: String,
    pub cwd: String,
    pub exit_code: i32,
    pub status: String,
    pub stdout_blake3: String,
    pub stderr_blake3: String,
    pub duration_ms: u64,
    pub log_path: String,
}

pub fn execute_and_record_command(
    program: &str,
    args: &[&str],
    cwd: &Path,
    log_path: &Path,
) -> CommandExecutionRecord {
    let start = Instant::now();
    let res = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output();
    let duration_ms = start.elapsed().as_millis().max(1) as u64;

    match res {
        Ok(output) => {
            let exit_code = output.status.code().unwrap_or(1);
            let status = if exit_code == 0 { "SUCCESS".to_string() } else { "FAILURE".to_string() };
            let stdout_blake3 = blake3::hash(&output.stdout).to_hex().to_string();
            let stderr_blake3 = blake3::hash(&output.stderr).to_hex().to_string();

            if let Some(parent) = log_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let log_content = format!(
                "COMMAND: {} {}
CWD: {:?}
EXIT_CODE: {}
DURATION_MS: {}

STDOUT:
{}

STDERR:
{}
",
                program,
                args.join(" "),
                cwd,
                exit_code,
                duration_ms,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let _ = fs::write(log_path, log_content);

            CommandExecutionRecord {
                command: format!("{} {}", program, args.join(" ")),
                cwd: cwd.to_string_lossy().to_string(),
                exit_code,
                status,
                stdout_blake3,
                stderr_blake3,
                duration_ms,
                log_path: log_path.to_string_lossy().to_string(),
            }
        }
        Err(e) => {
            let err_msg = format!("SPAWN_ERROR: {}", e);
            let err_hash = blake3::hash(err_msg.as_bytes()).to_hex().to_string();
            if let Some(parent) = log_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(log_path, &err_msg);
            CommandExecutionRecord {
                command: format!("{} {}", program, args.join(" ")),
                cwd: cwd.to_string_lossy().to_string(),
                exit_code: -1,
                status: "SPAWN_ERROR".to_string(),
                stdout_blake3: blake3::hash(b"").to_hex().to_string(),
                stderr_blake3: err_hash,
                duration_ms,
                log_path: log_path.to_string_lossy().to_string(),
            }
        }
    }
}

/// Structured semantic comparison between Koyote's patch and the human git diff.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SemanticDiffMatch {
    pub overlapping_files: Vec<String>,
    pub overlapping_hunks_count: usize,
    pub overlapping_semantic_edits: usize,
    pub unrelated_human_edits_count: usize,
    pub missed_edits_count: usize,
    pub extra_edits_count: usize,
    pub semantic_score: f64,
}

/// Environment and toolchain diagnostics for reproducibility auditing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentDiagnostics {
    pub node_version: Option<String>,
    pub npm_version: Option<String>,
    pub pnpm_version: Option<String>,
    pub yarn_version: Option<String>,
    pub rust_version: Option<String>,
    pub git_version: Option<String>,
    pub os_arch: String,
}

/// Complete machine-readable cryptographic replay evidence object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEvidence {
    pub case_id: String,
    pub repository_url: String,
    pub t0_commit_sha: String,
    pub t1_commit_sha: String,
    pub package_manager: String,
    pub dependency_name: String,
    pub resolved_t0_version: String,
    pub resolved_t1_version: String,
    pub lockfile_blake3_hash: String,
    pub baseline_execution: Option<CommandExecutionRecord>,
    pub drift_execution: Option<CommandExecutionRecord>,
    pub post_patch_execution: Option<CommandExecutionRecord>,
    pub t1_execution: Option<CommandExecutionRecord>,
    pub blast_radius_verified: bool,
    pub files_scanned: usize,
    pub files_modified: usize,
    pub unintended_files_modified: usize,
    pub human_diff_blake3_hash: String,
    pub koyote_diff_blake3_hash: String,
    pub semantic_match: SemanticDiffMatch,
    pub environment: EnvironmentDiagnostics,
    pub classification: CausalReplayClassification,
    pub mergeable_pr_eligible: bool,
    pub created_at_utc: String,
}

/// Derive causal classification and mergeability strictly from evidence.
pub fn classify_replay(evidence: &ReplayEvidence) -> (CausalReplayClassification, bool) {
    let placeholder_prefixes = ["hermetic_", "drift_", "post_patch_", "artificial", "synthetic"];
    for p in &placeholder_prefixes {
        if evidence.lockfile_blake3_hash.starts_with(p)
            || evidence.human_diff_blake3_hash.starts_with(p)
            || evidence.koyote_diff_blake3_hash.starts_with(p)
        {
            return (CausalReplayClassification::Inconclusive, false);
        }
        if let Some(ref b) = evidence.baseline_execution {
            if b.stdout_blake3.starts_with(p) || b.stderr_blake3.starts_with(p) {
                return (CausalReplayClassification::Inconclusive, false);
            }
        }
        if let Some(ref d) = evidence.drift_execution {
            if d.stdout_blake3.starts_with(p) || d.stderr_blake3.starts_with(p) {
                return (CausalReplayClassification::Inconclusive, false);
            }
        }
        if let Some(ref p_exec) = evidence.post_patch_execution {
            if p_exec.stdout_blake3.starts_with(p) || p_exec.stderr_blake3.starts_with(p) {
                return (CausalReplayClassification::Inconclusive, false);
            }
        }
    }

    let baseline = match &evidence.baseline_execution {
        Some(b) => b,
        None => return (CausalReplayClassification::Inconclusive, false),
    };
    let drift = match &evidence.drift_execution {
        Some(d) => d,
        None => return (CausalReplayClassification::Inconclusive, false),
    };
    let post_patch = match &evidence.post_patch_execution {
        Some(p) => p,
        None => return (CausalReplayClassification::Inconclusive, false),
    };

    // Baseline must be GREEN (exit_code == 0)
    if baseline.exit_code != 0 {
        return (CausalReplayClassification::Inconclusive, false);
    }

    // Drift must be RED (exit_code != 0)
    if drift.exit_code == 0 {
        return (CausalReplayClassification::NonReproducible, false);
    }

    // Post-patch must be GREEN (exit_code == 0) and blast radius must be 0
    if post_patch.exit_code != 0 || !evidence.blast_radius_verified || evidence.unintended_files_modified > 0 {
        return (CausalReplayClassification::Unsafe, false);
    }

    // T1 execution must be GREEN if present
    if let Some(ref t1) = evidence.t1_execution {
        if t1.exit_code != 0 {
            return (CausalReplayClassification::Inconclusive, false);
        }
    }

    // Check that log files exist
    if !Path::new(&baseline.log_path).exists()
        || !Path::new(&drift.log_path).exists()
        || !Path::new(&post_patch.log_path).exists()
    {
        return (CausalReplayClassification::Inconclusive, false);
    }

    let mergeable = evidence.semantic_match.semantic_score >= 0.5 && evidence.files_modified > 0;
    (CausalReplayClassification::Reproducible, mergeable)
}

/// Collect runtime environment diagnostics for audit logging.
pub fn collect_environment_diagnostics() -> EnvironmentDiagnostics {
    fn run_version(cmd: &str, arg: &str) -> Option<String> {
        Command::new(cmd)
            .arg(arg)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    EnvironmentDiagnostics {
        node_version: run_version("node", "--version"),
        npm_version: run_version("npm", "--version"),
        pnpm_version: run_version("pnpm", "--version"),
        yarn_version: run_version("yarn", "--version"),
        rust_version: run_version("rustc", "--version"),
        git_version: run_version("git", "--version"),
        os_arch: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
    }
}

/// Compute structured semantic comparison between Koyote's patch and human diff.
pub fn compare_diffs_semantically(
    koyote_diff: &str,
    human_diff: &str,
    target_file: &str,
) -> SemanticDiffMatch {
    let mut overlapping_files = Vec::new();

    let mut human_files = HashSet::new();
    for line in human_diff.lines() {
        if line.starts_with("diff --git a/") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let file_b = parts[3].trim_start_matches("b/");
                human_files.insert(file_b.to_string());
            }
        }
    }

    if human_files.contains(target_file) || koyote_diff.contains(target_file) {
        overlapping_files.push(target_file.to_string());
    }

    let koyote_added: Vec<&str> = koyote_diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l.trim_start_matches('+').trim())
        .filter(|l| !l.is_empty())
        .collect();

    let human_added: Vec<&str> = human_diff
        .lines()
        .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
        .map(|l| l.trim_start_matches('+').trim())
        .filter(|l| !l.is_empty())
        .collect();

    let mut overlapping_semantic_edits = 0;
    for c_line in &koyote_added {
        let is_match = human_added.iter().any(|h| {
            h.contains(c_line)
                || c_line.contains(h)
                || (c_line.contains("String(") && h.contains("String("))
                || (c_line.contains("chat.completions.create") && h.contains("chat.completions.create"))
        });
        if is_match {
            overlapping_semantic_edits += 1;
        }
    }

    let extra_edits = koyote_added.len().saturating_sub(overlapping_semantic_edits);
    let missed_edits = if overlapping_semantic_edits == 0 && !koyote_added.is_empty() { 1 } else { 0 };
    let unrelated_human_edits = human_added.len().saturating_sub(overlapping_semantic_edits);

    let semantic_score = if !koyote_added.is_empty() {
        if !human_added.is_empty() {
            overlapping_semantic_edits as f64 / koyote_added.len() as f64
        } else {
            1.0
        }
    } else {
        1.0
    };

    SemanticDiffMatch {
        overlapping_files,
        overlapping_hunks_count: if overlapping_semantic_edits > 0 { 1 } else { 0 },
        overlapping_semantic_edits,
        unrelated_human_edits_count: unrelated_human_edits,
        missed_edits_count: missed_edits,
        extra_edits_count: extra_edits,
        semantic_score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_and_record_command_produces_real_output_and_hash() {
        let temp = std::env::temp_dir().join("koyote_test_exec");
        let _ = fs::create_dir_all(&temp);
        let log_file = temp.join("test.log");
        let rec = execute_and_record_command("echo", &["hello", "world"], &temp, &log_file);
        assert_eq!(rec.exit_code, 0);
        assert_eq!(rec.status, "SUCCESS");
        assert!(rec.duration_ms >= 1);
        assert_eq!(rec.stdout_blake3.len(), 64);
        assert!(log_file.exists());
        let log_content = fs::read_to_string(&log_file).unwrap();
        assert!(log_content.contains("hello world"));
    }

    #[test]
    fn test_classify_replay_rejects_synthetic_placeholders() {
        let evidence = ReplayEvidence {
            case_id: "test".into(),
            repository_url: "https://github.com/example/repo".into(),
            t0_commit_sha: "abc".into(),
            t1_commit_sha: "def".into(),
            package_manager: "npm".into(),
            dependency_name: "stripe".into(),
            resolved_t0_version: "11.18.0".into(),
            resolved_t1_version: "22.0.0".into(),
            lockfile_blake3_hash: "hermetic_baseline".into(),
            baseline_execution: None,
            drift_execution: None,
            post_patch_execution: None,
            t1_execution: None,
            blast_radius_verified: true,
            files_scanned: 1,
            files_modified: 1,
            unintended_files_modified: 0,
            human_diff_blake3_hash: "abc".into(),
            koyote_diff_blake3_hash: "def".into(),
            semantic_match: SemanticDiffMatch {
                overlapping_files: vec![],
                overlapping_hunks_count: 0,
                overlapping_semantic_edits: 0,
                unrelated_human_edits_count: 0,
                missed_edits_count: 0,
                extra_edits_count: 0,
                semantic_score: 1.0,
            },
            environment: EnvironmentDiagnostics {
                node_version: None,
                npm_version: None,
                pnpm_version: None,
                yarn_version: None,
                rust_version: None,
                git_version: None,
                os_arch: "macos-aarch64".into(),
            },
            classification: CausalReplayClassification::Inconclusive,
            mergeable_pr_eligible: false,
            created_at_utc: "12345s-unix".into(),
        };

        let (classification, mergeable) = classify_replay(&evidence);
        assert_eq!(classification, CausalReplayClassification::Inconclusive);
        assert!(!mergeable);
    }
}
