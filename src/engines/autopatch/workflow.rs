use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub kind: StepKind,
    #[serde(default)]
    pub compartment: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub timeout_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepKind {
    SchemaRadar,
    ImpactAnalysis,
    PlanGeneration,
    Patch,
    Test,
    Verify,
    Command { cmd: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub trigger: TriggerConfig,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub kind: TriggerKind,
    #[serde(default)]
    pub api_name: String,
    #[serde(default)]
    pub schedule_cron: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggerKind {
    SchemaDrift,
    Vulnerability,
    Incident,
    FrameworkUpgrade,
    Scheduled,
    Manual,
}

pub fn parse_workflow(json: &str) -> Result<WorkflowDef, String> {
    serde_json::from_str(json).map_err(|e| format!("Workflow parse error: {e}"))
}

///   - All depends_on references exist.
///   - No circular dependencies (simple check).
///   - At least one step.
pub fn validate_workflow(wf: &WorkflowDef) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if wf.steps.is_empty() {
        errors.push("Workflow must have at least one step".into());
    }

    // Check unique names.
    let mut seen = std::collections::HashSet::new();
    for step in &wf.steps {
        if !seen.insert(&step.name) {
            errors.push(format!("Duplicate step name: '{}'", step.name));
        }
    }

    // Check depends_on references.
    let names: std::collections::HashSet<&str> = wf.steps.iter().map(|s| s.name.as_str()).collect();
    for step in &wf.steps {
        for dep in &step.depends_on {
            if !names.contains(dep.as_str()) {
                errors.push(format!(
                    "Step '{}' depends on '{}' which does not exist",
                    step.name, dep
                ));
            }
            if dep == &step.name {
                errors.push(format!("Step '{}' depends on itself", step.name));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Compute a topological execution order for the workflow steps.
/// Returns step names in the order they should execute.
pub fn execution_order(wf: &WorkflowDef) -> Result<Vec<String>, String> {
    let mut in_degree: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut dependents: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();

    for step in &wf.steps {
        in_degree.entry(&step.name).or_insert(0);
        for dep in &step.depends_on {
            dependents.entry(dep.as_str()).or_default().push(&step.name);
            *in_degree.entry(&step.name).or_insert(0) += 1;
        }
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();
    queue.sort(); // Deterministic ordering for equal-priority steps.

    let mut order = Vec::new();

    while let Some(current) = queue.first().copied() {
        queue.remove(0);
        order.push(current.to_string());

        if let Some(deps) = dependents.get(current) {
            for dep in deps {
                if let Some(deg) = in_degree.get_mut(dep) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push(dep);
                        queue.sort();
                    }
                }
            }
        }
    }

    if order.len() != wf.steps.len() {
        return Err("Circular dependency detected in workflow".into());
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_workflow_json() -> &'static str {
        r#"{
  "name": "stripe-api-drift",
  "description": "Auto-maintain Stripe API integration",
  "trigger": {
    "kind": "SchemaDrift",
    "api_name": "stripe"
  },
  "steps": [
    { "name": "radar", "kind": "SchemaRadar", "compartment": "research", "depends_on": [] },
    { "name": "impact", "kind": "ImpactAnalysis", "compartment": "research", "depends_on": ["radar"] },
    { "name": "plan", "kind": "PlanGeneration", "compartment": "builder", "depends_on": ["radar", "impact"] },
    { "name": "patch", "kind": "Patch", "compartment": "builder", "depends_on": ["plan"] },
    { "name": "test", "kind": "Test", "compartment": "tester", "depends_on": ["patch"] },
    { "name": "verify", "kind": "Verify", "compartment": "tester", "depends_on": ["test"] }
  ]
}"#
    }

    #[test]
    fn workflow_parses_valid_json() {
        let wf = parse_workflow(sample_workflow_json()).unwrap();
        assert_eq!(wf.name, "stripe-api-drift");
        assert_eq!(wf.steps.len(), 6);
        assert_eq!(wf.trigger.kind, TriggerKind::SchemaDrift);
    }

    #[test]
    fn workflow_parse_invalid_json_returns_error() {
        assert!(parse_workflow("not json").is_err());
    }

    #[test]
    fn workflow_validates_valid_workflow() {
        let wf = parse_workflow(sample_workflow_json()).unwrap();
        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn workflow_validates_catches_duplicate_names() {
        let json = r#"{
  "name": "test",
  "trigger": { "kind": "Manual" },
  "steps": [
    { "name": "a", "kind": "SchemaRadar", "depends_on": [] },
    { "name": "a", "kind": "Test", "depends_on": [] }
  ]
}"#;
        let wf = parse_workflow(json).unwrap();
        let errs = validate_workflow(&wf).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("Duplicate")));
    }

    #[test]
    fn workflow_validates_catches_missing_dependency() {
        let json = r#"{
  "name": "test",
  "trigger": { "kind": "Manual" },
  "steps": [
    { "name": "a", "kind": "SchemaRadar", "depends_on": ["nonexistent"] }
  ]
}"#;
        let wf = parse_workflow(json).unwrap();
        let errs = validate_workflow(&wf).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("does not exist")));
    }

    #[test]
    fn workflow_validates_catches_self_dependency() {
        let json = r#"{
  "name": "test",
  "trigger": { "kind": "Manual" },
  "steps": [
    { "name": "a", "kind": "SchemaRadar", "depends_on": ["a"] }
  ]
}"#;
        let wf = parse_workflow(json).unwrap();
        let errs = validate_workflow(&wf).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("depends on itself")));
    }

    #[test]
    fn workflow_validates_catches_empty_steps() {
        let json = r#"{
  "name": "test",
  "trigger": { "kind": "Manual" },
  "steps": []
}"#;
        let wf = parse_workflow(json).unwrap();
        let errs = validate_workflow(&wf).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("at least one step")));
    }

    #[test]
    fn workflow_execution_order_topological() {
        let wf = parse_workflow(sample_workflow_json()).unwrap();
        let order = execution_order(&wf).unwrap();
        assert_eq!(order.len(), 6);
        // radar must come before impact and plan.
        let radar_pos = order.iter().position(|s| s == "radar").unwrap();
        let impact_pos = order.iter().position(|s| s == "impact").unwrap();
        let plan_pos = order.iter().position(|s| s == "plan").unwrap();
        let patch_pos = order.iter().position(|s| s == "patch").unwrap();
        let test_pos = order.iter().position(|s| s == "test").unwrap();
        let verify_pos = order.iter().position(|s| s == "verify").unwrap();
        assert!(radar_pos < impact_pos);
        assert!(radar_pos < plan_pos);
        assert!(impact_pos < plan_pos);
        assert!(plan_pos < patch_pos);
        assert!(patch_pos < test_pos);
        assert!(test_pos < verify_pos);
    }

    #[test]
    fn workflow_execution_order_detects_cycle() {
        let json = r#"{
  "name": "cycle",
  "trigger": { "kind": "Manual" },
  "steps": [
    { "name": "a", "kind": "SchemaRadar", "depends_on": ["b"] },
    { "name": "b", "kind": "Test", "depends_on": ["a"] }
  ]
}"#;
        let wf = parse_workflow(json).unwrap();
        assert!(execution_order(&wf).is_err());
    }

    #[test]
    fn workflow_execution_order_parallel_roots() {
        let json = r#"{
  "name": "parallel",
  "trigger": { "kind": "Manual" },
  "steps": [
    { "name": "alpha", "kind": "SchemaRadar", "depends_on": [] },
    { "name": "beta", "kind": "ImpactAnalysis", "depends_on": [] },
    { "name": "merge", "kind": "Verify", "depends_on": ["alpha", "beta"] }
  ]
}"#;
        let wf = parse_workflow(json).unwrap();
        let order = execution_order(&wf).unwrap();
        assert_eq!(order.len(), 3);
        let merge_pos = order.iter().position(|s| s == "merge").unwrap();
        assert_eq!(merge_pos, 2, "merge should be last");
    }
}
