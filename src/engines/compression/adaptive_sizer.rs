use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;

pub fn compute_optimal_k(items: &[&str], bias: f64, min_k: usize, max_k: Option<usize>) -> usize {
    let n = items.len();
    let effective_max = max_k.unwrap_or(n);

    if n <= 8 {
        return n;
    }

    let unique_count = count_unique_simhash(items, 3);
    if unique_count <= 3 {
        let k = min_k.max(unique_count);
        return k.min(effective_max);
    }

    let curve = compute_unique_bigram_curve(items);
    let mut knee = find_knee(&curve);

    let diversity_ratio = unique_count as f64 / n as f64;

    knee = match knee {
        None => {
            let keep_fraction = 0.3 + 0.7 * diversity_ratio;
            Some(min_k.max((n as f64 * keep_fraction) as usize))
        }
        Some(k) if diversity_ratio > 0.7 => {
            let floor = min_k.max((n as f64 * (0.3 + 0.7 * diversity_ratio)) as usize);
            Some(k.max(floor))
        }
        some => some,
    };

    let knee = knee.unwrap_or(min_k);

    let mut k = min_k.max((knee as f64 * bias) as usize);
    k = k.min(effective_max);

    k = validate_with_zlib(items, k, effective_max, 0.15);

    min_k.max(k.min(effective_max))
}

pub fn find_knee(curve: &[usize]) -> Option<usize> {
    let n = curve.len();
    if n < 3 {
        return None;
    }

    let x_min: usize = 0;
    let x_max: usize = n - 1;
    let y_min = curve[0] as f64;
    let y_max = curve[n - 1] as f64;

    if (y_max - y_min).abs() < f64::EPSILON {
        return Some(1);
    }

    let x_range = (x_max - x_min) as f64;
    let y_range = y_max - y_min;

    let mut max_diff: f64 = -1.0;
    let mut knee_idx: Option<usize> = None;

    for (i, &y) in curve.iter().enumerate() {
        let x_norm = (i - x_min) as f64 / x_range;
        let y_norm = (y as f64 - y_min) / y_range;
        let diff = y_norm - x_norm;
        if diff > max_diff {
            max_diff = diff;
            knee_idx = Some(i);
        }
    }

    if max_diff < 0.05 {
        return None;
    }

    knee_idx.map(|i| i + 1)
}

pub fn compute_unique_bigram_curve(items: &[&str]) -> Vec<usize> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut curve: Vec<usize> = Vec::with_capacity(items.len());

    for item in items {
        let lower = item.to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        if words.len() < 2 {
            let first = words.first().copied().unwrap_or("");
            seen.insert((first.to_string(), String::new()));
        } else {
            for j in 0..words.len() - 1 {
                seen.insert((words[j].to_string(), words[j + 1].to_string()));
            }
        }
        curve.push(seen.len());
    }

    curve
}

pub fn simhash(text: &str) -> u64 {
    let lower = text.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let n = chars.len();

    let iter_count = if n <= 3 { 1 } else { n - 3 };

    let mut votes: [i32; 64] = [0; 64];

    for i in 0..iter_count {
        let gram: String = chars.iter().skip(i).take(4).collect();

        let mut hasher = DefaultHasher::new();
        gram.hash(&mut hasher);
        let h = hasher.finish();

        for (j, vote) in votes.iter_mut().enumerate() {
            if (h >> j) & 1 == 1 {
                *vote += 1;
            } else {
                *vote -= 1;
            }
        }
    }

    let mut fingerprint: u64 = 0;
    for (j, &v) in votes.iter().enumerate() {
        if v > 0 {
            fingerprint |= 1 << j;
        }
    }
    fingerprint
}

#[inline]
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

pub fn count_unique_simhash(items: &[&str], threshold: u32) -> usize {
    if items.is_empty() {
        return 0;
    }

    let fingerprints: Vec<u64> = items.iter().map(|s| simhash(s)).collect();
    let mut clusters: Vec<u64> = Vec::new();

    for &fp in &fingerprints {
        let mut matched = false;
        for &rep in &clusters {
            if hamming_distance(fp, rep) <= threshold {
                matched = true;
                break;
            }
        }
        if !matched {
            clusters.push(fp);
        }
    }

    clusters.len()
}

