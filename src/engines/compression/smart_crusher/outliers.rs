use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::ERROR_KEYWORDS;

pub fn detect_structural_outliers(items: &[Value]) -> Vec<usize> {
    if items.len() < 5 {
        return Vec::new();
    }

    let mut field_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in items {
        if let Some(obj) = item.as_object() {
            for key in obj.keys() {
                *field_counts.entry(key.as_str()).or_insert(0) += 1;
            }
        }
    }

    let n = items.len();
    let common_fields: HashSet<String> = field_counts
        .iter()
        .filter(|&(_, &c)| c as f64 >= n as f64 * 0.8)
        .map(|(k, _)| (*k).to_string())
        .collect();
    let rare_fields: HashSet<&str> = field_counts
        .iter()
        .filter(|&(_, &c)| (c as f64) < n as f64 * 0.2)
        .map(|(k, _)| *k)
        .collect();

    let mut outlier_set: BTreeSet<usize> = BTreeSet::new();

    for (i, item) in items.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let has_rare = obj.keys().any(|k| rare_fields.contains(k.as_str()));
        if has_rare {
            outlier_set.insert(i);
        }
    }

    for idx in detect_rare_status_values(items, &common_fields) {
        outlier_set.insert(idx);
    }

    outlier_set.into_iter().collect()
}

pub fn detect_rare_status_values(items: &[Value], common_fields: &HashSet<String>) -> Vec<usize> {
    let mut outlier_indices: Vec<usize> = Vec::new();

    let mut sorted_fields: Vec<&String> = common_fields.iter().collect();
    sorted_fields.sort();

    for field_name in sorted_fields {
        let values: Vec<&Value> = items
            .iter()
            .filter_map(|item| item.as_object())
            .filter_map(|m| m.get(field_name))
            .collect();

        let stringify = |v: &Value| -> String {
            match v {
                Value::Null => unreachable!("null filtered above"),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => v.to_string(),
            }
        };

        let unique_values: BTreeSet<String> = values
            .iter()
            .filter(|v| !matches!(v, Value::Null))
            .map(|v| stringify(v))
            .collect();

        if !(2..=50).contains(&unique_values.len()) {
            continue;
        }

        let mut value_counts: BTreeMap<String, usize> = BTreeMap::new();
        for v in &values {
            let key = if matches!(v, Value::Null) {
                "__none__".to_string()
            } else {
                stringify(v)
            };
            *value_counts.entry(key).or_insert(0) += 1;
        }
        if value_counts.is_empty() {
            continue;
        }

        let total = values.len();

        let mut sorted_counts: Vec<(&String, &usize)> = value_counts.iter().collect();
        sorted_counts.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));

        let threshold = (total as f64 * 0.8).ceil() as usize;
        let mut cumulative: usize = 0;
        let mut top_k_values: HashSet<String> = HashSet::new();
        for (value, count) in &sorted_counts {
            cumulative += **count;
            top_k_values.insert((*value).clone());
            if cumulative >= threshold {
                break;
            }
        }

        if top_k_values.len() > 5 {
            continue;
        }

        for (i, item) in items.iter().enumerate() {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(field_value) = obj.get(field_name) else {
                continue;
            };
            let item_value = if matches!(field_value, Value::Null) {
                "__none__".to_string()
            } else {
                stringify(field_value)
            };
            if !top_k_values.contains(&item_value) {
                outlier_indices.push(i);
            }
        }
    }

    outlier_indices
}

