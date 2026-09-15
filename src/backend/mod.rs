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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PwApi {
    /// `pw_filter` capture, mirroring `pw-mididump`: timing comes from the
    /// `spa_io_position` passed into the process callback.
    #[default]
    Filter,
    /// `pw_stream` capture with `pw_stream_get_time` timing.
    Stream,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub source: MidiSource,
    pub termination: CaptureTermination,
    /// Permit the explicitly labeled `CLOCK_MONOTONIC_RAW` userspace fallback
    /// when timestamped ALSA RawMIDI reads are unavailable.
    pub allow_userspace_timestamps: bool,
    /// PipeWire only: create the capture stream without autoconnecting, so
    /// the user can link a MIDI source to the exposed input port manually.
    pub manual_connect: bool,
    /// PipeWire only: which native API the capture path uses.
    pub pw_api: PwApi,
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
                manual_connect: false,
                pw_api: PwApi::default(),
            }),
        }
    }

    pub fn allow_userspace_timestamps(mut self) -> Self {
        self.allow_userspace_timestamps = true;
        self
    }

    pub fn manual_connect(mut self) -> Self {
        self.manual_connect = true;
        self
    }

    pub fn pw_api(mut self, api: PwApi) -> Self {
        self.pw_api = api;
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
    use super::{CaptureRequest, CaptureTermination, MidiSource, PwApi};

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

    #[test]
    fn capture_request_defaults_to_filter_api() {
        let request =
            CaptureRequest::new(source(), CaptureTermination::DurationSeconds(1)).unwrap();
        assert_eq!(request.pw_api, PwApi::Filter);
    }

    #[test]
    fn capture_request_builder_selects_stream_api() {
        let request = CaptureRequest::new(source(), CaptureTermination::DurationSeconds(1))
            .unwrap()
            .pw_api(PwApi::Stream);
        assert_eq!(request.pw_api, PwApi::Stream);
    }

    #[test]
    fn manual_connect_composes_with_either_pipewire_api() {
        for api in [PwApi::Filter, PwApi::Stream] {
            let request = CaptureRequest::new(source(), CaptureTermination::DurationSeconds(1))
                .unwrap()
                .manual_connect()
                .pw_api(api);
            assert!(request.manual_connect);
            assert_eq!(request.pw_api, api);
        }
    }
}
