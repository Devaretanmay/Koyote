use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompressionStrategy {
    None,
    Skip,
    TimeSeries,
    ClusterSample,
    TopN,
    SmartSample,
}

impl CompressionStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            CompressionStrategy::None => "none",
            CompressionStrategy::Skip => "skip",
            CompressionStrategy::TimeSeries => "time_series",
            CompressionStrategy::ClusterSample => "cluster",
            CompressionStrategy::TopN => "top_n",
            CompressionStrategy::SmartSample => "smart_sample",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FieldStats {
    pub name: String,
    pub field_type: String,
    pub count: usize,
    pub unique_count: usize,
    pub unique_ratio: f64,
    pub is_constant: bool,
    pub constant_value: Option<Value>,

    pub min_val: Option<f64>,
    pub max_val: Option<f64>,
    pub mean_val: Option<f64>,
    pub variance: Option<f64>,
    pub change_points: Vec<usize>,

    pub avg_length: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CrushabilityAnalysis {
    pub crushable: bool,
    pub reason: String,
}

impl CrushabilityAnalysis {
    #[allow(dead_code)]
    pub fn skip(reason: impl Into<String>) -> Self {
        CrushabilityAnalysis {
            crushable: false,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArrayAnalysis {
    pub item_count: usize,
    pub field_stats: BTreeMap<String, FieldStats>,
    pub detected_pattern: String,
    pub recommended_strategy: CompressionStrategy,
    pub constant_fields: BTreeMap<String, Value>,
    pub estimated_reduction: f64,
    pub crushability: Option<CrushabilityAnalysis>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CompressionPlan {
    pub strategy: CompressionStrategy,
    pub keep_indices: Vec<usize>,
    pub constant_fields: BTreeMap<String, Value>,
    pub cluster_field: Option<String>,
    pub sort_field: Option<String>,
    pub keep_count: usize,
}

impl Default for CompressionPlan {
    fn default() -> Self {
        CompressionPlan {
            strategy: CompressionStrategy::None,
            keep_indices: Vec::new(),
            constant_fields: BTreeMap::new(),
            cluster_field: None,
            sort_field: None,
            keep_count: 10,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CrushResult {
    pub compressed: String,
    pub original: String,
    pub was_modified: bool,
    pub strategy: String,
}

impl CrushResult {
    #[allow(dead_code)]
    pub fn passthrough(content: impl Into<String>) -> Self {
        let s = content.into();
        CrushResult {
            compressed: s.clone(),
            original: s,
            was_modified: false,
            strategy: "passthrough".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_strategy_strings_match_python() {
        assert_eq!(CompressionStrategy::None.as_str(), "none");
        assert_eq!(CompressionStrategy::Skip.as_str(), "skip");
        assert_eq!(CompressionStrategy::TimeSeries.as_str(), "time_series");
        assert_eq!(CompressionStrategy::ClusterSample.as_str(), "cluster");
        assert_eq!(CompressionStrategy::TopN.as_str(), "top_n");
        assert_eq!(CompressionStrategy::SmartSample.as_str(), "smart_sample");
    }

    #[test]
    fn crushability_skip_helper() {
        let r = CrushabilityAnalysis::skip("too small");
        assert!(!r.crushable);
        assert_eq!(r.reason, "too small");
    }

    #[test]
    fn compression_plan_default_keep_count_matches_python() {
        let p = CompressionPlan::default();
        assert_eq!(p.keep_count, 10);
        assert_eq!(p.strategy, CompressionStrategy::None);
        assert!(p.keep_indices.is_empty());
    }

    #[test]
    fn crush_result_passthrough() {
        let r = CrushResult::passthrough("hello");
        assert_eq!(r.compressed, "hello");
        assert_eq!(r.original, "hello");
        assert!(!r.was_modified);
        assert_eq!(r.strategy, "passthrough");
    }
}
