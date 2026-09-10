use serde_json::Value;

use super::statistics::{calculate_string_entropy, detect_sequential_pattern, is_uuid_format};
use super::types::FieldStats;

pub fn detect_id_field_statistically(stats: &FieldStats, values: &[Value]) -> (bool, f64) {
    if stats.unique_ratio < 0.9 {
        return (false, 0.0);
    }

    if stats.field_type == "string" {
        let sample_values: Vec<&str> = values.iter().take(20).filter_map(|v| v.as_str()).collect();

        if !sample_values.is_empty() {
            let uuid_count = sample_values.iter().filter(|s| is_uuid_format(s)).count();
            if (uuid_count as f64 / sample_values.len() as f64) > 0.8 {
                return (true, 0.95);
            }

            let avg_entropy = sample_values
                .iter()
                .map(|s| calculate_string_entropy(s))
                .sum::<f64>()
                / sample_values.len() as f64;
            if avg_entropy > 0.7 && stats.unique_ratio > 0.95 {
                return (true, 0.8);
            }
        }
    }

    if stats.field_type == "numeric" {
        if detect_sequential_pattern(values, true) && stats.unique_ratio > 0.95 {
            return (true, 0.9);
        }

        if let (Some(min_v), Some(max_v)) = (stats.min_val, stats.max_val) {
            let value_range = max_v - min_v;
            if value_range > 0.0 && stats.unique_ratio > 0.95 {
                return (true, 0.85);
            }
        }
    }

    if stats.unique_ratio > 0.98 {
        return (true, 0.7);
    }

    (false, 0.0)
}

