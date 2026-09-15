pub mod histogram;
pub mod period;
pub mod phase;

use std::path::{Path, PathBuf};

use plotters::coord::Shift;
use plotters::prelude::*;

use crate::{AnalysisResult, AppError, CaptureFile};

pub const PHASE_PLOT_FILE: &str = "phase.png";
pub const PERIOD_PLOT_FILE: &str = "period.png";
pub const PHASE_HISTOGRAM_FILE: &str = "phase-histogram.png";

pub const PLOT_WIDTH: u32 = 1024;
pub const PLOT_HEIGHT: u32 = 768;

fn plot_error(error: impl std::fmt::Display) -> AppError {
    AppError::Plot(error.to_string())
}

fn drawing_root(path: &Path) -> Result<DrawingArea<BitMapBackend<'_>, Shift>, AppError> {
    let root = BitMapBackend::new(path, (PLOT_WIDTH, PLOT_HEIGHT)).into_drawing_area();
    root.fill(&WHITE).map_err(plot_error)?;
    Ok(root)
}

/// Expands a data range so degenerate (single-valued) inputs still render.
fn padded_range(minimum: f64, maximum: f64, fallback: f64) -> (f64, f64) {
    if (maximum - minimum).abs() <= f64::EPSILON {
        (minimum - fallback, maximum + fallback)
    } else {
        let padding = (maximum - minimum) * 0.05;
        (minimum - padding, maximum + padding)
    }
}

fn series_range(values: &[f64], fallback: f64) -> Result<(f64, f64), AppError> {
    let (Some(minimum), Some(maximum)) = (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
    ) else {
        return Err(AppError::Plot("not enough data points to plot".to_owned()));
    };
    Ok(padded_range(minimum, maximum, fallback))
}

/// Renders the three required v0.1 plots and returns the created files.
pub fn render_plots(
    capture: &CaptureFile,
    analysis: &AnalysisResult,
    output_dir: &Path,
) -> Result<Vec<PathBuf>, AppError> {
    std::fs::create_dir_all(output_dir).map_err(|error| AppError::FileWrite {
        path: output_dir.display().to_string(),
        message: error.to_string(),
    })?;

    let phase_path = output_dir.join(PHASE_PLOT_FILE);
    let period_path = output_dir.join(PERIOD_PLOT_FILE);
    let histogram_path = output_dir.join(PHASE_HISTOGRAM_FILE);

    phase::render_phase_plot(capture, analysis, &phase_path)?;
    period::render_period_plot(capture, analysis, &period_path)?;
    histogram::render_phase_histogram(capture, analysis, &histogram_path)?;

    Ok(vec![phase_path, period_path, histogram_path])
}
