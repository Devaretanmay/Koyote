use serde_json::Value;
use std::collections::{BTreeSet, HashSet};

use super::outliers::{detect_error_items_for_preservation, detect_structural_outliers};
use super::types::{ArrayAnalysis, FieldStats};
use super::SmartCrusherConfig;
use crate::engines::compression::anchor_selector::compute_item_hash;

pub fn deduplicate_indices_by_content(
    keep_indices: &BTreeSet<usize>,
    items: &[Value],
) -> BTreeSet<usize> {
    if keep_indices.is_empty() {
        return BTreeSet::new();
    }

    let mut seen: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for &idx in keep_indices {
        if idx >= items.len() {
            continue;
        }
        let h = item_content_hash(&items[idx], idx);
        seen.entry(h).or_insert(idx);
    }
    seen.values().copied().collect()
}

pub fn fill_remaining_slots(
    keep_indices: &BTreeSet<usize>,
    items: &[Value],
    n: usize,
    effective_max: usize,
) -> BTreeSet<usize> {
    let remaining = effective_max.saturating_sub(keep_indices.len());
    if remaining == 0 {
        return keep_indices.clone();
    }

    let mut seen: HashSet<String> = HashSet::new();
    for &idx in keep_indices {
        if idx < n {
            seen.insert(item_content_hash(&items[idx], idx));
        }
    }

    let candidates: Vec<usize> = (0..n).filter(|i| !keep_indices.contains(i)).collect();
    if candidates.is_empty() {
        return keep_indices.clone();
    }

    let mut result = keep_indices.clone();
    let step = (candidates.len() / (remaining + 1)).max(1);
    let mut added = 0;

    'outer: for start_offset in 0..step {
        if added >= remaining {
            break;
        }
        let mut i = start_offset;
        while i < candidates.len() {
            if added >= remaining {
                break 'outer;
            }
            let idx = candidates[i];
            let h = item_content_hash(&items[idx], idx);
            if !seen.contains(&h) {
                result.insert(idx);
                seen.insert(h);
                added += 1;
            }
            i += step;
        }
    }

    result
}

pub fn prioritize_indices(
    config: &SmartCrusherConfig,
    keep_indices: &BTreeSet<usize>,
    items: &[Value],
    n: usize,
    analysis: Option<&ArrayAnalysis>,
    effective_max: usize,
) -> BTreeSet<usize> {
    let mut current = if config.dedup_identical_items {
        deduplicate_indices_by_content(keep_indices, items)
    } else {
        keep_indices.clone()
    };

    if current.len() < effective_max && current.len() < n {
        current = fill_remaining_slots(&current, items, n, effective_max);
    }

    if current.len() <= effective_max {
        return current;
    }

    let error_indices: BTreeSet<usize> = detect_error_items_for_preservation(items, None)
        .into_iter()
        .collect();

    let outlier_indices: BTreeSet<usize> = detect_structural_outliers(items).into_iter().collect();

    let anomaly_indices = numeric_anomaly_indices(config, items, analysis);

    let mut prioritized: BTreeSet<usize> = BTreeSet::new();
    prioritized.extend(&error_indices);
    prioritized.extend(&outlier_indices);
    prioritized.extend(&anomaly_indices);

    let mut remaining = effective_max.saturating_sub(prioritized.len());
    if remaining > 0 {
        for i in 0..3.min(n) {
            if !prioritized.contains(&i) && remaining > 0 {
                prioritized.insert(i);
                remaining -= 1;
            }
        }
        let last_start = n.saturating_sub(2);
        for i in last_start..n {
            if !prioritized.contains(&i) && remaining > 0 {
                prioritized.insert(i);
                remaining -= 1;
            }
        }
    }

    if remaining > 0 {
        let mut others: Vec<usize> = current.difference(&prioritized).copied().collect();
        others.sort();
        for i in others {
            if remaining == 0 {
                break;
            }
            prioritized.insert(i);
            remaining -= 1;
        }
    }

    prioritized.into_iter().take(effective_max).collect()
}

fn numeric_anomaly_indices(
    config: &SmartCrusherConfig,
    items: &[Value],
    analysis: Option<&ArrayAnalysis>,
) -> BTreeSet<usize> {
    let mut anomalies: BTreeSet<usize> = BTreeSet::new();
    let Some(analysis) = analysis else {
        return anomalies;
    };
    if analysis.field_stats.is_empty() {
        return anomalies;
    }

    for (field_name, stats) in &analysis.field_stats {
        if !is_numeric_field_with_variance(stats) {
            continue;
        }
        let (Some(mean_val), Some(var)) = (stats.mean_val, stats.variance) else {
            continue;
        };
        if var <= 0.0 {
            continue;
        }
        let std = var.sqrt();
        if std <= 0.0 {
            continue;
        }
        let threshold = config.variance_threshold * std;
        for (i, item) in items.iter().enumerate() {
            let Some(obj) = item.as_object() else {
                continue;
            };
            let Some(v) = obj.get(field_name) else {
                continue;
            };
            if let Some(num) = v.as_f64() {
                if !num.is_nan() && (num - mean_val).abs() > threshold {
                    anomalies.insert(i);
                }
            }
        }
    }

    anomalies
}

