use std::path::Path;

use plotters::prelude::*;

use crate::analysis::is_clean_phase;
use crate::{AnalysisResult, AppError, CaptureFile};

use super::{drawing_root, plot_error, series_range};

/// Phase error in milliseconds over capture time, with a zero reference.
pub fn render_phase_plot(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
    path: &Path,
) -> Result<(), AppError> {
    let points: Vec<(f64, f64)> = analysis
        .rows
        .iter()
        .filter(|row| is_clean_phase(row))
        .map(|row| {
            (
                row.timestamp_ns as f64 / 1_000_000_000.0,
                row.phase_error_ns / 1_000_000.0,
            )
        })
        .collect();
    if points.is_empty() {
        return Err(AppError::Plot(
            "no usable clock events for the phase plot".to_owned(),
        ));
    }

    let times: Vec<f64> = points.iter().map(|point| point.0).collect();
    let errors: Vec<f64> = points.iter().map(|point| point.1).collect();
    let (x_min, x_max) = series_range(&times, 1.0)?;
    let (mut y_min, mut y_max) = series_range(&errors, 1.0)?;
    y_min = y_min.min(0.0);
    y_max = y_max.max(0.0);

    let root = drawing_root(path)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Phase error — {} @ {:.2} BPM",
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
        .y_desc("phase error (ms)")
        .draw()
        .map_err(plot_error)?;
    chart
        .draw_series(LineSeries::new(vec![(x_min, 0.0), (x_max, 0.0)], &BLACK))
        .map_err(plot_error)?;
    chart
        .draw_series(LineSeries::new(points, &RED))
        .map_err(plot_error)?;
    root.present().map_err(plot_error)?;
    Ok(())
}
