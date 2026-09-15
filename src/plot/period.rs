use std::path::Path;

use plotters::prelude::*;

use crate::{AnalysisResult, AppError, CaptureFile};

use super::{drawing_root, plot_error, series_range};

/// Inter-clock interval in milliseconds over capture time, with the fitted
/// period as reference. Only normal one-tick intervals are plotted so that
/// inferred missing ticks do not dominate the scale.
pub fn render_period_plot(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
    path: &Path,
) -> Result<(), AppError> {
    let usable: Vec<_> = analysis
        .rows
        .iter()
        .filter(|row| !row.duplicate && !row.anomalous)
        .collect();
    let mut points = Vec::new();
    for pair in usable.windows(2) {
        let (previous, current) = (pair[0], pair[1]);
        if current.tick_index != previous.tick_index + 1 {
            continue;
        }
        points.push((
            current.timestamp_ns as f64 / 1_000_000_000.0,
            (current.timestamp_ns - previous.timestamp_ns) as f64 / 1_000_000.0,
        ));
    }
    if points.is_empty() {
        return Err(AppError::Plot(
            "no normal one-tick intervals for the period plot".to_owned(),
        ));
    }

    let times: Vec<f64> = points.iter().map(|point| point.0).collect();
    let intervals: Vec<f64> = points.iter().map(|point| point.1).collect();
    let (x_min, x_max) = series_range(&times, 1.0)?;
    let (mut y_min, mut y_max) = series_range(&intervals, 1.0)?;
    let fitted_ms = analysis.fitted_period_ns / 1_000_000.0;
    y_min = y_min.min(fitted_ms);
    y_max = y_max.max(fitted_ms);

    let root = drawing_root(path)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Inter-clock interval — {} @ {:.2} BPM",
                capture.source.display_name, analysis.measured_bpm
            ),
            ("sans-serif", 28).into_font(),
        )
        .margin(12)
        .x_label_area_size(44)
        .y_label_area_size(64)
        .build_cartesian_2d(x_min..x_max, y_min..y_max)
        .map_err(plot_error)?;
    chart
        .configure_mesh()
        .x_desc("capture time (s)")
        .y_desc("F8 interval (ms)")
        .draw()
        .map_err(plot_error)?;
    chart
        .draw_series(LineSeries::new(
            vec![(x_min, fitted_ms), (x_max, fitted_ms)],
            &BLACK,
        ))
        .map_err(plot_error)?;
    chart
        .draw_series(LineSeries::new(points, &BLUE))
        .map_err(plot_error)?;
    root.present().map_err(plot_error)?;
    Ok(())
}
