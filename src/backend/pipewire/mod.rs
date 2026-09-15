mod capture;
mod enumerate;
pub mod timing;

use crate::AppError;
use crate::CaptureFile;
use crate::backend::{CaptureBackend, CaptureRequest, MidiSource};

#[derive(Debug, Default, Clone, Copy)]
pub struct PipeWireBackend;

impl CaptureBackend for PipeWireBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError> {
        enumerate::midi_sources()
    }

    fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError> {
        capture::record(request)
    }
}
