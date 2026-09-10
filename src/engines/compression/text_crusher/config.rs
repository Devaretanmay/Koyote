#[derive(Debug, Clone)]
pub struct TextCrusherConfig {
    pub target_ratio: f64,
    pub w_recency: f64,
    pub w_relevance: f64,
    pub w_salience: f64,
    pub min_segment_chars: usize,
    pub near_dup_threshold: f64,
    pub min_segments_for_crush: usize,
}

impl Default for TextCrusherConfig {
    fn default() -> Self {
        TextCrusherConfig {
            target_ratio: 0.5,
            w_recency: 1.0,
            w_relevance: 2.0,
            w_salience: 1.5,
            min_segment_chars: 12,
            near_dup_threshold: 0.85,
            min_segments_for_crush: 6,
        }
    }
}