fn is_numeric_field_with_variance(stats: &FieldStats) -> bool {
    stats.field_type == "numeric" && stats.mean_val.is_some() && stats.variance.unwrap_or(0.0) > 0.0
}

fn item_content_hash(item: &Value, idx: usize) -> String {
    if item.is_object() || item.is_array() {
        compute_item_hash(item)
    } else {
        let content = match item {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "None".to_string(),
            _ => format!("__idx_{}__", idx),
        };
        blake3::hash(content.as_bytes()).to_hex()[..16].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> SmartCrusherConfig {
        SmartCrusherConfig::default()
    }

    fn idx_set(indices: &[usize]) -> BTreeSet<usize> {
        indices.iter().copied().collect()
    }

    #[test]
    fn dedup_empty_input() {
        let result = deduplicate_indices_by_content(&BTreeSet::new(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn dedup_lowest_index_wins_for_duplicates() {
        let items = vec![
            json!({"name": "alice"}),
            json!({"name": "alice"}),
            json!({"name": "bob"}),
        ];
        let kept = idx_set(&[0, 1, 2]);
        let result = deduplicate_indices_by_content(&kept, &items);
        assert_eq!(result, idx_set(&[0, 2]));
    }

    #[test]
    fn dedup_all_distinct_unchanged() {
        let items = vec![json!({"id": 1}), json!({"id": 2}), json!({"id": 3})];
        let kept = idx_set(&[0, 1, 2]);
        let result = deduplicate_indices_by_content(&kept, &items);
        assert_eq!(result, idx_set(&[0, 1, 2]));
    }

    #[test]
    fn dedup_skips_out_of_bounds() {
        let items = vec![json!({"a": 1})];
        let kept = idx_set(&[0, 5, 10]);
        let result = deduplicate_indices_by_content(&kept, &items);
        assert_eq!(result, idx_set(&[0]));
    }

    #[test]
    fn dedup_key_order_independent() {
        let items = vec![json!({"b": 2, "a": 1}), json!({"a": 1, "b": 2})];
        let kept = idx_set(&[0, 1]);
        let result = deduplicate_indices_by_content(&kept, &items);
        assert_eq!(result.len(), 1);
        assert!(result.contains(&0));
    }

    #[test]
    fn fill_when_at_or_over_budget_returns_unchanged() {
        let items: Vec<Value> = (0..10).map(|i| json!({"id": i})).collect();
        let kept = idx_set(&[0, 1, 2, 3, 4]);
        let result = fill_remaining_slots(&kept, &items, items.len(), 5);
        assert_eq!(result, kept);
    }

    #[test]
    fn fill_adds_diverse_uniques_up_to_max() {
        let items: Vec<Value> = (0..20).map(|i| json!({"id": i})).collect();
        let kept = idx_set(&[0, 5]);
        let result = fill_remaining_slots(&kept, &items, items.len(), 10);
        assert!(result.len() <= 10);
        assert!(result.len() >= 2);
        assert!(result.contains(&0));
        assert!(result.contains(&5));
    }

    #[test]
    fn fill_skips_content_duplicates() {
        let mut items: Vec<Value> = (0..10).map(|i| json!({"id": i})).collect();
        items.extend(std::iter::repeat_with(|| json!({"id": 0})).take(10));
        let kept = idx_set(&[0]);
        let result = fill_remaining_slots(&kept, &items, items.len(), 15);
        for i in 10..20 {
            assert!(!result.contains(&i), "dup index {} should not be added", i);
        }
    }

    #[test]
    fn prioritize_under_budget_passthrough_after_dedup() {
        let items: Vec<Value> = (0..5).map(|i| json!({"id": i})).collect();
        let kept = idx_set(&[0, 1, 2]);
        let result = prioritize_indices(&cfg(), &kept, &items, items.len(), None, 10);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn prioritize_dedup_collapses_then_returns_under_max() {
        let items = vec![
            json!({"name": "alice"}),
            json!({"name": "alice"}),
            json!({"name": "bob"}),
        ];
        let kept = idx_set(&[0, 1, 2]);
        let result = prioritize_indices(&cfg(), &kept, &items, items.len(), None, 10);
        assert_eq!(result, idx_set(&[0, 2]));
    }

    #[test]
    fn prioritize_keeps_error_items_when_over_budget() {
        let mut items: Vec<Value> = (0..30)
            .map(|i| json!({"id": i, "msg": format!("ok {}", i)}))
            .collect();
        items.push(json!({"id": 30, "msg": "FATAL: out of memory"}));
        let kept: BTreeSet<usize> = (0..items.len()).collect();
        let result = prioritize_indices(&cfg(), &kept, &items, items.len(), None, 10);
        assert!(
            result.contains(&30),
            "error item must survive prioritization"
        );
    }

    #[test]
    fn prioritize_includes_first_3_and_last_2_when_room() {
        let items: Vec<Value> = (0..30).map(|i| json!({"id": i, "v": i})).collect();
        let kept: BTreeSet<usize> = (5..15).collect();
        let result = prioritize_indices(&cfg(), &kept, &items, items.len(), None, 10);
        assert!(result.len() <= 10);
    }
}
