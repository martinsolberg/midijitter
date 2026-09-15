mod enumerate;
pub mod timing;

use crate::AppError;
use crate::backend::{CaptureBackend, MidiSource};

#[derive(Debug, Default, Clone, Copy)]
pub struct PipeWireBackend;

impl CaptureBackend for PipeWireBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError> {
        enumerate::midi_sources()
    }
}
