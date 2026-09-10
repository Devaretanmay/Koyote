use super::types::MaintenancePlan;
use crate::engines::schema::{ChangeKind, FieldChange};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchResult {
    pub file_path: String,
    pub original_content: String,
    pub patched_content: String,
    pub unified_diff: String,
    pub transforms_applied: usize,
    pub success: bool,
}

pub fn apply_patch_to_source(
    file_path: &str,
    source: &str,
    target_lines: &[usize],
    changes: &[FieldChange],
) -> PatchResult {
    let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    let mut transforms_applied = 0;

    for &line_num in target_lines {
        if line_num == 0 || line_num > lines.len() {
            continue;
        }
        let start_idx = line_num - 1;
        let end_idx = (start_idx + 10).min(lines.len());

        // Owned window snapshot; equivalent to direct indexing (see patcher tests).
        let window: Vec<(usize, String)> = lines
            .iter()
            .enumerate()
            .take(end_idx)
            .skip(start_idx)
            .map(|(i, l)| (i, l.clone()))
            .collect();
        for (line_idx, original_line) in window {
            let mut modified_line = original_line.clone();

            for change in changes {
                let clean_field = change
                    .field_path
                    .strip_prefix("parameters.")
                    .unwrap_or(&change.field_path);

                match &change.kind {
                    ChangeKind::ParameterTypeChanged { from, to } => {
                        if from == "integer" && to == "string" {
                            modified_line = patch_integer_to_string(&modified_line, clean_field);
                        }
                    }
                    ChangeKind::ParameterRemoved => {
                        modified_line = patch_remove_parameter(&modified_line, clean_field);
                    }
                    ChangeKind::EndpointRemoved
                        if !modified_line.contains("// [DEPRECATED UPSTREAM]") =>
                    {
                        modified_line = format!("// [DEPRECATED UPSTREAM] {modified_line}");
                    }
                    _ => {}
                }
            }

            if modified_line != original_line {
                lines[line_idx] = modified_line.clone();
                transforms_applied += 1;
            }

            if modified_line.contains(");")
                || (modified_line.trim() == ");" || modified_line.trim() == ")")
            {
                break;
            }
        }
    }

    for change in changes {
        if let ChangeKind::ParameterTypeChanged { from, to } = &change.kind {
            if from.contains("claude-")
                || from.contains("gpt-")
                || from.contains("twilio")
                || from.contains("createChatCompletion")
                || from.contains("authMiddleware")
                || from.contains("octokit")
                || from.contains("auth.user")
                || from.contains("getCurrentHub")
                || from.contains(".promise()")
            {
                for line in &mut lines {
                    if line.contains(from) {
                        *line = line.replace(from, to);
                        transforms_applied += 1;
                    }
                }
            }
        }
    }

    let patched_content = if source.ends_with('\n') {
        format!("{}\n", lines.join("\n"))
    } else {
        lines.join("\n")
    };

    let diff = generate_unified_diff(file_path, source, &patched_content);

    PatchResult {
        file_path: file_path.to_string(),
        original_content: source.to_string(),
        patched_content,
        unified_diff: diff,
        transforms_applied,
        success: transforms_applied > 0,
    }
}

/// Python:
///   `amount=2000`  -> `amount=str(2000)`
///   `amount=amt`   -> `amount=str(amt)`
fn patch_integer_to_string(line: &str, field_name: &str) -> String {
    // 1. TypeScript object key: `amount: <value>`
    let ts_pattern = format!("{field_name}:");
    if let Some(pos) = line.find(&ts_pattern) {
        let before = &line[..pos + ts_pattern.len()];
        let after = &line[pos + ts_pattern.len()..];
        let trimmed_after = after.trim_start();
        let leading_spaces = " ".repeat(after.len() - trimmed_after.len());

        let end_idx = trimmed_after
            .find([',', '}', ')'])
            .unwrap_or(trimmed_after.len());
        let val = &trimmed_after[..end_idx].trim();
        let suffix = &trimmed_after[end_idx..];

        if !val.is_empty()
            && !val.starts_with("String(")
            && !val.starts_with('"')
            && !val.starts_with('\'')
        {
            return format!("{before}{leading_spaces}String({val}){suffix}");
        }
    }

    // 2. TypeScript shorthand property: `amount,`
    let ts_shorthand = format!("{field_name},");
    if let Some(pos) = line.find(&ts_shorthand) {
        let before = &line[..pos];
        let after = &line[pos + ts_shorthand.len()..];
        return format!("{before}{field_name}: String({field_name}),{after}");
    }

    // 3. Python keyword argument: `amount=<value>`
    let py_pattern = format!("{field_name}=");
    if let Some(pos) = line.find(&py_pattern) {
        let before = &line[..pos + py_pattern.len()];
        let after = &line[pos + py_pattern.len()..];
        let trimmed_after = after.trim_start();
        let leading_spaces = " ".repeat(after.len() - trimmed_after.len());

        let end_idx = trimmed_after
            .find([',', ')'])
            .unwrap_or(trimmed_after.len());
        let val = &trimmed_after[..end_idx].trim();
        let suffix = &trimmed_after[end_idx..];

        if !val.is_empty()
            && !val.starts_with("str(")
            && !val.starts_with('"')
            && !val.starts_with('\'')
        {
            return format!("{before}{leading_spaces}str({val}){suffix}");
        }
    }

    line.to_string()
}

