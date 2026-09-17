mod common;
mod enumerate;
mod filter_capture;
pub mod timing;

use crate::AppError;
use crate::CaptureFile;
use crate::PairedCapture;
use crate::backend::{CaptureBackend, CaptureRequest, MidiSource, PairedCaptureRequest};

#[derive(Debug, Default, Clone, Copy)]
pub struct PipeWireBackend;

impl CaptureBackend for PipeWireBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError> {
        enumerate::midi_sources()
    }

    fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError> {
        filter_capture::run_filter_capture(&request)
    }
}

impl PipeWireBackend {
    pub fn record_paired(&self, request: PairedCaptureRequest) -> Result<PairedCapture, AppError> {
        filter_capture::run_paired_filter_capture(&request)
    }
}
