use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashSet};

use super::stats_math::{format_g, mean, median, sample_stdev};
use super::SmartCrusherConfig;
use super::ERROR_KEYWORDS;
use crate::engines::compression::adaptive_sizer::compute_optimal_k;

pub fn compute_k_split(
    items: &[&str],
    config: &SmartCrusherConfig,
    bias: f64,
) -> (usize, usize, usize, usize) {
    let max_k = if config.max_items_after_crush > 0 {
        Some(config.max_items_after_crush)
    } else {
        None
    };
    let k_total = compute_optimal_k(items, bias, 3, max_k);
    let k_first_raw =
        1_usize.max((k_total as f64 * config.first_fraction).round_ties_even() as usize);
    let k_last_raw =
        1_usize.max((k_total as f64 * config.last_fraction).round_ties_even() as usize);
    let k_first = k_first_raw.min(k_total);
    let k_last = k_last_raw.min(k_total.saturating_sub(k_first));
    let k_importance = k_total.saturating_sub(k_first + k_last);
    (k_total, k_first, k_last, k_importance)
}

pub fn crush_string_array(
    items: &[&str],
    config: &SmartCrusherConfig,
    bias: f64,
) -> (Vec<String>, String) {
    let n = items.len();
    if n <= 8 {
        return (
            items.iter().map(|s| (*s).to_string()).collect(),
            "string:passthrough".to_string(),
        );
    }

    let (k_total, k_first, k_last, _k_importance) = compute_k_split(items, config, bias);

    let mut error_indices: BTreeSet<usize> = BTreeSet::new();
    for (i, s) in items.iter().enumerate() {
        let lower = s.to_lowercase();
        if ERROR_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
            error_indices.insert(i);
        }
    }

    let lengths: Vec<f64> = items.iter().map(|s| s.chars().count() as f64).collect();
    let mut anomaly_indices: BTreeSet<usize> = BTreeSet::new();
    if lengths.len() > 1 {
        let mean_len = mean(&lengths).unwrap_or(0.0);
        let std_len = sample_stdev(&lengths).unwrap_or(0.0);
        if std_len > 0.0 {
            let threshold = config.variance_threshold * std_len;
            for (i, &length) in lengths.iter().enumerate() {
                if (length - mean_len).abs() > threshold {
                    anomaly_indices.insert(i);
                }
            }
        }
    }

    let first_indices: BTreeSet<usize> = (0..k_first.min(n)).collect();
    let last_start = n.saturating_sub(k_last);
    let last_indices: BTreeSet<usize> = (last_start..n).collect();

    let mut keep_indices: BTreeSet<usize> = BTreeSet::new();
    keep_indices.extend(error_indices.iter().copied());
    keep_indices.extend(anomaly_indices.iter().copied());
    keep_indices.extend(first_indices.iter().copied());
    keep_indices.extend(last_indices.iter().copied());

    let mut seen: HashSet<&str> = HashSet::new();
    for &i in &keep_indices {
        seen.insert(items[i]);
    }

    let mut dedup_count: usize = 0;
    let remaining_budget = k_total.saturating_sub(keep_indices.len());
    if remaining_budget > 0 {
        let stride = ((n.saturating_sub(1)) / (remaining_budget + 1)).max(1);
        let cap = k_total + error_indices.len() + anomaly_indices.len();
        let mut i: usize = 0;
        while i < n {
            if keep_indices.len() >= cap {
                break;
            }
            if !keep_indices.contains(&i) {
                if !seen.contains(items[i]) {
                    keep_indices.insert(i);
                    seen.insert(items[i]);
                } else {
                    dedup_count += 1;
                }
            }
            i += stride;
        }
    }

    let result: Vec<String> = keep_indices.iter().map(|&i| items[i].to_string()).collect();

    let mut strategy = format!("string:adaptive({}->{}", n, result.len());
    if dedup_count > 0 {
        strategy.push_str(&format!(",dedup={}", dedup_count));
    }
    if !error_indices.is_empty() {
        strategy.push_str(&format!(",errors={}", error_indices.len()));
    }
    strategy.push(')');

    (result, strategy)
}

