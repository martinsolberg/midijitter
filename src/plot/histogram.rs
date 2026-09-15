use std::path::Path;

use plotters::prelude::*;

use crate::{AnalysisResult, AppError, CaptureFile};

use super::{drawing_root, padded_range, plot_error};

/// Histogram of phase errors in milliseconds.
pub fn render_phase_histogram(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
    path: &Path,
) -> Result<(), AppError> {
    let mut errors: Vec<f64> = analysis
        .rows
        .iter()
        .filter(|row| !row.duplicate && !row.anomalous)
        .map(|row| row.phase_error_ns / 1_000_000.0)
        .collect();
    if errors.is_empty() {
        return Err(AppError::Plot(
            "no usable clock events for the phase histogram".to_owned(),
        ));
    }
    errors.sort_by(f64::total_cmp);

    let bin_count = errors.len().div_ceil(5).clamp(10, 50);
    let (minimum, maximum) = (errors[0], errors[errors.len() - 1]);
    let width = if (maximum - minimum).abs() <= f64::EPSILON {
        1.0
    } else {
        (maximum - minimum) / bin_count as f64
    };
    let mut bins = vec![0_u32; bin_count];
    for error in &errors {
        let mut index = ((error - minimum) / width) as usize;
        if index >= bin_count {
            index = bin_count - 1;
        }
        bins[index] += 1;
    }

    let x_min = minimum - width * 0.5;
    let x_max = minimum + width * (bin_count as f64 - 0.5);
    let (x_min, x_max) = padded_range(x_min, x_max, width);
    let y_max = f64::from(*bins.iter().max().unwrap_or(&0));
    let y_max = if y_max <= f64::EPSILON {
        1.0
    } else {
        y_max * 1.05
    };

    let root = drawing_root(path)?;
    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Phase error histogram — {} @ {:.2} BPM",
                capture.source.display_name, analysis.measured_bpm
            ),
            ("sans-serif", 28).into_font(),
        )
        .margin(12)
        .x_label_area_size(44)
        .y_label_area_size(64)
        .build_cartesian_2d(x_min..x_max, 0.0..y_max)
        .map_err(plot_error)?;
    chart
        .configure_mesh()
        .x_desc("phase error (ms)")
        .y_desc("count")
        .draw()
        .map_err(plot_error)?;
    chart
        .draw_series(bins.iter().enumerate().map(|(index, count)| {
            let x0 = minimum + width * (index as f64 - 0.5);
            let x1 = minimum + width * (index as f64 + 0.5);
            Rectangle::new([(x0, 0.0), (x1, f64::from(*count))], BLUE.filled())
        }))
        .map_err(plot_error)?;
    root.present().map_err(plot_error)?;
    Ok(())
}