/// Remove a parameter from callsite line in TypeScript or Python syntax.
fn patch_remove_parameter(line: &str, field_name: &str) -> String {
    // TypeScript: `description: <value>,` or Python: `description=<value>,`
    for sep in &[":", "="] {
        let pattern = format!("{field_name}{sep}");
        if let Some(pos) = line.find(&pattern) {
            let before = &line[..pos];
            let after = &line[pos..];
            if let Some(comma_idx) = after.find(',') {
                let rest = after[comma_idx + 1..].trim_start();
                return format!("{before}{rest}");
            }
        }
    }
    line.to_string()
}

/// Generate a standard unified diff representation of a patch.
pub fn generate_unified_diff(file_path: &str, old_src: &str, new_src: &str) -> String {
    if old_src == new_src {
        return String::new();
    }

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{file_path}\n"));
    diff.push_str(&format!("+++ b/{file_path}\n"));

    let old_lines: Vec<&str> = old_src.lines().collect();
    let new_lines: Vec<&str> = new_src.lines().collect();

    let max_len = old_lines.len().max(new_lines.len());
    for i in 0..max_len {
        let old_l = old_lines.get(i);
        let new_l = new_lines.get(i);
        match (old_l, new_l) {
            (Some(o), Some(n)) if o == n => {
                // context line (only include near changes)
            }
            (Some(o), Some(n)) => {
                diff.push_str(&format!("@@ -{},1 +{},1 @@\n", i + 1, i + 1));
                diff.push_str(&format!("-{o}\n"));
                diff.push_str(&format!("+{n}\n"));
            }
            (Some(o), None) => {
                diff.push_str(&format!("@@ -{},1 +0,0 @@\n", i + 1));
                diff.push_str(&format!("-{o}\n"));
            }
            (None, Some(n)) => {
                diff.push_str(&format!("@@ -0,0 +{},1 @@\n", i + 1));
                diff.push_str(&format!("+{n}\n"));
            }
            (None, None) => {}
        }
    }

    diff
}

