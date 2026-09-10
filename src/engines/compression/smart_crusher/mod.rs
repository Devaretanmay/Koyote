mod analyzer;

mod anchors;
mod classifier;
pub mod compaction;
mod crusher;
mod crushers;
mod field_detect;

mod orchestration;
mod outliers;
mod planning;
mod statistics;
mod stats_math;
mod types;

pub use crusher::SmartCrusher;

use std::collections::BTreeSet;

use blake3;
use serde_json::Value;

pub fn must_keep(items: &[Value], item_strings: Option<&[String]>) -> Vec<usize> {
    let mut kept: BTreeSet<usize> = BTreeSet::new();
    for idx in outliers::detect_error_items_for_preservation(items, item_strings) {
        kept.insert(idx);
    }
    for idx in outliers::detect_structural_outliers(items) {
        kept.insert(idx);
    }
    kept.into_iter().collect()
}

pub fn hash_field_name(field_name: &str) -> String {
    let h = blake3::hash(field_name.as_bytes());
    h.to_hex().as_str()[..8].to_string()
}

#[cfg(test)]
mod hashing_tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(hash_field_name(""), "af1349b9");
    }

    #[test]
    fn deterministic() {
        assert_eq!(hash_field_name("test"), hash_field_name("test"));
    }

    #[test]
    fn output_length_is_8() {
        assert_eq!(hash_field_name("a").len(), 8);
        assert_eq!(hash_field_name(&"x".repeat(1000)).len(), 8);
    }
}

pub const ERROR_KEYWORDS: &[&str] = &[
    "error",
    "exception",
    "failed",
    "failure",
    "critical",
    "fatal",
    "crash",
    "panic",
    "abort",
    "timeout",
    "denied",
    "rejected",
];

#[cfg(test)]
mod error_keywords_tests {
    use super::*;

    #[test]
    fn matches_python_count() {
        assert_eq!(ERROR_KEYWORDS.len(), 12);
    }

    #[test]
    fn all_lowercase_invariant() {
        for &kw in ERROR_KEYWORDS {
            assert_eq!(
                kw,
                kw.to_lowercase(),
                "ERROR_KEYWORDS must all be lowercase"
            );
        }
    }

    #[test]
    fn pinned_membership() {
        let expected = [
            "error",
            "exception",
            "failed",
            "failure",
            "critical",
            "fatal",
            "crash",
            "panic",
            "abort",
            "timeout",
            "denied",
            "rejected",
        ];
        let actual: std::collections::BTreeSet<&str> = ERROR_KEYWORDS.iter().copied().collect();
        let expected: std::collections::BTreeSet<&str> = expected.iter().copied().collect();
        assert_eq!(actual, expected);
    }
}

#[cfg(test)]
mod must_keep_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn must_keep_finds_error_items() {
        let mut items: Vec<Value> = (0..9).map(|i| json!({"id": i, "status": "ok"})).collect();
        items.push(json!({"id": 9, "status": "ERROR", "msg": "FATAL: boom"}));
        let kept = must_keep(&items, None);
        assert!(kept.contains(&9));
    }

    #[test]
    fn must_keep_uses_item_strings_when_provided() {
        let items: Vec<Value> = vec![json!({"a": 1}), json!({"a": "exception"})];
        let strings: Vec<String> = items
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect();
        let with_cache = must_keep(&items, Some(&strings));
        let without_cache = must_keep(&items, None);
        assert_eq!(with_cache, without_cache);
        assert!(with_cache.contains(&1));
    }

    #[test]
    fn must_keep_finds_structural_outliers() {
        let mut items: Vec<Value> = (0..20)
            .map(|i| json!({"id": i, "kind": "common"}))
            .collect();
        items.push(json!({"id": 20, "kind": "common", "rare_extra_field": "x"}));
        let kept = must_keep(&items, None);
        assert!(kept.contains(&20));
    }

    #[test]
    fn must_keep_merges_error_and_outlier_indices() {
        let mut items: Vec<Value> = (0..20)
            .map(|i| json!({"id": i, "kind": "common"}))
            .collect();
        items.push(json!({"id": 20, "kind": "common", "x": "rare"}));
        items.push(json!({"id": 21, "status": "error", "msg": "FATAL"}));
        let kept = must_keep(&items, None);
        assert!(kept.contains(&20));
        assert!(kept.contains(&21));
    }

    #[test]
    fn must_keep_handles_empty_array() {
        let kept = must_keep(&[], None);
        assert!(kept.is_empty());
    }
}

#[derive(Debug, Clone)]
pub struct SmartCrusherConfig {
    pub min_items_to_analyze: usize,
    pub min_tokens_to_crush: usize,
    pub variance_threshold: f64,
    pub max_items_after_crush: usize,
    pub preserve_change_points: bool,
    pub factor_out_constants: bool,
    pub dedup_identical_items: bool,
    pub first_fraction: f64,
    pub last_fraction: f64,
    pub relevance_threshold: f64,
    pub lossless_min_savings_ratio: f64,
    pub enable_ccr_marker: bool,
    pub lossless_only: bool,
    pub compaction_core_field_fraction: f64,
    pub compaction_heterogeneous_core_ratio: f64,
    pub compaction_max_flatten_inner_keys: usize,
    pub compaction_min_buckets: usize,
    pub compaction_max_buckets: usize,
    pub preview_count: usize,
}

impl Default for SmartCrusherConfig {
    fn default() -> Self {
        SmartCrusherConfig {
            min_items_to_analyze: 5,
            min_tokens_to_crush: 200,
            variance_threshold: 2.0,
            max_items_after_crush: 15,
            preserve_change_points: true,
            factor_out_constants: false,
            dedup_identical_items: true,
            first_fraction: 0.3,
            last_fraction: 0.15,
            relevance_threshold: 0.3,
            lossless_min_savings_ratio: 0.15,
            enable_ccr_marker: true,
            lossless_only: false,
            compaction_core_field_fraction: 0.8,
            compaction_heterogeneous_core_ratio: 0.6,
            compaction_max_flatten_inner_keys: 6,
            compaction_min_buckets: 2,
            compaction_max_buckets: 8,
            preview_count: 0,
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn defaults_match_python() {
        let c = SmartCrusherConfig::default();
        assert_eq!(c.min_items_to_analyze, 5);
        assert_eq!(c.min_tokens_to_crush, 200);
        assert_eq!(c.variance_threshold, 2.0);
        assert_eq!(c.max_items_after_crush, 15);
        assert!(c.preserve_change_points);
        assert!(!c.factor_out_constants);
        assert!(c.dedup_identical_items);
        assert_eq!(c.first_fraction, 0.3);
        assert_eq!(c.last_fraction, 0.15);
        assert_eq!(c.relevance_threshold, 0.3);
        assert_eq!(c.lossless_min_savings_ratio, 0.15);
        assert!(c.enable_ccr_marker);
        assert!(!c.lossless_only);
        assert_eq!(c.compaction_core_field_fraction, 0.8);
        assert_eq!(c.compaction_heterogeneous_core_ratio, 0.6);
        assert_eq!(c.compaction_max_flatten_inner_keys, 6);
        assert_eq!(c.compaction_min_buckets, 2);
        assert_eq!(c.compaction_max_buckets, 8);
        assert_eq!(c.preview_count, 0);
    }
}
