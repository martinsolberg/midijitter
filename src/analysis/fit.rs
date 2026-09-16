use crate::AppError;

use super::indexing::{EventDisposition, IndexedEvent};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Fit {
    pub intercept_ns: f64,
    pub period_ns: f64,
}

pub(crate) fn least_squares(rows: &[IndexedEvent<'_>]) -> Result<Fit, AppError> {
    let included: Vec<_> = rows
        .iter()
        .filter(|row| row.disposition == EventDisposition::Valid)
        .collect();
    if included.len() < 2 {
        return Err(AppError::InvalidCapture(
            "clock analysis requires two non-anomalous Clock events".to_owned(),
        ));
    }

    let count = included.len() as f64;
    let mean_x = included
        .iter()
        .map(|row| row.tick_index as f64)
        .sum::<f64>()
        / count;
    let mean_y = included
        .iter()
        .map(|row| row.event.timestamp_ns as f64)
        .sum::<f64>()
        / count;
    let (numerator, denominator) = included.iter().fold((0.0, 0.0), |(num, den), row| {
        let x = row.tick_index as f64 - mean_x;
        let y = row.event.timestamp_ns as f64 - mean_y;
        (num + x * y, den + x * x)
    });
    if denominator == 0.0 {
        return Err(AppError::InvalidCapture(
            "clock analysis requires distinct Clock tick indices".to_owned(),
        ));
    }
    let period_ns = numerator / denominator;
    Ok(Fit {
        intercept_ns: mean_y - period_ns * mean_x,
        period_ns,
    })
}