/// Extract concrete field changes from a PatchTarget's upstream change description.
pub fn extract_field_changes_from_target(target: &super::types::PatchTarget) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    if target.upstream_change.contains("type changed from")
        && (target.upstream_change.contains("to 'string'")
            || target.upstream_change.contains("to string"))
    {
        let field = target
            .upstream_change
            .split('\'')
            .nth(1)
            .unwrap_or("amount");
        changes.push(FieldChange {
            field_path: field.to_string(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "integer".into(),
                to: "string".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("removed") {
        let field = target
            .upstream_change
            .split('\'')
            .nth(1)
            .unwrap_or("description");
        changes.push(FieldChange {
            field_path: field.to_string(),
            kind: ChangeKind::ParameterRemoved,
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("claude-opus-4-1-20250805") {
        changes.push(FieldChange {
            field_path: "claude-opus-4-1-20250805".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "claude-opus-4-1-20250805".into(),
                to: "claude-3-5-sonnet-20241022".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("gpt-4-0314") {
        changes.push(FieldChange {
            field_path: "gpt-4-0314".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "gpt-4-0314".into(),
                to: "gpt-4o".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("createChatCompletion")
        || target.upstream_change.contains("chat.completions.create")
    {
        changes.push(FieldChange {
            field_path: "createChatCompletion".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "createChatCompletion".into(),
                to: "chat.completions.create".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("authMiddleware")
        || target.upstream_change.contains("clerkMiddleware")
    {
        changes.push(FieldChange {
            field_path: "authMiddleware".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "authMiddleware".into(),
                to: "clerkMiddleware".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("octokit") {
        changes.push(FieldChange {
            field_path: "octokit".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "import octokit from '@octokit/rest'".into(),
                to: "import { Octokit } from '@octokit/rest'".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("auth.user") {
        changes.push(FieldChange {
            field_path: "auth.user".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "auth.user()".into(),
                to: "auth.getUser()".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("getCurrentHub") {
        changes.push(FieldChange {
            field_path: "getCurrentHub".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "Sentry.getCurrentHub().getClient()".into(),
                to: "Sentry.getClient()".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains(".promise()") {
        changes.push(FieldChange {
            field_path: ".promise()".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: ").promise()".into(),
                to: ")".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    if target.upstream_change.contains("api.ashburn.twilio.com") {
        changes.push(FieldChange {
            field_path: "api.ashburn.twilio.com".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "api.ashburn.twilio.com".into(),
                to: "api.twilio.com".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: target.upstream_change.clone(),
        });
    }
    changes
}

/// Apply patches to all targets specified in a MaintenancePlan.
/// If `dry_run` is false, writes modified content back to disk.
pub fn patch_plan_targets(
    repo_root: &str,
    plan: &MaintenancePlan,
    dry_run: bool,
) -> Result<Vec<PatchResult>, String> {
    let mut results = Vec::new();

    // Map upstream changes by endpoint
    for target in &plan.patch_targets {
        let full_path = if Path::new(&target.file_path).is_absolute() {
            Path::new(&target.file_path).to_path_buf()
        } else {
            Path::new(repo_root).join(&target.file_path)
        };
        let source = match fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let changes = extract_field_changes_from_target(target);

        let res = apply_patch_to_source(&target.file_path, &source, &target.line_numbers, &changes);

        if !dry_run && res.success {
            fs::write(&full_path, &res.patched_content)
                .map_err(|e| format!("Failed to write {}: {}", target.file_path, e))?;
        }

        results.push(res);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_typescript_integer_literal_to_string() {
        let src = r#"const charge = await stripe.charges.create({
  amount: 2000,
  currency: 'usd',
});"#;
        let changes = vec![FieldChange {
            field_path: "amount".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "integer".into(),
                to: "string".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: "amount integer to string".into(),
        }];

        let res = apply_patch_to_source("billing.ts", src, &[2], &changes);
        assert!(res.success);
        assert!(res.patched_content.contains("amount: String(2000),"));
        assert!(res.unified_diff.contains("-  amount: 2000,"));
        assert!(res.unified_diff.contains("+  amount: String(2000),"));
    }

    #[test]
    fn patches_python_keyword_arg_to_str() {
        let src = "charge = stripe.charges.create(amount=1500, currency='usd')";
        let changes = vec![FieldChange {
            field_path: "amount".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "integer".into(),
                to: "string".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: "amount integer to string".into(),
        }];

        let res = apply_patch_to_source("service.py", src, &[1], &changes);
        assert!(res.success);
        assert!(res.patched_content.contains("amount=str(1500),"));
    }

    #[test]
    fn patches_typescript_shorthand_property() {
        let src = r#"async function createCharge(amount: number) {
  return await stripe.charges.create({
    amount,
    currency: 'usd',
  });
}"#;
        let changes = vec![FieldChange {
            field_path: "amount".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "integer".into(),
                to: "string".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: "amount integer to string".into(),
        }];

        let res = apply_patch_to_source("payment.ts", src, &[3], &changes);
        assert!(res.success);
        assert!(res.patched_content.contains("amount: String(amount),"));
    }

    #[test]
    fn patches_removed_parameter() {
        let src = "const c = stripe.charges.create({ amount: 100, description: 'test', currency: 'usd' });";
        let changes = vec![FieldChange {
            field_path: "description".into(),
            kind: ChangeKind::ParameterRemoved,
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: "description removed".into(),
        }];

        let res = apply_patch_to_source("billing.ts", src, &[1], &changes);
        assert!(res.success);
        assert!(!res.patched_content.contains("description: 'test'"));
        assert!(res.patched_content.contains("currency: 'usd'"));
    }

    #[test]
    fn unimpacted_lines_are_not_modified() {
        let src = "const unrelated = 42;\nconst charge = stripe.charges.create({ amount: 100 });";
        let changes = vec![FieldChange {
            field_path: "amount".into(),
            kind: ChangeKind::ParameterTypeChanged {
                from: "integer".into(),
                to: "string".into(),
            },
            severity: crate::engines::schema::BreakingSeverity::Breaking,
            description: "amount integer to string".into(),
        }];

        let res = apply_patch_to_source("billing.ts", src, &[2], &changes);
        assert_eq!(res.transforms_applied, 1);
        assert!(res.patched_content.starts_with("const unrelated = 42;\n"));
    }
}
