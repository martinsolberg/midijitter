use crate::{AppError, CaptureFile};

pub mod alsa;
pub mod pipewire;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiSource {
    pub display_name: String,
    pub node_name: String,
    pub port_name: String,
    pub object_serial: Option<String>,
    pub node_id: u32,
    pub port_id: u32,
    /// ALSA RawMIDI device identifier (`hw:card,device,subdevice`); `None`
    /// for PipeWire sources.
    pub alsa_device: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTermination {
    DurationSeconds(u64),
    Ticks(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub source: MidiSource,
    pub termination: CaptureTermination,
    /// Permit the explicitly labeled `CLOCK_MONOTONIC_RAW` userspace fallback
    /// when timestamped ALSA RawMIDI reads are unavailable.
    pub allow_userspace_timestamps: bool,
}

impl CaptureRequest {
    pub fn new(source: MidiSource, termination: CaptureTermination) -> Result<Self, AppError> {
        match termination {
            CaptureTermination::DurationSeconds(0) | CaptureTermination::Ticks(0) => Err(
                AppError::InvalidCapture("capture termination must be positive".to_owned()),
            ),
            _ => Ok(Self {
                source,
                termination,
                allow_userspace_timestamps: false,
            }),
        }
    }

    pub fn allow_userspace_timestamps(mut self) -> Self {
        self.allow_userspace_timestamps = true;
        self
    }
}

impl MidiSource {
    pub fn stable_identity(&self) -> String {
        if let Some(device) = &self.alsa_device {
            return device.clone();
        }
        match &self.object_serial {
            Some(serial) => format!("{}/{}#{serial}", self.node_name, self.port_name),
            None => format!("{}/{}", self.node_name, self.port_name),
        }
    }
}

pub trait CaptureBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError>;
    fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError>;
}

pub fn select_source(sources: &[MidiSource], selector: &str) -> Result<MidiSource, AppError> {
    let matches: Vec<_> = sources
        .iter()
        .filter(|source| source.display_name == selector || source.stable_identity() == selector)
        .cloned()
        .collect();

    match matches.as_slice() {
        [] if sources.is_empty() => Err(AppError::NoSource),
        [] => Err(AppError::SourceNotFound {
            selector: selector.to_owned(),
        }),
        [source] => Ok(source.clone()),
        _ => Err(AppError::AmbiguousSource {
            selector: selector.to_owned(),
            candidates: matches.iter().map(MidiSource::stable_identity).collect(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureRequest, CaptureTermination, MidiSource};

    fn source() -> MidiSource {
        MidiSource {
            display_name: "Test source".to_owned(),
            node_name: "test-node".to_owned(),
            port_name: "out".to_owned(),
            object_serial: None,
            node_id: 1,
            port_id: 2,
            alsa_device: None,
        }
    }

    #[test]
    fn capture_request_has_one_positive_bounded_termination_condition() {
        assert!(CaptureRequest::new(source(), CaptureTermination::Ticks(1)).is_ok());
        assert!(CaptureRequest::new(source(), CaptureTermination::DurationSeconds(1)).is_ok());
        assert!(CaptureRequest::new(source(), CaptureTermination::Ticks(0)).is_err());
        assert!(CaptureRequest::new(source(), CaptureTermination::DurationSeconds(0)).is_err());
    }
}