pub fn validate_with_zlib(items: &[&str], k: usize, max_k: usize, tolerance: f64) -> usize {
    if k >= items.len() || k >= max_k {
        return k;
    }

    let full_text = items.join("\n");
    let subset_text = items[..k].join("\n");

    if full_text.len() < 200 {
        return k;
    }

    let full_compressed = zlib_compressed_len(full_text.as_bytes());
    let subset_compressed = zlib_compressed_len(subset_text.as_bytes());

    let full_ratio = if !full_text.is_empty() {
        full_compressed as f64 / full_text.len() as f64
    } else {
        1.0
    };
    let subset_ratio = if !subset_text.is_empty() {
        subset_compressed as f64 / subset_text.len() as f64
    } else {
        1.0
    };

    let ratio_diff = (full_ratio - subset_ratio).abs();

    if ratio_diff > tolerance {
        let adjusted = ((k as f64) * 1.2) as usize;
        return adjusted.min(max_k);
    }

    k
}

fn zlib_compressed_len(bytes: &[u8]) -> usize {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).expect("in-memory write");
    let compressed = encoder.finish().expect("flush");
    compressed.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simhash_deterministic() {
        assert_eq!(simhash("hello"), simhash("hello"));
        assert_eq!(simhash(""), simhash(""));
    }

    #[test]
    fn simhash_lowercases_input() {
        assert_eq!(simhash("ABC"), simhash("abc"));
        assert_eq!(simhash("Hello"), simhash("hello"));
    }

    #[test]
    fn simhash_different_inputs_diverge() {
        assert_ne!(simhash("abc"), simhash("xyz"));
    }

    #[test]
    fn hamming_distance_zero_identical() {
        assert_eq!(hamming_distance(0, 0), 0);
        assert_eq!(hamming_distance(0xff, 0xff), 0);
    }

    #[test]
    fn hamming_distance_basic() {
        assert_eq!(hamming_distance(0b0000, 0b1111), 4);
        assert_eq!(hamming_distance(0b1010, 0b0101), 4);
        assert_eq!(hamming_distance(0b1100, 0b1010), 2);
    }

    #[test]
    fn hamming_distance_full_64_bits() {
        assert_eq!(hamming_distance(u64::MAX, 0), 64);
    }

    #[test]
    fn count_unique_simhash_empty() {
        assert_eq!(count_unique_simhash(&[], 3), 0);
    }

    #[test]
    fn count_unique_simhash_all_identical() {
        let items = ["abc", "abc", "abc"];
        assert_eq!(count_unique_simhash(&items, 3), 1);
    }

    #[test]
    fn count_unique_simhash_diverse_items() {
        let items = [
            "the cat sat on the mat",
            "the dog ran in the park",
            "a fish swam in the sea",
        ];
        assert_eq!(count_unique_simhash(&items, 3), 3);
    }

    #[test]
    fn count_unique_simhash_threshold_groups_near_dupes() {
        let items = ["abc", "abc"];
        assert_eq!(count_unique_simhash(&items, 0), 1);
    }

    #[test]
    fn bigram_curve_distinct_words() {
        let items = ["the cat", "the dog", "a fish"];
        assert_eq!(compute_unique_bigram_curve(&items), vec![1, 2, 3]);
    }

    #[test]
    fn bigram_curve_single_word_dedup() {
        let items = ["hello", "world", "hello"];
        assert_eq!(compute_unique_bigram_curve(&items), vec![1, 2, 2]);
    }

    #[test]
    fn bigram_curve_empty_string_contributes_one() {
        let items = ["", "a", "a b"];
        assert_eq!(compute_unique_bigram_curve(&items), vec![1, 2, 3]);
    }

    #[test]
    fn bigram_curve_lowercases_for_dedup() {
        let items = ["Hello", "hello"];
        assert_eq!(compute_unique_bigram_curve(&items), vec![1, 1]);
    }

    #[test]
    fn find_knee_too_short_is_none() {
        assert_eq!(find_knee(&[]), None);
        assert_eq!(find_knee(&[1]), None);
        assert_eq!(find_knee(&[1, 2]), None);
    }

    #[test]
    fn find_knee_flat_curve_returns_one() {
        assert_eq!(find_knee(&[5, 5, 5, 5, 5]), Some(1));
    }

    #[test]
    fn find_knee_concave_curve() {
        assert_eq!(find_knee(&[1, 5, 8, 9, 10, 10, 10, 10, 10]), Some(3));
    }

    #[test]
    fn find_knee_linear_no_clear_knee() {
        assert_eq!(find_knee(&[1, 2, 3, 4, 5, 6, 7, 8, 9]), None);
    }

    #[test]
    fn validate_zlib_passthrough_when_k_at_max() {
        let items = ["a", "b", "c"];
        assert_eq!(validate_with_zlib(&items, 3, 10, 0.15), 3);
    }

    #[test]
    fn validate_zlib_passthrough_when_total_too_small() {
        let items: [&str; 5] = ["short"; 5];
        assert_eq!(validate_with_zlib(&items, 2, 100, 0.15), 2);
    }

    #[test]
    fn validate_zlib_bumps_k_when_subset_undercompresses() {
        let items: [&str; 20] = ["the quick brown fox jumps over the lazy dog"; 20];
        let result = validate_with_zlib(&items, 5, 100, 0.15);
        assert_eq!(result, 6, "expected 1.2x bump from 5 to 6");
    }

    #[test]
    fn validate_zlib_passthrough_when_subset_representative() {
        let many: Vec<String> = (0..20)
            .map(|i| {
                format!(
                    "entry id={} payload=item value with content for item number {}",
                    i, i
                )
            })
            .collect();
        let items: Vec<&str> = many.iter().map(|s| s.as_str()).collect();
        let result = validate_with_zlib(&items, 10, 100, 0.15);
        assert_eq!(result, 10, "expected passthrough for representative subset");
    }

    #[test]
    fn compute_optimal_k_n_le_8_returns_n() {
        let items = ["a", "b", "c", "d", "e"];
        assert_eq!(compute_optimal_k(&items, 1.0, 3, None), 5);
    }

    #[test]
    fn compute_optimal_k_low_diversity_returns_unique_count() {
        let items: [&str; 10] = ["abc"; 10];
        assert_eq!(compute_optimal_k(&items, 1.0, 3, None), 3);
    }

    #[test]
    fn compute_optimal_k_all_unique_keeps_all() {
        let items: Vec<String> = (0..20)
            .map(|i| format!("unique item number {} with some long content", i))
            .collect();
        let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        assert_eq!(compute_optimal_k(&refs, 1.0, 3, None), 20);
    }

    #[test]
    fn compute_optimal_k_respects_max_k() {
        let items: Vec<String> = (0..20).map(|i| format!("item {}", i)).collect();
        let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        let k = compute_optimal_k(&refs, 1.0, 3, Some(10));
        assert!(k <= 10, "k={} should be <= max_k=10", k);
    }

    #[test]
    fn compute_optimal_k_respects_min_k() {
        let items: [&str; 20] = ["abc"; 20];
        let k = compute_optimal_k(&items, 1.0, 5, None);
        assert_eq!(k, 5);
    }

    #[test]
    fn compute_optimal_k_bias_keeps_more() {
        let items: Vec<String> = (0..30).map(|i| format!("item content {}", i)).collect();
        let refs: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
        let k_low = compute_optimal_k(&refs, 0.7, 3, None);
        let k_mid = compute_optimal_k(&refs, 1.0, 3, None);
        let k_high = compute_optimal_k(&refs, 1.5, 3, None);
        assert!(
            k_low <= k_mid,
            "bias 0.7 -> {} should be <= bias 1.0 -> {}",
            k_low,
            k_mid
        );
        assert!(
            k_mid <= k_high,
            "bias 1.0 -> {} should be <= bias 1.5 -> {}",
            k_mid,
            k_high
        );
    }
}
