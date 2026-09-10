use super::types::*;
use serde_json::Value;
use std::collections::BTreeMap;

///
/// We intentionally avoid pulling in a full OpenAPI crate — the subset we need
/// (paths → operations → parameters + response schemas) is small and keeping it
/// in-house means zero transitive dependencies beyond `serde_json`.
pub fn parse_spec(spec_json: &str) -> Result<ParsedSpec, String> {
    let root: Value =
        serde_json::from_str(spec_json).map_err(|e| format!("JSON parse error: {e}"))?;

    let info = parse_info(&root);
    let endpoints = parse_paths(&root);
    let endpoint_count = endpoints.len();

    Ok(ParsedSpec {
        info: SpecInfo {
            title: info.0,
            version: info.1,
            endpoint_count,
        },
        endpoints,
    })
}

fn parse_info(root: &Value) -> (String, String) {
    let info = root.get("info").unwrap_or(&Value::Null);
    let title = info
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let version = info
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0")
        .to_string();
    (title, version)
}

const METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

fn parse_paths(root: &Value) -> BTreeMap<String, ParsedEndpoint> {
    let mut endpoints = BTreeMap::new();

    let paths = match root.get("paths").and_then(Value::as_object) {
        Some(p) => p,
        None => return endpoints,
    };

    for (path, path_item) in paths {
        let path_item = match path_item.as_object() {
            Some(o) => o,
            None => continue,
        };

        for method in METHODS {
            let op = match path_item.get(*method) {
                Some(v) => v,
                None => continue,
            };

            let key = format!("{method}:{path}");
            let parameters = parse_parameters(op);
            let (response_fields, response_statuses) = parse_responses(op);

            endpoints.insert(
                key.clone(),
                ParsedEndpoint {
                    path: path.clone(),
                    method: method.to_string(),
                    parameters,
                    response_fields,
                    response_statuses,
                },
            );
        }
    }
    endpoints
}

fn parse_parameters(op: &Value) -> BTreeMap<String, ParsedParam> {
    let mut params = BTreeMap::new();
    let arr = match op.get("parameters").and_then(Value::as_array) {
        Some(a) => a,
        None => return params,
    };

    for p in arr {
        let name = p
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let location = p
            .get("in")
            .and_then(Value::as_str)
            .unwrap_or("query")
            .to_string();
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        let param_type = extract_type(p.get("schema").unwrap_or(&Value::Null));

        params.insert(
            name.clone(),
            ParsedParam {
                name,
                location,
                param_type,
                required,
            },
        );
    }
    params
}

fn parse_responses(op: &Value) -> (BTreeMap<String, ParsedField>, Vec<String>) {
    let mut fields = BTreeMap::new();
    let mut statuses = Vec::new();

    let responses = match op.get("responses").and_then(Value::as_object) {
        Some(r) => r,
        None => return (fields, statuses),
    };

    for (status, resp) in responses {
        statuses.push(status.clone());

        // Walk content → application/json → schema → properties
        let schema = resp
            .get("content")
            .and_then(|c| c.get("application/json"))
            .and_then(|j| j.get("schema"));

        if let Some(schema) = schema {
            collect_fields(schema, "", &mut fields);
        }
    }

    statuses.sort();
    (fields, statuses)
}

fn collect_fields(schema: &Value, prefix: &str, out: &mut BTreeMap<String, ParsedField>) {
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (name, prop) in props {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}.{name}")
            };
            let field_type = extract_type(prop);
            out.insert(
                path.clone(),
                ParsedField {
                    name: path.clone(),
                    field_type,
                },
            );
            // Recurse into nested objects.
            if prop.get("type").and_then(Value::as_str) == Some("object") {
                collect_fields(prop, &path, out);
            }
            // Recurse into array items.
            if prop.get("type").and_then(Value::as_str) == Some("array") {
                if let Some(items) = prop.get("items") {
                    collect_fields(items, &format!("{path}[]"), out);
                }
            }
        }
    }
}

fn extract_type(schema: &Value) -> String {
    schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("any")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> &'static str {
        r#"{
  "openapi": "3.0.0",
  "info": { "title": "Payments API", "version": "2024-06-01" },
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

    #[test]
    fn schema_parse_spec_extracts_info() {
        let parsed = parse_spec(sample_spec()).unwrap();
        assert_eq!(parsed.info.title, "Payments API");
        assert_eq!(parsed.info.version, "2024-06-01");
        assert_eq!(parsed.info.endpoint_count, 1);
    }

    #[test]
    fn schema_parse_spec_extracts_parameters() {
        let parsed = parse_spec(sample_spec()).unwrap();
        let ep = parsed.endpoints.get("post:/v1/charges").unwrap();
        assert_eq!(ep.parameters.len(), 2);
        assert!(ep.parameters.get("amount").unwrap().required);
        assert_eq!(ep.parameters.get("amount").unwrap().param_type, "integer");
    }

    #[test]
    fn schema_parse_spec_extracts_response_fields() {
        let parsed = parse_spec(sample_spec()).unwrap();
        let ep = parsed.endpoints.get("post:/v1/charges").unwrap();
        assert_eq!(ep.response_fields.len(), 3);
        assert_eq!(ep.response_fields.get("id").unwrap().field_type, "string");
    }

    #[test]
    fn schema_parse_spec_invalid_json_returns_error() {
        assert!(parse_spec("not json").is_err());
    }

    #[test]
    fn schema_parse_spec_empty_paths() {
        let spec = r#"{"openapi":"3.0.0","info":{"title":"Empty","version":"1"},"paths":{}}"#;
        let parsed = parse_spec(spec).unwrap();
        assert_eq!(parsed.info.endpoint_count, 0);
    }
}
