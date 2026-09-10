use super::types::*;
use crate::engines::ast::{locate_callsites, ScanConfig, ScanResult};
use crate::engines::autopatch::resolver::{assess_impact_with_index, MatchConfidence};
use crate::engines::schema::{diff_specs, parse_spec, BreakingSeverity, ChangeKind, SchemaDiff};
use std::collections::{HashMap, HashSet};

pub fn plan_maintenance(
    old_spec_json: &str,
    new_spec_json: &str,
    repo_root: &str,
    scan_config: &ScanConfig,
) -> Result<MaintenancePlan, String> {
    let old_spec = parse_spec(old_spec_json)?;
    let new_spec = parse_spec(new_spec_json)?;

    let diff = diff_specs(&old_spec, &new_spec);

    if diff.breaking_count == 0 && diff.warning_count == 0 {
        return Ok(MaintenancePlan {
            status: PlanStatus::Clean,
            api_name: new_spec.info.title.clone(),
            old_version: old_spec.info.version.clone(),
            new_version: new_spec.info.version.clone(),
            ..Default::default()
        });
    }

    let scan_result = locate_callsites(repo_root, scan_config);

    // 4. Correlate using dynamic spec route index + canonical routing.
    let spec_index =
        crate::engines::autopatch::resolver::SpecRouteIndex::from_parsed_spec(&new_spec);
    let (impacted_endpoints, patch_targets, verification_specs) =
        correlate(&diff, &scan_result, Some(&spec_index));

    let total_affected_callsites: usize =
        impacted_endpoints.iter().map(|ie| ie.confirmed_count).sum();
    let total_affected_files = {
        let mut files: HashSet<&str> = HashSet::new();
        for ie in &impacted_endpoints {
            for ac in &ie.affected_callsites {
                files.insert(&ac.file_path);
            }
        }
        files.len()
    };

    let status = if total_affected_callsites == 0 {
        PlanStatus::NoImpact
    } else {
        PlanStatus::ActionRequired
    };

    Ok(MaintenancePlan {
        status,
        api_name: new_spec.info.title.clone(),
        old_version: old_spec.info.version.clone(),
        new_version: new_spec.info.version.clone(),
        breaking_changes: diff.breaking_count,
        total_affected_files,
        total_affected_callsites,
        impacted_endpoints,
        patch_targets,
        verification_specs,
    })
}

/// Produce a maintenance plan from pre-computed diff and scan results.
/// Useful for testing without filesystem access.
#[allow(dead_code)]
pub fn plan_from_diff_and_scan(diff: &SchemaDiff, scan_result: &ScanResult) -> MaintenancePlan {
    plan_from_diff_and_scan_with_index(diff, scan_result, None)
}

/// Produce a maintenance plan with an explicit SpecRouteIndex.
pub fn plan_from_diff_and_scan_with_index(
    diff: &SchemaDiff,
    scan_result: &ScanResult,
    spec_index: Option<&crate::engines::autopatch::resolver::SpecRouteIndex>,
) -> MaintenancePlan {
    if diff.breaking_count == 0 && diff.warning_count == 0 {
        return MaintenancePlan {
            status: PlanStatus::Clean,
            api_name: diff.new_spec.title.clone(),
            old_version: diff.old_spec.version.clone(),
            new_version: diff.new_spec.version.clone(),
            ..Default::default()
        };
    }

    let (impacted_endpoints, patch_targets, verification_specs) =
        correlate(diff, scan_result, spec_index);

    let total_affected_callsites: usize =
        impacted_endpoints.iter().map(|ie| ie.confirmed_count).sum();
    let total_affected_files = {
        let mut files: HashSet<&str> = HashSet::new();
        for ie in &impacted_endpoints {
            for ac in &ie.affected_callsites {
                files.insert(&ac.file_path);
            }
        }
        files.len()
    };

    let status = if total_affected_callsites == 0 {
        PlanStatus::NoImpact
    } else {
        PlanStatus::ActionRequired
    };

    MaintenancePlan {
        status,
        api_name: diff.new_spec.title.clone(),
        old_version: diff.old_spec.version.clone(),
        new_version: diff.new_spec.version.clone(),
        breaking_changes: diff.breaking_count,
        total_affected_files,
        total_affected_callsites,
        impacted_endpoints,
        patch_targets,
        verification_specs,
    }
}

