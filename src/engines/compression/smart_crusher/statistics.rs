use serde_json::Value;
use std::collections::HashMap;

pub fn is_uuid_format(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }

    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    for (part, &expected_len) in parts.iter().zip(expected_lens.iter()) {
        if part.len() != expected_len {
            return false;
        }
        for c in part.chars() {
            if !c.is_ascii_hexdigit() {
                return false;
            }
        }
    }
    true
}

pub fn calculate_string_entropy(s: &str) -> f64 {
    let n = s.chars().count();
    if n < 2 {
        return 0.0;
    }

    let mut freq: HashMap<char, usize> = HashMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }

    let length = n as f64;
    let mut entropy = 0.0_f64;
    for &count in freq.values() {
        let p = count as f64 / length;
        if p > 0.0 {
            entropy -= p * p.log2();
        }
    }

    let max_entropy = (freq.len().min(n) as f64).log2();
    if max_entropy > 0.0 {
        entropy / max_entropy
    } else {
        0.0
    }
}

fn python_int_parse(s: &str) -> Option<i64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let cleaned: String = if trimmed.contains('_') {
        let bytes = trimmed.as_bytes();
        let starts_or_ends =
            bytes[0] == b'_' || *bytes.last().unwrap() == b'_' || trimmed.contains("__");
        if starts_or_ends {
            return None;
        }
        trimmed.replace('_', "")
    } else {
        trimmed.to_string()
    };
    cleaned.parse::<i64>().ok()
}