pub fn crush_number_array(
    items: &[Value],
    config: &SmartCrusherConfig,
    bias: f64,
) -> (Vec<Value>, String) {
    let n = items.len();
    if n <= 8 {
        return (items.to_vec(), "number:passthrough".to_string());
    }

    let finite: Vec<f64> = items
        .iter()
        .filter_map(|v| v.as_f64().filter(|f| f.is_finite()))
        .collect();
    if finite.is_empty() {
        return (items.to_vec(), "number:no_finite".to_string());
    }

    let item_strings: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let item_str_refs: Vec<&str> = item_strings.iter().map(|s| s.as_str()).collect();
    let (k_total, k_first, k_last, _) = compute_k_split(&item_str_refs, config, bias);

    let mean_val = mean(&finite).unwrap_or(0.0);
    let median_val = median(&finite).unwrap_or(0.0);
    let std_val = if finite.len() > 1 {
        sample_stdev(&finite).unwrap_or(0.0)
    } else {
        0.0
    };

    let mut sorted_finite: Vec<f64> = finite.clone();
    sorted_finite.sort_by(f64::total_cmp);

    let p25 = percentile_linear(&sorted_finite, 0.25);
    let p75 = percentile_linear(&sorted_finite, 0.75);

    let mut outlier_indices: BTreeSet<usize> = BTreeSet::new();
    if std_val > 0.0 {
        let threshold = config.variance_threshold * std_val;
        for (i, val) in items.iter().enumerate() {
            if let Some(num) = val.as_f64().filter(|f| f.is_finite()) {
                if (num - mean_val).abs() > threshold {
                    outlier_indices.insert(i);
                }
            }
        }
    }

    let mut change_indices: BTreeSet<usize> = BTreeSet::new();
    if config.preserve_change_points && n > 10 {
        let window: usize = 5;
        for i in window..n.saturating_sub(window) {
            let left: Vec<f64> = items[i - window..i]
                .iter()
                .filter_map(|v| v.as_f64().filter(|f| f.is_finite()))
                .collect();
            let right: Vec<f64> = items[i..i + window]
                .iter()
                .filter_map(|v| v.as_f64().filter(|f| f.is_finite()))
                .collect();
            if !left.is_empty() && !right.is_empty() {
                let left_mean = mean(&left).unwrap_or(0.0);
                let right_mean = mean(&right).unwrap_or(0.0);
                if std_val > 0.0
                    && (right_mean - left_mean).abs() > config.variance_threshold * std_val
                {
                    change_indices.insert(i);
                }
            }
        }
    }

    let first_indices: BTreeSet<usize> = (0..k_first.min(n)).collect();
    let last_start = n.saturating_sub(k_last);
    let last_indices: BTreeSet<usize> = (last_start..n).collect();

    let mut keep_indices: BTreeSet<usize> = BTreeSet::new();
    keep_indices.extend(outlier_indices.iter().copied());
    keep_indices.extend(change_indices.iter().copied());
    keep_indices.extend(first_indices.iter().copied());
    keep_indices.extend(last_indices.iter().copied());

    let remaining_budget = k_total.saturating_sub(keep_indices.len());
    if remaining_budget > 0 {
        let stride = ((n.saturating_sub(1)) / (remaining_budget + 1)).max(1);
        let cap = k_total + outlier_indices.len();
        let mut i: usize = 0;
        while i < n {
            if keep_indices.len() >= cap {
                break;
            }
            if !keep_indices.contains(&i) {
                keep_indices.insert(i);
            }
            i += stride;
        }
    }

    let kept_values: Vec<Value> = keep_indices.iter().map(|&i| items[i].clone()).collect();

    let mn = finite_min(&finite);
    let mx = finite_max(&finite);
    let mut strategy = format!(
        "number:adaptive({}->{},min={},max={},mean={},median={},stddev={},p25={},p75={}",
        n,
        kept_values.len(),
        format_number_repr(mn),
        format_number_repr(mx),
        format_g(mean_val),
        format_g(median_val),
        format_g(std_val),
        format_g(p25),
        format_g(p75),
    );
    if !outlier_indices.is_empty() {
        strategy.push_str(&format!(",outliers={}", outlier_indices.len()));
    }
    if !change_indices.is_empty() {
        strategy.push_str(&format!(",change_points={}", change_indices.len()));
    }
    strategy.push(')');

    (kept_values, strategy)
}