pub fn detect_score_field_statistically(stats: &FieldStats, items: &[Value]) -> (bool, f64) {
    if stats.field_type != "numeric" {
        return (false, 0.0);
    }

    let (min_val, max_val) = match (stats.min_val, stats.max_val) {
        (Some(min_v), Some(max_v)) => (min_v, max_v),
        _ => return (false, 0.0),
    };

    let mut confidence: f64 = 0.0;

    let is_bounded = if (0.0..=1.0).contains(&min_val) && (0.0..=1.0).contains(&max_val) {
        confidence += 0.4;
        true
    } else if (0.0..=10.0).contains(&min_val) && (0.0..=10.0).contains(&max_val) {
        confidence += 0.3;
        true
    } else if (0.0..=100.0).contains(&min_val) && (0.0..=100.0).contains(&max_val) {
        confidence += 0.25;
        true
    } else if min_val >= -1.0 && max_val <= 1.0 {
        confidence += 0.35;
        true
    } else {
        false
    };

    if !is_bounded {
        return (false, 0.0);
    }

    let sample_values: Vec<&Value> = items
        .iter()
        .take(50)
        .filter_map(|item| item.as_object().and_then(|m| m.get(&stats.name)))
        .collect();

    let sample_owned: Vec<Value> = sample_values.iter().map(|v| (*v).clone()).collect();
    if detect_sequential_pattern(&sample_owned, true) {
        return (false, 0.0);
    }

    let values_in_order: Vec<f64> = items
        .iter()
        .filter_map(|item| item.as_object().and_then(|m| m.get(&stats.name)))
        .filter_map(|v| v.as_f64())
        .filter(|f| f.is_finite())
        .collect();

    if values_in_order.len() >= 5 {
        let num_pairs = values_in_order.len() - 1;
        let descending_count = values_in_order.windows(2).filter(|w| w[0] >= w[1]).count();
        if num_pairs > 0 && (descending_count as f64 / num_pairs as f64) > 0.7 {
            confidence += 0.3;
        }
    }

    let first_20: &[f64] = if values_in_order.len() > 20 {
        &values_in_order[..20]
    } else {
        &values_in_order[..]
    };
    let float_count = first_20
        .iter()
        .filter(|&&v| v.is_finite() && v != v.trunc())
        .count();
    if !first_20.is_empty() && (float_count as f64) > (first_20.len() as f64 * 0.3) {
        confidence += 0.1;
    }

    let is_score = confidence >= 0.4;
    let bounded_confidence = confidence.min(0.95);
    (is_score, bounded_confidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn stats(name: &str, field_type: &str, unique_ratio: f64) -> FieldStats {
        FieldStats {
            name: name.to_string(),
            field_type: field_type.to_string(),
            count: 100,
            unique_count: (100.0 * unique_ratio) as usize,
            unique_ratio,
            is_constant: false,
            constant_value: None,
            min_val: None,
            max_val: None,
            mean_val: None,
            variance: None,
            change_points: Vec::new(),
            avg_length: None,
        }
    }

    fn stats_with_range(name: &str, min_v: f64, max_v: f64) -> FieldStats {
        let mut s = stats(name, "numeric", 1.0);
        s.min_val = Some(min_v);
        s.max_val = Some(max_v);
        s
    }

    #[test]
    fn id_field_low_uniqueness_rejected() {
        let s = stats("status", "string", 0.5);
        let values: Vec<Value> = vec![json!("ok"), json!("error"), json!("ok"), json!("ok")];
        assert_eq!(detect_id_field_statistically(&s, &values), (false, 0.0));
    }

    #[test]
    fn id_field_uuid_strings_high_confidence() {
        let s = stats("uid", "string", 1.0);
        let values: Vec<Value> = (0..20)
            .map(|i| json!(format!("550e8400-e29b-41d4-a716-{:012x}", i)))
            .collect();
        let (is_id, conf) = detect_id_field_statistically(&s, &values);
        assert!(is_id);
        assert_eq!(conf, 0.95);
    }

    #[test]
    fn id_field_high_entropy_strings() {
        let mut s = stats("uid", "string", 1.0);
        s.unique_ratio = 0.96;
        let values: Vec<Value> = (0..20)
            .map(|i| json!(format!("a3f7b2c{:06x}d8e1f4a7", i)))
            .collect();
        let (is_id, conf) = detect_id_field_statistically(&s, &values);
        assert!(is_id);
        assert!((conf - 0.8).abs() < 1e-9);
    }

    #[test]
    fn id_field_sequential_numeric() {
        let mut s = stats("id", "numeric", 1.0);
        s.unique_ratio = 0.96;
        s.min_val = Some(1.0);
        s.max_val = Some(100.0);
        let values: Vec<Value> = (1..=100).map(|i| json!(i)).collect();
        let (is_id, conf) = detect_id_field_statistically(&s, &values);
        assert!(is_id);
        assert!((conf - 0.9).abs() < 1e-9);
    }

    #[test]
    fn id_field_high_uniqueness_alone_triggers_catchall() {
        let mut s = stats("misc", "numeric", 0.99);
        s.min_val = Some(0.0);
        s.max_val = Some(0.0);
        let values: Vec<Value> = (0..100).map(|_| json!(0)).collect();
        let (is_id, conf) = detect_id_field_statistically(&s, &values);
        assert!(is_id);
        assert!((conf - 0.7).abs() < 1e-9);
    }

    #[test]
    fn score_field_unit_range_with_descending_sort() {
        let s = stats_with_range("score", 0.0, 1.0);
        let items: Vec<Value> = (0..10)
            .rev()
            .map(|i| json!({"score": (i as f64) / 10.0}))
            .collect();
        let (is_score, conf) = detect_score_field_statistically(&s, &items);
        assert!(is_score);
        assert!(conf >= 0.7);
        assert!(conf <= 0.95);
    }

    #[test]
    fn score_field_sequential_rejected() {
        let s = stats_with_range("score", 1.0, 10.0);
        let items: Vec<Value> = (1..=10).map(|i| json!({"score": i})).collect();
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(!is_score);
    }

    #[test]
    fn score_field_unbounded_range_rejected() {
        let s = stats_with_range("metric", 0.0, 1000.0);
        let items: Vec<Value> = (0..10).map(|i| json!({"metric": i * 100})).collect();
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(!is_score);
    }

    #[test]
    fn score_field_signed_similarity_range() {
        let s = stats_with_range("similarity", -0.9, 0.95);
        let items: Vec<Value> = (0..10)
            .rev()
            .map(|i| json!({"similarity": (i as f64) / 10.0 - 0.5}))
            .collect();
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(is_score);
    }

    #[test]
    fn score_field_below_threshold_rejected() {
        let s = stats_with_range("metric", 0.0, 100.0);
        let items: Vec<Value> = vec![
            json!({"metric": 50}),
            json!({"metric": 10}),
            json!({"metric": 80}),
            json!({"metric": 20}),
            json!({"metric": 90}),
        ];
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(!is_score);
    }

    #[test]
    fn score_field_non_numeric_rejected() {
        let s = stats("name", "string", 0.5);
        let items: Vec<Value> = vec![
            json!({"name": "alice"}),
            json!({"name": "bob"}),
            json!({"name": "alice"}),
        ];
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(!is_score);
    }

    #[test]
    fn score_field_missing_min_max_rejected() {
        let s = stats("score", "numeric", 1.0);
        let items: Vec<Value> = vec![json!({"score": 0.5})];
        let (is_score, _) = detect_score_field_statistically(&s, &items);
        assert!(!is_score);
    }

    #[test]
    fn score_field_confidence_capped_at_95() {
        let s = stats_with_range("score", 0.0, 1.0);
        let items: Vec<Value> = (0..50)
            .rev()
            .map(|i| json!({"score": (i as f64) / 50.0}))
            .collect();
        let (_, conf) = detect_score_field_statistically(&s, &items);
        assert!(conf <= 0.95);
    }
}