pub fn detect_sequential_pattern(values: &[Value], check_order: bool) -> bool {
    if values.len() < 5 {
        return false;
    }

    let mut nums: Vec<f64> = Vec::new();
    let mut had_non_string_numeric = false;

    for v in values {
        match v {
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    nums.push(f);
                    had_non_string_numeric = true;
                }
            }
            Value::Bool(_) => {}
            Value::String(s) => {
                if let Some(parsed) = python_int_parse(s) {
                    nums.push(parsed as f64);
                }
            }
            _ => {}
        }
    }

    if nums.len() < 5 {
        return false;
    }

    if !had_non_string_numeric {
        return false;
    }

    if nums.len() < 2 {
        return false;
    }

    let mut sorted_nums = nums.clone();
    sorted_nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let diffs: Vec<f64> = sorted_nums.windows(2).map(|w| w[1] - w[0]).collect();
    if diffs.is_empty() {
        return false;
    }

    let avg_diff: f64 = diffs.iter().sum::<f64>() / diffs.len() as f64;
    if !(0.5..=2.0).contains(&avg_diff) {
        return false;
    }

    let consistent_count = diffs.iter().filter(|&&d| (0.5..=2.0).contains(&d)).count();
    let is_sequential = consistent_count as f64 / diffs.len() as f64 > 0.8;
    if !is_sequential {
        return false;
    }

    if check_order {
        let ascending_count = nums.windows(2).filter(|w| w[0] <= w[1]).count();
        let n_pairs = nums.len() - 1;
        let is_ascending = ascending_count as f64 / n_pairs as f64 > 0.7;
        return is_ascending;
    }

    is_sequential
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uuid_format_canonical_lowercase() {
        assert!(is_uuid_format("550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn uuid_format_uppercase() {
        assert!(is_uuid_format("550E8400-E29B-41D4-A716-446655440000"));
    }

    #[test]
    fn uuid_format_wrong_length_rejected() {
        assert!(!is_uuid_format("550e8400-e29b-41d4-a716-44665544000"));
        assert!(!is_uuid_format("550e8400-e29b-41d4-a716-4466554400000"));
    }

    #[test]
    fn uuid_format_wrong_segment_count() {
        assert!(!is_uuid_format("550e8400e29b41d4a716446655440000"));
    }

    #[test]
    fn uuid_format_non_hex_rejected() {
        assert!(!is_uuid_format("550e8400-e29b-41d4-a716-44665544000z"));
    }

    #[test]
    fn uuid_format_empty_rejected() {
        assert!(!is_uuid_format(""));
    }

    #[test]
    fn entropy_empty_string_is_zero() {
        assert_eq!(calculate_string_entropy(""), 0.0);
    }

    #[test]
    fn entropy_single_char_is_zero() {
        assert_eq!(calculate_string_entropy("a"), 0.0);
    }

    #[test]
    fn entropy_all_same_chars_is_zero() {
        assert_eq!(calculate_string_entropy("aaaa"), 0.0);
    }

    #[test]
    fn entropy_perfectly_uniform_normalized_to_one() {
        let e = calculate_string_entropy("ab");
        assert!((e - 1.0).abs() < 1e-9);
    }

    #[test]
    fn entropy_mostly_repeated_low() {
        let e = calculate_string_entropy("aaaaaab");
        assert!(e < 0.7);
    }

    #[test]
    fn entropy_high_for_random_looking_string() {
        let e = calculate_string_entropy("a3f7b2c9d8e1f4a7");
        assert!(e > 0.7);
    }

    #[test]
    fn sequential_simple_int_ascending() {
        let v: Vec<Value> = (1..=10).map(|i| json!(i)).collect();
        assert!(detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_too_few_values() {
        let v = vec![json!(1), json!(2), json!(3)];
        assert!(!detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_random_numbers_not_detected() {
        let v: Vec<Value> = vec![
            json!(100),
            json!(2),
            json!(85),
            json!(7),
            json!(43),
            json!(17),
        ];
        assert!(!detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_descending_with_check_order_rejected() {
        let v: Vec<Value> = (1..=10).rev().map(|i| json!(i)).collect();
        assert!(!detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_descending_without_check_order_accepted() {
        let v: Vec<Value> = (1..=10).rev().map(|i| json!(i)).collect();
        assert!(detect_sequential_pattern(&v, false));
    }

    #[test]
    fn bug2_zero_padded_strings_no_longer_misclassified() {
        let v: Vec<Value> = (1..=10).map(|i| json!(format!("{:03}", i))).collect();
        assert!(
            !detect_sequential_pattern(&v, true),
            "BUG #2 fix: zero-padded string IDs must not be classified as sequential"
        );
    }

    #[test]
    fn bug2_mixed_string_and_int_still_detected() {
        let v = vec![json!(1), json!(2), json!("3"), json!(4), json!(5), json!(6)];
        assert!(detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_bools_excluded() {
        let v = vec![
            json!(true),
            json!(false),
            json!(true),
            json!(false),
            json!(true),
            json!(false),
        ];
        assert!(!detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_floats_with_unit_step() {
        let v: Vec<Value> = (1..=10).map(|i| json!(i as f64)).collect();
        assert!(detect_sequential_pattern(&v, true));
    }

    #[test]
    fn sequential_fractional_unit_step() {
        let v: Vec<Value> = vec![json!(1.5), json!(2.5), json!(3.5), json!(4.5), json!(5.5)];
        assert!(detect_sequential_pattern(&v, true));
    }

    #[test]
    fn bug2_all_unparseable_strings_returns_false() {
        let v: Vec<Value> = vec![
            json!("abc"),
            json!("def"),
            json!("ghi"),
            json!("jkl"),
            json!("mno"),
        ];
        assert!(!detect_sequential_pattern(&v, true));
    }

    #[test]
    fn bug2_single_int_among_strings_still_detects() {
        let v: Vec<Value> = vec![
            json!("001"),
            json!("002"),
            json!(3),
            json!("004"),
            json!("005"),
            json!("006"),
        ];
        assert!(detect_sequential_pattern(&v, true));
    }

    #[test]
    fn python_int_parse_basic() {
        assert_eq!(python_int_parse("5"), Some(5));
        assert_eq!(python_int_parse("-5"), Some(-5));
        assert_eq!(python_int_parse("+5"), Some(5));
    }

    #[test]
    fn python_int_parse_strips_whitespace() {
        assert_eq!(python_int_parse("  5  "), Some(5));
        assert_eq!(python_int_parse("\t-3\n"), Some(-3));
    }

    #[test]
    fn python_int_parse_underscores() {
        assert_eq!(python_int_parse("3_000"), Some(3000));
        assert_eq!(python_int_parse("1_000_000"), Some(1_000_000));
    }

    #[test]
    fn python_int_parse_underscore_edge_cases_rejected() {
        assert_eq!(python_int_parse("_5"), None);
        assert_eq!(python_int_parse("5_"), None);
        assert_eq!(python_int_parse("3__000"), None);
    }

    #[test]
    fn python_int_parse_rejects_floats() {
        assert_eq!(python_int_parse("3.14"), None);
    }

    #[test]
    fn python_int_parse_rejects_non_numeric() {
        assert_eq!(python_int_parse("abc"), None);
        assert_eq!(python_int_parse(""), None);
        assert_eq!(python_int_parse("   "), None);
    }

    #[test]
    fn sequential_with_whitespace_padded_strings_via_python_int_parse() {
        let v: Vec<Value> = vec![
            json!(1),
            json!("  2  "),
            json!(3),
            json!(" 4 "),
            json!(5),
            json!(6),
        ];
        assert!(detect_sequential_pattern(&v, true));
    }
}
