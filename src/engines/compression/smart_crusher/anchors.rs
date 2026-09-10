use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

static UUID_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b")
        .expect("UUID_PATTERN")
});

static NUMERIC_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{4,}\b").expect("NUMERIC_ID_PATTERN"));

static HOSTNAME_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[a-zA-Z0-9][-a-zA-Z0-9]*\.[a-zA-Z0-9][-a-zA-Z0-9]*(?:\.[a-zA-Z]{2,})?\b")
        .expect("HOSTNAME_PATTERN")
});

static QUOTED_STRING_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"['"]([^'"]{1,50})['"]"#).expect("QUOTED_STRING_PATTERN"));

static EMAIL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").expect("EMAIL_PATTERN")
});

const HOSTNAME_FALSE_POSITIVES: &[&str] = &["e.g", "i.e", "etc."];

pub fn extract_query_anchors(text: &str) -> HashSet<String> {
    let mut anchors = HashSet::new();

    if text.is_empty() {
        return anchors;
    }

    for m in UUID_PATTERN.find_iter(text) {
        anchors.insert(m.as_str().to_lowercase());
    }

    for m in NUMERIC_ID_PATTERN.find_iter(text) {
        anchors.insert(m.as_str().to_string());
    }

    for m in HOSTNAME_PATTERN.find_iter(text) {
        let lc = m.as_str().to_lowercase();
        if !HOSTNAME_FALSE_POSITIVES.contains(&lc.as_str()) {
            anchors.insert(lc);
        }
    }

    for caps in QUOTED_STRING_PATTERN.captures_iter(text) {
        if let Some(inner) = caps.get(1) {
            if inner.as_str().trim().len() >= 2 {
                anchors.insert(inner.as_str().to_lowercase());
            }
        }
    }

    for m in EMAIL_PATTERN.find_iter(text) {
        anchors.insert(m.as_str().to_lowercase());
    }

    anchors
}

fn python_repr(value: &Value) -> String {
    let mut out = String::new();
    write_python_repr(&mut out, value);
    out
}

fn write_python_repr(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Number(n) => {
            out.push_str(&n.to_string());
        }
        Value::String(s) => {
            out.push('\'');
            out.push_str(s);
            out.push('\'');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_python_repr(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push('\'');
                out.push_str(k);
                out.push('\'');
                out.push_str(": ");
                write_python_repr(out, v);
            }
            out.push('}');
        }
    }
}

pub fn item_matches_anchors(item: &Value, anchors: &HashSet<String>) -> bool {
    if anchors.is_empty() {
        return false;
    }

    let item_str = python_repr(item).to_lowercase();
    anchors.iter().any(|a| item_str.contains(a))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_text_no_anchors() {
        assert!(extract_query_anchors("").is_empty());
    }

    #[test]
    fn extracts_uuid_lowercased() {
        let anchors = extract_query_anchors("see id 550E8400-E29B-41D4-A716-446655440000 plz");
        assert!(anchors.contains("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn extracts_numeric_id_unchanged() {
        let anchors = extract_query_anchors("user 12345 reported issue");
        assert!(anchors.contains("12345"));
    }

    #[test]
    fn three_digit_number_not_anchor() {
        let anchors = extract_query_anchors("user 123 reported issue");
        assert!(!anchors.iter().any(|a| a == "123"));
    }

    #[test]
    fn extracts_hostname() {
        let anchors = extract_query_anchors("connect to api.example.com asap");
        assert!(anchors.contains("api.example.com"));
    }

    #[test]
    fn hostname_false_positive_filtered() {
        let anchors = extract_query_anchors("test e.g.com endpoint");
        assert!(!anchors.contains("e.g"));
    }

    #[test]
    fn extracts_quoted_string_double() {
        let anchors = extract_query_anchors(r#"find the "user_name" field"#);
        assert!(anchors.contains("user_name"));
    }

    #[test]
    fn extracts_quoted_string_single() {
        let anchors = extract_query_anchors("find the 'user_name' field");
        assert!(anchors.contains("user_name"));
    }

    #[test]
    fn very_short_quoted_skipped() {
        let anchors = extract_query_anchors(r#"the "x" thing"#);
        assert!(!anchors.contains("x"));
    }

    #[test]
    fn extracts_email() {
        let anchors = extract_query_anchors("contact USER@example.COM please");
        assert!(anchors.contains("user@example.com"));
    }

    #[test]
    fn item_matches_anchors_empty_set() {
        let empty = HashSet::new();
        assert!(!item_matches_anchors(&json!({"a": 1}), &empty));
    }

    #[test]
    fn item_matches_anchor_in_value() {
        let anchors: HashSet<String> = ["alice".to_string()].into_iter().collect();
        assert!(item_matches_anchors(&json!({"name": "Alice"}), &anchors));
    }

    #[test]
    fn item_matches_anchor_in_key() {
        let anchors: HashSet<String> = ["status".to_string()].into_iter().collect();
        assert!(item_matches_anchors(&json!({"status": "ok"}), &anchors));
    }

    #[test]
    fn item_no_match_with_unrelated_anchor() {
        let anchors: HashSet<String> = ["xyz123".to_string()].into_iter().collect();
        assert!(!item_matches_anchors(&json!({"a": "b"}), &anchors));
    }

    #[test]
    fn hostname_blocklist_drops_e_g() {
        let anchors = extract_query_anchors("see e.g for example");
        assert!(!anchors.contains("e.g"));
        let anchors = extract_query_anchors("connect to api.example.com");
        assert!(anchors.contains("api.example.com"));
    }

    #[test]
    fn email_typo_pattern_still_matches_real_emails() {
        let anchors = extract_query_anchors("contact alice@example.com today");
        assert!(anchors.contains("alice@example.com"));
        let anchors = extract_query_anchors("ping bob@SUB.EXAMPLE.IO");
        assert!(anchors.contains("bob@sub.example.io"));
    }

    #[test]
    fn python_repr_matches_python_str_for_dict() {
        let v = json!({"name": "Alice", "ok": true, "count": 5, "val": null});
        let r = python_repr(&v);
        assert_eq!(r, "{'count': 5, 'name': 'Alice', 'ok': True, 'val': None}");
    }

    #[test]
    fn python_repr_list_uses_space_after_comma() {
        let v = json!([1, 2, "abc", true]);
        assert_eq!(python_repr(&v), "[1, 2, 'abc', True]");
    }

    #[test]
    fn python_repr_nested() {
        let v = json!({"a": [1, {"b": "c"}]});
        assert_eq!(python_repr(&v), "{'a': [1, {'b': 'c'}]}");
    }

    #[test]
    fn item_matches_anchor_with_python_none_form() {
        let anchors: HashSet<String> = ["none".to_string()].into_iter().collect();
        assert!(item_matches_anchors(&json!({"val": null}), &anchors));
    }

    #[test]
    fn item_matches_anchor_avoids_json_null_token() {
        let anchors: HashSet<String> = ["null".to_string()].into_iter().collect();
        assert!(!item_matches_anchors(&json!({"val": null}), &anchors));
    }

    #[test]
    fn python_repr_string_with_single_quote_drift() {
        let v = json!({"k": "it's fine"});
        assert_eq!(python_repr(&v), "{'k': 'it's fine'}");
    }
}
