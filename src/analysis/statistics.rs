#[derive(Debug, Clone, Copy)]
pub(crate) struct Summary {
    pub mean: f64,
    pub standard_deviation: f64,
    pub rms: f64,
    pub mean_absolute: f64,
    pub median_absolute: f64,
    pub p95_absolute: f64,
    pub p99_absolute: f64,
    pub minimum: f64,
    pub maximum: f64,
}

pub(crate) fn summarize(values: &[f64]) -> Summary {
    debug_assert!(!values.is_empty());
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let standard_deviation = (values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / count)
        .sqrt();
    let rms = (values.iter().map(|value| value.powi(2)).sum::<f64>() / count).sqrt();
    let absolute: Vec<_> = values.iter().map(|value| value.abs()).collect();
    Summary {
        mean,
        standard_deviation,
        rms,
        mean_absolute: absolute.iter().sum::<f64>() / count,
        median_absolute: percentile(&absolute, 0.5),
        p95_absolute: percentile(&absolute, 0.95),
        p99_absolute: percentile(&absolute, 0.99),
        minimum: values
            .iter()
            .copied()
            .reduce(f64::min)
            .expect("non-empty values"),
        maximum: values
            .iter()
            .copied()
            .reduce(f64::max)
            .expect("non-empty values"),
    }
}

pub(crate) fn percentile(values: &[f64], percentile: f64) -> f64 {
    debug_assert!(!values.is_empty());
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((sorted.len() - 1) as f64 * percentile).round() as usize;
    sorted[rank]
}