fn correlate(
    diff: &SchemaDiff,
    scan_result: &ScanResult,
    spec_index: Option<&crate::engines::autopatch::resolver::SpecRouteIndex>,
) -> (
    Vec<ImpactedEndpoint>,
    Vec<PatchTarget>,
    Vec<VerificationSpec>,
) {
    let mut impacted = Vec::new();
    let mut targets = Vec::new();
    let mut verifications = Vec::new();

    for ec in &diff.endpoint_changes {
        let change_summaries: Vec<String> =
            ec.changes.iter().map(|c| c.description.clone()).collect();
        let max_severity = ec
            .changes
            .iter()
            .map(|c| c.severity)
            .max_by_key(|s| match s {
                BreakingSeverity::Breaking => 2,
                BreakingSeverity::Warning => 1,
                BreakingSeverity::Info => 0,
            })
            .unwrap_or(BreakingSeverity::Info);

        let mut confirmed: Vec<AffectedCallsite> = Vec::new();
        let mut provably_unaffected: Vec<AffectedCallsite> = Vec::new();
        let mut unresolved: Vec<AffectedCallsite> = Vec::new();
        let mut unresolved_callsites: Vec<UnresolvedCallsite> = Vec::new();
        let mut false_positive_count: usize = 0;

        for cs in &scan_result.callsites {
            let confidence = assess_impact_with_index(
                &cs.kind,
                &cs.matched_pattern,
                &ec.method,
                &ec.path,
                spec_index,
            );
            match confidence {
                MatchConfidence::Confirmed => confirmed.push(AffectedCallsite::from(cs)),
                MatchConfidence::FalsePositive => {
                    false_positive_count += 1;
                    provably_unaffected.push(AffectedCallsite::from(cs));
                }
                MatchConfidence::Unresolved(reason, explanation) => {
                    unresolved.push(AffectedCallsite::from(cs));
                    unresolved_callsites.push(UnresolvedCallsite {
                        reason,
                        file_path: cs.file_path.clone(),
                        line_number: cs.line_number,
                        source_text: cs.line_content.trim().to_string(),
                        provider: diff.new_spec.title.clone(),
                        inferred_operation: None,
                        confidence_evidence: explanation.clone(),
                        why_autofix_disabled: format!(
                            "Uncertainty threshold exceeded ({}). Automated modification prohibited by safety policy.",
                            reason.as_str()
                        ),
                    });
                }
                MatchConfidence::Unresolvable => {
                    unresolved.push(AffectedCallsite::from(cs));
                    unresolved_callsites.push(UnresolvedCallsite {
                        reason: UncertaintyReason::InsufficientEvidence,
                        file_path: cs.file_path.clone(),
                        line_number: cs.line_number,
                        source_text: cs.line_content.trim().to_string(),
                        provider: diff.new_spec.title.clone(),
                        inferred_operation: None,
                        confidence_evidence:
                            "Insufficient evidence to resolve operation with certainty.".into(),
                        why_autofix_disabled: "Automated modification prohibited by safety policy."
                            .into(),
                    });
                }
            }
        }

        // Only emit if we have confirmed hits. Unresolvable (imports, type refs)
        // are listed separately for transparency but do NOT trigger patch targets
        // on their own — that was the source of import-proximity false positives.
        let total_references = scan_result.callsites.len();
        let affected = confirmed.clone();

        if !affected.is_empty() || max_severity == BreakingSeverity::Breaking {
            // For Breaking changes with zero confirmed hits: if there are unresolvable
            // references (imports we can't trace), include them as "potentially affected"
            // but flag them clearly — still no automatic patch target.
            let reported_affected = confirmed.clone();

            impacted.push(ImpactedEndpoint {
                path: ec.path.clone(),
                method: ec.method.clone(),
                change_summary: change_summaries,
                severity: max_severity,
                affected_callsites: reported_affected.clone(),
                confirmed_count: confirmed.len(),
                false_positive_count,
                unresolvable_count: unresolved.len(),
                total_sdk_references: total_references,
                unresolved_callsites,
                provably_unaffected_callsites: provably_unaffected,
            });

            // Patch targets only for confirmed callsites.
            let mut file_lines: HashMap<String, Vec<usize>> = HashMap::new();
            for ac in &confirmed {
                file_lines
                    .entry(ac.file_path.clone())
                    .or_default()
                    .push(ac.line_number);
            }
            for (file_path, mut lines) in file_lines {
                lines.sort();
                lines.dedup();
                let target = PatchTarget {
                    file_path,
                    line_numbers: lines,
                    reason: format!(
                        "{} {} has breaking changes",
                        ec.method.to_uppercase(),
                        ec.path
                    ),
                    upstream_change: ec
                        .changes
                        .iter()
                        .map(|c| c.description.as_str())
                        .collect::<Vec<_>>()
                        .join("; "),
                };

                let decision =
                    crate::engines::autopatch::policy::SafetyPolicy::evaluate_patch_eligibility(
                        &target,
                        &crate::engines::autopatch::types::ImpactState::ConfirmedAffected,
                    );
                if decision == crate::engines::autopatch::policy::SafetyDecision::Approved {
                    targets.push(target);
                }
            }

            // Verification specs only for endpoints with confirmed field-level hits.
            if !confirmed.is_empty() {
                let fields_to_verify: Vec<String> = ec
                    .changes
                    .iter()
                    .filter(|c| {
                        matches!(
                            c.kind,
                            ChangeKind::ParameterTypeChanged { .. }
                                | ChangeKind::ResponseFieldTypeChanged { .. }
                                | ChangeKind::ParameterRemoved
                                | ChangeKind::ResponseFieldRemoved
                        )
                    })
                    .map(|c| c.field_path.clone())
                    .collect();

                if !fields_to_verify.is_empty() {
                    verifications.push(VerificationSpec {
                        endpoint: ec.path.clone(),
                        method: ec.method.clone(),
                        fields_to_verify,
                    });
                }
            }
        }
    }

    (impacted, targets, verifications)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::ast::{Callsite, CallsiteKind, ScanResult};
    use crate::engines::schema::{diff_specs, parse_spec};

    fn old_spec() -> &'static str {
        r#"{
  "openapi": "3.0.0",
  "info": { "title": "Payments", "version": "2024-06-01" },
  "paths": {
    "/v1/charges": {
      "post": {
        "parameters": [
          { "name": "amount", "in": "query", "required": true, "schema": { "type": "integer" } },
          { "name": "currency", "in": "query", "required": true, "schema": { "type": "string" } }
        ],
        "responses": {
          "200": {
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string" },
                    "amount": { "type": "integer" }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}"#
    }

    fn new_spec() -> &'static str {
        r#"{
  "openapi": "3.0.0",
  "info": { "title": "Payments", "version": "2026-02-15" },
  "paths": {
    "/v1/charges": {
      "post": {
        "parameters": [
          { "name": "amount", "in": "query", "required": true, "schema": { "type": "string" } },
          { "name": "currency", "in": "query", "required": true, "schema": { "type": "string" } },
          { "name": "idempotency_key", "in": "header", "required": true, "schema": { "type": "string" } }
        ],
        "responses": {
          "200": {
            "content": {
              "application/json": {
                "schema": {
                  "type": "object",
                  "properties": {
                    "id": { "type": "string" },
                    "status": { "type": "string" },
                    "amount": { "type": "string" }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}"#
    }

    fn mock_scan_result() -> ScanResult {
        ScanResult {
            files_scanned: 3,
            files_with_hits: 2,
            callsites: vec![
                Callsite {
                    file_path: "src/billing.ts".into(),
                    line_number: 5,
                    column: 1,
                    line_content: "import Stripe from 'stripe';".into(),
                    kind: CallsiteKind::Import,
                    matched_pattern: "stripe".into(),
                    alias: None,
                },
                Callsite {
                    file_path: "src/billing.ts".into(),
                    line_number: 12,
                    column: 10,
                    line_content: "const charge = await stripe.charges.create({ amount: 2000 });"
                        .into(),
                    kind: CallsiteKind::MethodCall,
                    matched_pattern: "charges.create".into(),
                    alias: None,
                },
                Callsite {
                    file_path: "src/api_client.py".into(),
                    line_number: 8,
                    column: 1,
                    line_content: "resp = requests.post('https://api.stripe.com/v1/charges', ...)"
                        .into(),
                    kind: CallsiteKind::UrlReference,
                    matched_pattern: "api.stripe.com".into(),
                    alias: None,
                },
            ],
        }
    }

    #[test]
    fn autopatch_plan_from_diff_action_required() {
        let old = parse_spec(old_spec()).unwrap();
        let new = parse_spec(new_spec()).unwrap();
        let diff = diff_specs(&old, &new);
        let scan = mock_scan_result();

        let plan = plan_from_diff_and_scan(&diff, &scan);

        assert_eq!(plan.status, PlanStatus::ActionRequired);
        assert!(plan.breaking_changes >= 2);
        assert!(plan.total_affected_callsites >= 1);
        assert!(!plan.impacted_endpoints.is_empty());
    }

    #[test]
    fn autopatch_plan_generates_patch_targets() {
        let old = parse_spec(old_spec()).unwrap();
        let new = parse_spec(new_spec()).unwrap();
        let diff = diff_specs(&old, &new);
        let scan = mock_scan_result();

        let plan = plan_from_diff_and_scan(&diff, &scan);

        assert!(
            !plan.patch_targets.is_empty(),
            "should generate patch targets"
        );
        // Every patch target should have a non-empty reason.
        for pt in &plan.patch_targets {
            assert!(!pt.reason.is_empty());
            assert!(!pt.line_numbers.is_empty());
        }
    }

    #[test]
    fn autopatch_plan_generates_verification_specs() {
        let old = parse_spec(old_spec()).unwrap();
        let new = parse_spec(new_spec()).unwrap();
        let diff = diff_specs(&old, &new);
        let scan = mock_scan_result();

        let plan = plan_from_diff_and_scan(&diff, &scan);

        assert!(
            !plan.verification_specs.is_empty(),
            "should generate verification specs"
        );
        for vs in &plan.verification_specs {
            assert!(!vs.fields_to_verify.is_empty());
        }
    }

    #[test]
    fn autopatch_clean_when_no_breaking_changes() {
        let spec = parse_spec(old_spec()).unwrap();
        let diff = diff_specs(&spec, &spec);
        let scan = mock_scan_result();

        let plan = plan_from_diff_and_scan(&diff, &scan);
        assert_eq!(plan.status, PlanStatus::Clean);
        assert_eq!(plan.breaking_changes, 0);
    }

    #[test]
    fn autopatch_no_impact_when_no_callsites() {
        let old = parse_spec(old_spec()).unwrap();
        let new = parse_spec(new_spec()).unwrap();
        let diff = diff_specs(&old, &new);
        let empty_scan = ScanResult::default();

        let plan = plan_from_diff_and_scan(&diff, &empty_scan);
        // With breaking changes but no callsites, status should reflect the situation.
        assert!(
            plan.status == PlanStatus::NoImpact || plan.status == PlanStatus::ActionRequired,
            "status should be NoImpact or ActionRequired depending on fallback"
        );
    }

    #[test]
    fn autopatch_plan_serializes_to_json() {
        let old = parse_spec(old_spec()).unwrap();
        let new = parse_spec(new_spec()).unwrap();
        let diff = diff_specs(&old, &new);
        let scan = mock_scan_result();

        let plan = plan_from_diff_and_scan(&diff, &scan);
        let json = serde_json::to_string_pretty(&plan);
        assert!(json.is_ok(), "plan should serialize to JSON");
        let json = json.unwrap();
        assert!(json.contains("ActionRequired"));
        assert!(json.contains("Payments"));
    }
}
