use super::types::*;

/// Compute a structured diff between two parsed OpenAPI specs.
///
/// Returns a `SchemaDiff` containing every endpoint-level change,
/// classified by severity (Info / Warning / Breaking).
pub fn diff_specs(old: &ParsedSpec, new: &ParsedSpec) -> SchemaDiff {
    let mut endpoint_changes = Vec::new();
    let mut breaking = 0usize;
    let mut warning = 0usize;
    let mut info = 0usize;

    // 1. Detect removed and modified endpoints.
    for (key, old_ep) in &old.endpoints {
        match new.endpoints.get(key) {
            None => {
                // Entire endpoint removed → breaking.
                let fc = FieldChange {
                    field_path: String::new(),
                    kind: ChangeKind::EndpointRemoved,
                    severity: BreakingSeverity::Breaking,
                    description: format!(
                        "{} {} was removed",
                        old_ep.method.to_uppercase(),
                        old_ep.path
                    ),
                };
                breaking += 1;
                endpoint_changes.push(EndpointChange {
                    path: old_ep.path.clone(),
                    method: old_ep.method.clone(),
                    changes: vec![fc],
                });
            }
            Some(new_ep) => {
                let changes = diff_endpoint(old_ep, new_ep);
                for c in &changes {
                    match c.severity {
                        BreakingSeverity::Breaking => breaking += 1,
                        BreakingSeverity::Warning => warning += 1,
                        BreakingSeverity::Info => info += 1,
                    }
                }
                if !changes.is_empty() {
                    endpoint_changes.push(EndpointChange {
                        path: old_ep.path.clone(),
                        method: old_ep.method.clone(),
                        changes,
                    });
                }
            }
        }
    }

    // 2. Detect added endpoints.
    for (key, new_ep) in &new.endpoints {
        if !old.endpoints.contains_key(key) {
            let fc = FieldChange {
                field_path: String::new(),
                kind: ChangeKind::EndpointAdded,
                severity: BreakingSeverity::Info,
                description: format!("{} {} was added", new_ep.method.to_uppercase(), new_ep.path),
            };
            info += 1;
            endpoint_changes.push(EndpointChange {
                path: new_ep.path.clone(),
                method: new_ep.method.clone(),
                changes: vec![fc],
            });
        }
    }

    // Sort by path for deterministic output.
    endpoint_changes.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));

    SchemaDiff {
        old_spec: old.info.clone(),
        new_spec: new.info.clone(),
        endpoint_changes,
        breaking_count: breaking,
        warning_count: warning,
        info_count: info,
    }
}