pub fn crush_object(
    obj: &Map<String, Value>,
    config: &SmartCrusherConfig,
    bias: f64,
) -> (Map<String, Value>, String) {
    let n = obj.len();
    if n <= 8 {
        return (obj.clone(), "object:passthrough".to_string());
    }

    let mut kv_tokens: Vec<(String, usize)> = Vec::with_capacity(n);
    let mut total_tokens: usize = 0;
    for (key, val) in obj {
        let val_str = serde_json::to_string(val).unwrap_or_default();
        let tokens = val_str.len() / 4 + key.len() / 4 + 2;
        kv_tokens.push((key.clone(), tokens));
        total_tokens += tokens;
    }

    if total_tokens < config.min_tokens_to_crush {
        return (obj.clone(), "object:passthrough".to_string());
    }

    let keys: Vec<&String> = obj.keys().collect();
    let kv_strings: Vec<String> = keys
        .iter()
        .map(|k| {
            format!(
                "{}: {}",
                k,
                serde_json::to_string(&obj[k.as_str()]).unwrap_or_default()
            )
        })
        .collect();
    let kv_refs: Vec<&str> = kv_strings.iter().map(|s| s.as_str()).collect();

    let max_k = if config.max_items_after_crush > 0 {
        Some(config.max_items_after_crush)
    } else {
        None
    };
    let k_total = compute_optimal_k(&kv_refs, bias, 3, max_k);

    if k_total >= n {
        return (obj.clone(), "object:passthrough".to_string());
    }

    let mut keep_keys: HashSet<String> = HashSet::new();
    for (key, val) in obj {
        let val_str = serde_json::to_string(val)
            .unwrap_or_default()
            .to_lowercase();
        if ERROR_KEYWORDS.iter().any(|kw| val_str.contains(kw)) {
            keep_keys.insert(key.clone());
        }
    }

    let small_threshold_tokens = 50_usize / 4;
    for (key, tokens) in &kv_tokens {
        if *tokens <= small_threshold_tokens {
            keep_keys.insert(key.clone());
        }
    }

    let k_first = 1_usize.max((k_total as f64 * config.first_fraction).round_ties_even() as usize);
    let k_last = 1_usize.max((k_total as f64 * config.last_fraction).round_ties_even() as usize);
    for k in keys.iter().take(k_first) {
        keep_keys.insert((*k).clone());
    }
    for k in keys.iter().rev().take(k_last) {
        keep_keys.insert((*k).clone());
    }

    let remaining = k_total.saturating_sub(keep_keys.len());
    if remaining > 0 {
        let stride = ((n.saturating_sub(1)) / (remaining + 1)).max(1);
        let mut i: usize = 0;
        while i < n {
            let error_kept_count = keep_keys
                .iter()
                .filter(|k| {
                    let s = serde_json::to_string(&obj[k.as_str()])
                        .unwrap_or_default()
                        .to_lowercase();
                    ERROR_KEYWORDS.iter().any(|kw| s.contains(kw))
                })
                .count();
            if keep_keys.len() >= k_total + error_kept_count {
                break;
            }
            keep_keys.insert(keys[i].clone());
            i += stride;
        }
    }

    let mut result: Map<String, Value> = Map::new();
    for k in &keys {
        if keep_keys.contains(k.as_str()) {
            result.insert((*k).clone(), obj[k.as_str()].clone());
        }
    }

    let strategy = format!("object:adaptive({}->{} keys)", n, result.len());
    (result, strategy)
}

fn percentile_linear(sorted_values: &[f64], q: f64) -> f64 {
    let n = sorted_values.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted_values[0];
    }
    let pos = q * (n - 1) as f64;
    let lo = pos as usize;
    let hi = if lo + 1 < n { lo + 1 } else { lo };
    let frac = pos - lo as f64;
    sorted_values[lo] * (1.0 - frac) + sorted_values[hi] * frac
}

