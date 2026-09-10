pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let sum: f64 = values.iter().sum();
    let m = sum / values.len() as f64;
    if m.is_finite() {
        Some(m)
    } else {
        None
    }
}

pub fn sample_variance(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let m = mean(values)?;
    let sum_sq_diff: f64 = values.iter().map(|v| (v - m).powi(2)).sum();
    let var = sum_sq_diff / (values.len() - 1) as f64;
    if var.is_finite() {
        Some(var)
    } else {
        None
    }
}

pub fn sample_stdev(values: &[f64]) -> Option<f64> {
    sample_variance(values).map(f64::sqrt)
}

pub fn format_g(x: f64) -> String {
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
    if x == 0.0 {
        return "0".to_string();
    }

    let abs = x.abs();
    let exp = abs.log10().floor() as i32;

    if !(-4..4).contains(&exp) {
        let s = format!("{:.3e}", x);
        normalize_scientific_exp(&s)
    } else {
        let digits_after = (3 - exp).max(0) as usize;
        let s = format!("{:.*}", digits_after, x);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    }
}

fn normalize_scientific_exp(s: &str) -> String {
    let Some(epos) = s.find('e') else {
        return s.to_string();
    };
    let (mantissa, rest) = s.split_at(epos);
    let exp_part = &rest[1..];
    let exp_num: i32 = exp_part.parse().unwrap_or(0);
    let mantissa_clean = if mantissa.contains('.') {
        mantissa
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        mantissa.to_string()
    };
    let sign = if exp_num >= 0 { "+" } else { "-" };
    format!("{}e{}{:02}", mantissa_clean, sign, exp_num.abs())
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n.is_multiple_of(2) {
        let lo = sorted[n / 2 - 1];
        let hi = sorted[n / 2];
        Some((lo + hi) / 2.0)
    } else {
        Some(sorted[n / 2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn mean_empty_is_none() {
        assert_eq!(mean(&[]), None);
    }

    #[test]
    fn mean_single() {
        assert!(approx_eq(mean(&[5.0]).unwrap(), 5.0));
    }

    #[test]
    fn mean_basic() {
        assert!(approx_eq(mean(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap(), 3.0));
    }

    #[test]
    fn sample_variance_too_few_values_is_none() {
        assert_eq!(sample_variance(&[]), None);
        assert_eq!(sample_variance(&[5.0]), None);
    }

    #[test]
    fn sample_variance_uses_n_minus_1_denominator() {
        let v = sample_variance(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert!(approx_eq(v, 2.5), "got {v}, expected 2.5");
    }

    #[test]
    fn sample_stdev_basic() {
        let s = sample_stdev(&[1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        assert!(approx_eq(s, 2.5_f64.sqrt()), "got {s}");
    }

    #[test]
    fn sample_variance_constant_values_is_zero() {
        let v = sample_variance(&[7.0, 7.0, 7.0]).unwrap();
        assert!(approx_eq(v, 0.0));
    }

    #[test]
    fn mean_non_finite_overflow_returns_none() {
        let huge = f64::MAX / 2.0;
        let nums = vec![huge, huge, huge, huge];
        assert_eq!(mean(&nums), None);
    }

    #[test]
    fn sample_variance_non_finite_returns_none() {
        let huge = 1e200;
        let v = sample_variance(&[huge, -huge]);
        assert_eq!(v, None);
    }

    #[test]
    fn sample_stdev_non_finite_returns_none() {
        let huge = 1e200;
        assert_eq!(sample_stdev(&[huge, -huge]), None);
    }

    #[test]
    fn median_empty_is_none() {
        assert_eq!(median(&[]), None);
    }

    #[test]
    fn median_odd_count() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn median_even_count_mean_of_middles() {
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
    }

    #[test]
    fn median_single_element() {
        assert_eq!(median(&[42.0]), Some(42.0));
    }

    #[test]
    fn format_g_zero_and_special() {
        assert_eq!(format_g(0.0), "0");
        assert_eq!(format_g(-0.0), "0");
        assert_eq!(format_g(f64::NAN), "nan");
        assert_eq!(format_g(f64::INFINITY), "inf");
        assert_eq!(format_g(f64::NEG_INFINITY), "-inf");
    }

    #[test]
    fn format_g_fixed_range() {
        assert_eq!(format_g(1.5), "1.5");
        assert_eq!(format_g(1.0), "1");
        assert_eq!(format_g(1234.0), "1234");
        assert_eq!(format_g(0.123456), "0.1235");
    }

    #[test]
    fn format_g_scientific_range() {
        assert_eq!(format_g(12345.678), "1.235e+04");
        assert_eq!(format_g(0.00001234), "1.234e-05");
    }

    #[test]
    fn format_g_negative() {
        assert_eq!(format_g(-1.5), "-1.5");
        assert_eq!(format_g(-12345.678), "-1.235e+04");
    }
}