fn diff_endpoint(old: &ParsedEndpoint, new: &ParsedEndpoint) -> Vec<FieldChange> {
    let mut changes = Vec::new();

    // --- Parameter diffs ---

    // Removed params.
    for name in old.parameters.keys() {
        if !new.parameters.contains_key(name) {
            changes.push(FieldChange {
                field_path: format!("parameters.{name}"),
                kind: ChangeKind::ParameterRemoved,
                severity: BreakingSeverity::Breaking,
                description: format!("Parameter '{name}' was removed"),
            });
        }
    }

    // Added params.
    for (name, new_p) in &new.parameters {
        if !old.parameters.contains_key(name) {
            let severity = if new_p.required {
                BreakingSeverity::Breaking
            } else {
                BreakingSeverity::Info
            };
            changes.push(FieldChange {
                field_path: format!("parameters.{name}"),
                kind: ChangeKind::ParameterAdded {
                    required: new_p.required,
                },
                severity,
                description: format!("Parameter '{name}' was added (required={})", new_p.required),
            });
        }
    }

    // Modified params.
    for (name, old_p) in &old.parameters {
        if let Some(new_p) = new.parameters.get(name) {
            if old_p.param_type != new_p.param_type {
                changes.push(FieldChange {
                    field_path: format!("parameters.{name}"),
                    kind: ChangeKind::ParameterTypeChanged {
                        from: old_p.param_type.clone(),
                        to: new_p.param_type.clone(),
                    },
                    severity: BreakingSeverity::Breaking,
                    description: format!(
                        "Parameter '{name}' type changed from '{}' to '{}'",
                        old_p.param_type, new_p.param_type
                    ),
                });
            }
            if old_p.required != new_p.required {
                let severity = if new_p.required {
                    BreakingSeverity::Breaking
                } else {
                    BreakingSeverity::Info
                };
                changes.push(FieldChange {
                    field_path: format!("parameters.{name}"),
                    kind: ChangeKind::ParameterRequiredChanged {
                        was: old_p.required,
                        now: new_p.required,
                    },
                    severity,
                    description: format!(
                        "Parameter '{name}' required changed from {} to {}",
                        old_p.required, new_p.required
                    ),
                });
            }
        }
    }

    // --- Response field diffs ---

    for name in old.response_fields.keys() {
        if !new.response_fields.contains_key(name) {
            changes.push(FieldChange {
                field_path: format!("response.{name}"),
                kind: ChangeKind::ResponseFieldRemoved,
                severity: BreakingSeverity::Warning,
                description: format!("Response field '{name}' was removed"),
            });
        }
    }

    for name in new.response_fields.keys() {
        if !old.response_fields.contains_key(name) {
            changes.push(FieldChange {
                field_path: format!("response.{name}"),
                kind: ChangeKind::ResponseFieldAdded,
                severity: BreakingSeverity::Info,
                description: format!("Response field '{name}' was added"),
            });
        }
    }

    for (name, old_f) in &old.response_fields {
        if let Some(new_f) = new.response_fields.get(name) {
            if old_f.field_type != new_f.field_type {
                changes.push(FieldChange {
                    field_path: format!("response.{name}"),
                    kind: ChangeKind::ResponseFieldTypeChanged {
                        from: old_f.field_type.clone(),
                        to: new_f.field_type.clone(),
                    },
                    severity: BreakingSeverity::Breaking,
                    description: format!(
                        "Response field '{name}' type changed from '{}' to '{}'",
                        old_f.field_type, new_f.field_type
                    ),
                });
            }
        }
    }

    // --- Response status diffs ---

    for s in &old.response_statuses {
        if !new.response_statuses.contains(s) {
            changes.push(FieldChange {
                field_path: format!("responses.{s}"),
                kind: ChangeKind::ResponseStatusRemoved { status: s.clone() },
                severity: BreakingSeverity::Warning,
                description: format!("Response status '{s}' was removed"),
            });
        }
    }

    for s in &new.response_statuses {
        if !old.response_statuses.contains(s) {
            changes.push(FieldChange {
                field_path: format!("responses.{s}"),
                kind: ChangeKind::ResponseStatusAdded { status: s.clone() },
                severity: BreakingSeverity::Info,
                description: format!("Response status '{s}' was added"),
            });
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::schema::parse_spec;

    fn old_spec_json() -> &'static str {
        r#"{
  "openapi": "3.0.0",
  "info": { "title": "Payments", "version": "2024-06-01" },
  "paths": {
    "/v1/charges": {
      "post": {
        "parameters": [
          { "name": "amount", "in": "query", "required": true, "schema": { "type": "integer" } },
          { "name": "currency", "in": "query", "required": true, "schema": { "type": "string" } },
          { "name": "description", "in": "query", "required": false, "schema": { "type": "string" } }
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
                    "amount": { "type": "integer" },
                    "fee": { "type": "integer" }
                  }
                }
              }
            }
          }
        }
      }
    },
    "/v1/refunds": {
      "post": {
        "parameters": [],
        "responses": { "200": {} }
      }
    }
  }
}"#
    }

    fn new_spec_json() -> &'static str {
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
                    "amount": { "type": "string" },
                    "metadata": { "type": "object" }
                  }
                }
              }
            }
          }
        }
      }
    },
    "/v1/payment_intents": {
      "post": {
        "parameters": [],
        "responses": { "200": {} }
      }
    }
  }
}"#
    }

    #[test]
    fn schema_diff_detects_removed_endpoint() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let refund_change = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/refunds")
            .expect("should detect /v1/refunds removal");
        assert!(refund_change
            .changes
            .iter()
            .any(|c| matches!(c.kind, ChangeKind::EndpointRemoved)));
    }

    #[test]
    fn schema_diff_detects_added_endpoint() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let pi_change = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/payment_intents")
            .expect("should detect /v1/payment_intents addition");
        assert!(pi_change
            .changes
            .iter()
            .any(|c| matches!(c.kind, ChangeKind::EndpointAdded)));
    }

    #[test]
    fn schema_diff_detects_param_type_change() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let charges = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/charges")
            .expect("should have changes on /v1/charges");

        let type_change = charges
            .changes
            .iter()
            .find(|c| {
                matches!(
                    &c.kind,
                    ChangeKind::ParameterTypeChanged { from, to }
                    if from == "integer" && to == "string"
                )
            })
            .expect("should detect amount type change integer→string");
        assert_eq!(type_change.severity, BreakingSeverity::Breaking);
    }

    #[test]
    fn schema_diff_detects_param_removed() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let charges = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/charges")
            .unwrap();

        assert!(charges
            .changes
            .iter()
            .any(|c| c.field_path == "parameters.description"
                && matches!(c.kind, ChangeKind::ParameterRemoved)));
    }

    #[test]
    fn schema_diff_detects_required_param_added() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let charges = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/charges")
            .unwrap();

        let added = charges
            .changes
            .iter()
            .find(|c| c.field_path == "parameters.idempotency_key")
            .expect("should detect idempotency_key addition");
        assert_eq!(added.severity, BreakingSeverity::Breaking);
    }

    #[test]
    fn schema_diff_detects_response_field_removed() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let charges = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/charges")
            .unwrap();

        assert!(charges
            .changes
            .iter()
            .any(|c| c.field_path == "response.fee"
                && matches!(c.kind, ChangeKind::ResponseFieldRemoved)));
    }

    #[test]
    fn schema_diff_detects_response_field_type_change() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        let charges = diff
            .endpoint_changes
            .iter()
            .find(|ec| ec.path == "/v1/charges")
            .unwrap();

        assert!(charges
            .changes
            .iter()
            .any(|c| c.field_path == "response.amount"
                && matches!(
                    &c.kind,
                    ChangeKind::ResponseFieldTypeChanged { from, to }
                    if from == "integer" && to == "string"
                )));
    }

    #[test]
    fn schema_diff_counts_severities() {
        let old = parse_spec(old_spec_json()).unwrap();
        let new = parse_spec(new_spec_json()).unwrap();
        let diff = diff_specs(&old, &new);

        assert!(
            diff.breaking_count >= 3,
            "expected >=3 breaking, got {}",
            diff.breaking_count
        );
        assert!(
            diff.info_count >= 1,
            "expected >=1 info, got {}",
            diff.info_count
        );
    }

    #[test]
    fn schema_diff_identical_specs_produces_no_changes() {
        let spec = parse_spec(old_spec_json()).unwrap();
        let diff = diff_specs(&spec, &spec);
        assert_eq!(diff.endpoint_changes.len(), 0);
        assert_eq!(diff.breaking_count, 0);
    }
}