fn finite_min(values: &[f64]) -> f64 {
    values.iter().cloned().reduce(f64::min).unwrap_or(0.0)
}

fn finite_max(values: &[f64]) -> f64 {
    values.iter().cloned().reduce(f64::max).unwrap_or(0.0)
}

fn format_number_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    if x.fract() == 0.0 && x.abs() < 1e16 {
        return format!("{}", x as i64);
    }
    format!("{}", x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> SmartCrusherConfig {
        SmartCrusherConfig::default()
    }

    #[test]
    fn k_split_below_threshold_returns_n() {
        let items = ["a", "b", "c", "d", "e"];
        let (kt, kf, kl, ki) = compute_k_split(&items, &cfg(), 1.0);
        assert_eq!(kt, 5);
        assert_eq!(kf, 2);
        assert_eq!(kl, 1);
        assert_eq!(ki, 2);
    }

    #[test]
    fn bug4_k_split_no_overshoot_when_k_total_is_one() {
        let items: [&str; 1] = ["only"];
        let (kt, kf, kl, ki) = compute_k_split(&items, &cfg(), 1.0);
        assert_eq!(kt, 1, "n=1 triggers fast-path n<=8 -> k_total=1");
        assert!(
            kf + kl <= kt,
            "BUG #4: k_first={} + k_last={} must not exceed k_total={}",
            kf,
            kl,
            kt
        );
        assert_eq!(ki, kt.saturating_sub(kf + kl));
    }

    #[test]
    fn bug4_k_split_no_overshoot_when_k_total_is_two() {
        let items: [&str; 2] = ["a", "b"];
        let (kt, kf, kl, _) = compute_k_split(&items, &cfg(), 1.0);
        assert_eq!(kt, 2);
        assert!(kf + kl <= kt);
        assert_eq!(kf, 1);
        assert_eq!(kl, 1);
    }

    #[test]
    fn k_split_low_diversity_returns_min_k() {
        let items: [&str; 10] = ["x"; 10];
        let (kt, kf, kl, _) = compute_k_split(&items, &cfg(), 1.0);
        assert_eq!(kt, 3, "low-diversity -> max(min_k, unique_count)=3");
        assert_eq!(kf, 1);
        assert_eq!(kl, 1);
    }

    #[test]
    fn string_array_passthrough_at_threshold() {
        let items: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];
        let (out, strat) = crush_string_array(&items, &cfg(), 1.0);
        assert_eq!(out.len(), 8);
        assert_eq!(strat, "string:passthrough");
    }

    #[test]
    fn string_array_keeps_error_strings() {
        let items: Vec<&str> = (0..30)
            .map(|i| {
                if i == 15 {
                    "FATAL: out of memory"
                } else {
                    "ok"
                }
            })
            .collect();
        let (out, strat) = crush_string_array(&items, &cfg(), 1.0);
        assert!(out.iter().any(|s| s == "FATAL: out of memory"));
        assert!(strat.contains("errors=1"));
    }

    #[test]
    fn string_array_keeps_first_and_last() {
        let items: Vec<String> = (0..30).map(|i| format!("item_{}", i)).collect();
        let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        let (out, _) = crush_string_array(&refs, &cfg(), 1.0);
        assert!(out.iter().any(|s| s == "item_0"));
        assert!(out.iter().any(|s| s == "item_29"));
    }

    #[test]
    fn string_array_dedup_count_appears_in_strategy() {
        let items: Vec<&str> = std::iter::repeat_n("dup", 50).collect();
        let (_out, strat) = crush_string_array(&items, &cfg(), 1.0);
        assert!(
            strat.contains("dedup="),
            "strategy {} should mention dedup",
            strat
        );
    }

    #[test]
    fn number_array_passthrough_at_threshold() {
        let items: Vec<Value> = (0..8).map(|i| json!(i)).collect();
        let (out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert_eq!(out.len(), 8);
        assert_eq!(strat, "number:passthrough");
    }

    #[test]
    fn number_array_no_finite_returns_passthrough() {
        let items: Vec<Value> = (0..15).map(|_| json!(null)).collect();
        let (out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert_eq!(out.len(), items.len());
        assert_eq!(strat, "number:no_finite");
    }

    #[test]
    fn number_array_keeps_outliers() {
        let mut items: Vec<Value> = vec![json!(0); 30];
        items.push(json!(1000));
        let (out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert!(out.iter().any(|v| v.as_f64() == Some(1000.0)));
        assert!(strat.contains("outliers="));
    }

    #[test]
    fn number_array_strategy_string_includes_summary() {
        let items: Vec<Value> = (1..=20).map(|i| json!(i)).collect();
        let (_out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert!(strat.starts_with("number:adaptive("));
        assert!(strat.contains("min=1"));
        assert!(strat.contains("max=20"));
        assert!(strat.contains("mean="));
        assert!(strat.contains("median="));
        assert!(strat.contains("p25="));
        assert!(strat.contains("p75="));
    }

    #[test]
    fn object_passthrough_when_few_keys() {
        let mut obj = Map::new();
        for i in 0..5 {
            obj.insert(format!("k{}", i), json!(i));
        }
        let (out, strat) = crush_object(&obj, &cfg(), 1.0);
        assert_eq!(out.len(), 5);
        assert_eq!(strat, "object:passthrough");
    }

    #[test]
    fn object_passthrough_when_total_tokens_below_min() {
        let mut obj = Map::new();
        for i in 0..30 {
            obj.insert(format!("k{}", i), json!(i));
        }
        let (_out, strat) = crush_object(&obj, &cfg(), 1.0);
        assert_eq!(strat, "object:passthrough");
    }

    #[test]
    fn object_crushes_when_token_budget_exceeded() {
        let mut obj = Map::new();
        for i in 0..30 {
            obj.insert(
                format!("k{:02}", i),
                json!(format!(
                    "this is a relatively long value string for entry number {} with content",
                    i
                )),
            );
        }
        let (out, strat) = crush_object(&obj, &cfg(), 1.0);
        if strat == "object:passthrough" {
            assert_eq!(out.len(), 30);
        } else {
            assert!(strat.starts_with("object:adaptive("));
            assert!(out.len() <= 30);
        }
    }

    #[test]
    fn object_keeps_small_values() {
        let mut obj = Map::new();
        obj.insert("tiny".to_string(), json!(1));
        for i in 0..30 {
            obj.insert(
                format!("big{:02}", i),
                json!(format!(
                    "this is a long string with content for entry number {} that exceeds the small threshold",
                    i
                )),
            );
        }
        let (out, _) = crush_object(&obj, &cfg(), 1.0);
        assert!(
            out.contains_key("tiny"),
            "tiny key (small value) must survive"
        );
    }

    #[test]
    fn object_keeps_error_keywords() {
        let mut obj = Map::new();
        obj.insert(
            "msg1".to_string(),
            json!(format!("FATAL: {}", "x".repeat(200))),
        );
        for i in 0..30 {
            obj.insert(
                format!("k{:02}", i),
                json!(format!("padding content for entry {} with text", i)),
            );
        }
        let (out, _) = crush_object(&obj, &cfg(), 1.0);
        assert!(
            out.contains_key("msg1"),
            "key with error-keyword value must survive"
        );
    }

    #[test]
    fn bug1_percentile_proper_linear_interpolation() {
        let mut items: Vec<Value> = (1..=9).map(|i| json!(i)).collect();
        items.extend(vec![json!(null); 5]);
        let (_out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert!(strat.contains("p25=3"), "got: {}", strat);
        assert!(strat.contains("p75=7"), "got: {}", strat);
    }

    #[test]
    fn bug1_percentile_interpolates_when_index_non_integer() {
        let items: Vec<Value> = (1..=10).map(|i| json!(i * 10)).collect();
        let (_out, strat) = crush_number_array(&items, &cfg(), 1.0);
        assert!(
            strat.contains("p25=32.5"),
            "expected proper-percentile p25=32.5, got: {}",
            strat
        );
        assert!(
            strat.contains("p75=77.5"),
            "expected proper-percentile p75=77.5, got: {}",
            strat
        );
    }
}