pub fn detect_error_items_for_preservation(
    items: &[Value],
    item_strings: Option<&[String]>,
) -> Vec<usize> {
    let mut error_indices: Vec<usize> = Vec::new();

    for (i, item) in items.iter().enumerate() {
        if !item.is_object() {
            continue;
        }

        let serialized: String = match item_strings {
            Some(arr) if i < arr.len() => arr[i].to_lowercase(),
            _ => match serde_json::to_string(item) {
                Ok(s) => s.to_lowercase(),
                Err(_) => continue,
            },
        };

        if ERROR_KEYWORDS.iter().any(|kw| serialized.contains(kw)) {
            error_indices.push(i);
        }
    }

    error_indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn outliers_too_few_items_returns_empty() {
        let items: Vec<Value> = (0..3).map(|i| json!({"a": i})).collect();
        assert_eq!(detect_structural_outliers(&items), Vec::<usize>::new());
    }

    #[test]
    fn outliers_rare_field_flags_item() {
        let mut items: Vec<Value> = (0..9).map(|i| json!({"a": i})).collect();
        items.push(json!({"a": 9, "x": "rare"}));
        let outliers = detect_structural_outliers(&items);
        assert!(outliers.contains(&9));
    }

    #[test]
    fn outliers_no_dict_items_silently_skipped() {
        let items: Vec<Value> = vec![
            json!({"status": "ok"}),
            json!("string-not-dict"),
            json!({"status": "ok"}),
            json!({"status": "ok"}),
            json!({"status": "ok"}),
        ];
        let _ = detect_structural_outliers(&items);
    }

    #[test]
    fn rare_status_low_cardinality_dominant_value() {
        let mut items: Vec<Value> = (0..95).map(|_| json!({"status": "ok"})).collect();
        items.push(json!({"status": "error"}));
        items.push(json!({"status": "timeout"}));
        items.push(json!({"status": "error"}));
        items.push(json!({"status": "timeout"}));
        items.push(json!({"status": "fail"}));
        let common: HashSet<String> = ["status".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert_eq!(outliers.len(), 5);
    }

    #[test]
    fn rare_status_bug3_fix_high_cardinality_bimodal() {
        let mut items: Vec<Value> = Vec::new();
        for _ in 0..60 {
            items.push(json!({"code": "INFO"}));
        }
        for _ in 0..25 {
            items.push(json!({"code": "WARN"}));
        }
        for i in 0..15 {
            items.push(json!({"code": format!("ERR_{}", i)}));
        }
        let common: HashSet<String> = ["code".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert_eq!(outliers.len(), 15);
    }

    #[test]
    fn rare_status_uniform_distribution_no_outliers() {
        let items: Vec<Value> = (0..50)
            .map(|i| json!({"code": format!("CAT_{}", i)}))
            .collect();
        let common: HashSet<String> = ["code".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert!(
            outliers.is_empty(),
            "uniform distribution must not produce rare-status outliers"
        );
    }

    #[test]
    fn rare_status_cardinality_above_50_skipped() {
        let items: Vec<Value> = (0..60)
            .map(|i| json!({"code": format!("V_{}", i)}))
            .collect();
        let common: HashSet<String> = ["code".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert!(outliers.is_empty());
    }

    #[test]
    fn rare_status_cardinality_one_skipped() {
        let items: Vec<Value> = (0..100).map(|_| json!({"status": "ok"})).collect();
        let common: HashSet<String> = ["status".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert!(outliers.is_empty());
    }

    #[test]
    fn rare_status_nulls_filtered_from_cardinality() {
        let mut items: Vec<Value> = (0..95).map(|_| json!({"s": "ok"})).collect();
        for _ in 0..5 {
            items.push(json!({"s": null}));
        }
        let common: HashSet<String> = ["s".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert!(
            outliers.is_empty(),
            "cardinality 1 (after null filter) must skip the field"
        );
    }

    #[test]
    fn rare_status_nulls_count_in_value_counts_when_cardinality_passes() {
        let mut items: Vec<Value> = (0..90).map(|_| json!({"s": "ok"})).collect();
        for _ in 0..5 {
            items.push(json!({"s": "warn"}));
        }
        for _ in 0..5 {
            items.push(json!({"s": null}));
        }
        let common: HashSet<String> = ["s".to_string()].into_iter().collect();
        let outliers = detect_rare_status_values(&items, &common);
        assert_eq!(outliers.len(), 10, "5 warn + 5 null = 10 outliers");
    }

    #[test]
    fn error_keywords_preservation_basic() {
        let items: Vec<Value> = vec![
            json!({"status": "ok"}),
            json!({"status": "error", "msg": "boom"}),
            json!({"status": "ok"}),
            json!({"msg": "request failed"}),
            json!({"status": "ok"}),
        ];
        let errs = detect_error_items_for_preservation(&items, None);
        assert_eq!(errs, vec![1, 3]);
    }

    #[test]
    fn error_keywords_case_insensitive() {
        let items: Vec<Value> = vec![
            json!({"msg": "FATAL: out of memory"}),
            json!({"msg": "panic at line 42"}),
        ];
        let errs = detect_error_items_for_preservation(&items, None);
        assert_eq!(errs, vec![0, 1]);
    }

    #[test]
    fn error_keywords_no_match() {
        let items: Vec<Value> = vec![json!({"name": "alice"}), json!({"count": 5})];
        let errs = detect_error_items_for_preservation(&items, None);
        assert!(errs.is_empty());
    }

    #[test]
    fn error_keywords_uses_cached_strings_when_provided() {
        let items: Vec<Value> = vec![json!({"a": 1}), json!({"b": 2})];
        let cached = vec!["error".to_string(), "ok".to_string()];
        let errs = detect_error_items_for_preservation(&items, Some(&cached));
        assert_eq!(errs, vec![0]);
    }

    #[test]
    fn error_keywords_falls_back_when_cache_too_short() {
        let items: Vec<Value> = vec![json!({"a": 1}), json!({"msg": "error"})];
        let cached = vec!["ok".to_string()];
        let errs = detect_error_items_for_preservation(&items, Some(&cached));
        assert_eq!(errs, vec![1]);
    }

    #[test]
    fn error_keywords_skips_non_dict_items() {
        let items: Vec<Value> = vec![
            json!({"msg": "error"}),
            json!("error string"),
            json!({"msg": "error"}),
        ];
        let errs = detect_error_items_for_preservation(&items, None);
        assert_eq!(errs, vec![0, 2]);
    }
}
