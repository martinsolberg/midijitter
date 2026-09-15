use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unsupported capture format version {0}")]
    UnsupportedCaptureFormat(u32),

    #[error("invalid capture: {0}")]
    InvalidCapture(String),

    #[error("invalid capture JSON: {0}")]
    CaptureJson(#[from] serde_json::Error),

    #[error("cannot read capture file \"{path}\": {message}")]
    CaptureFileRead { path: String, message: String },

    #[error("cannot write file \"{path}\": {message}")]
    FileWrite { path: String, message: String },

    #[error("CSV output failed: {0}")]
    Csv(#[from] csv::Error),

    #[error("cannot install interrupt handler: {0}")]
    SignalHandler(String),

    #[error("no MIDI Clock messages were captured")]
    NoClockEvents,

    #[error("ALSA device unavailable: {detail}")]
    AlsaUnavailable { detail: String },

    #[error("ALSA device busy: {detail}")]
    AlsaBusy { detail: String },

    #[error("ALSA permission denied: {detail}")]
    AlsaPermissionDenied { detail: String },

    #[error("ALSA device disconnected during capture")]
    AlsaDisconnected,

    #[error(
        "ALSA timestamp mode unsupported; retry with --allow-userspace-timestamps \
         to use explicitly labeled userspace timestamping"
    )]
    AlsaTimestampUnsupported,

    #[error("plot rendering failed: {0}")]
    Plot(String),

    #[error("clock analysis did not converge")]
    AnalysisDidNotConverge,

    #[error("PipeWire graph-rate transitions are unsupported in v0.1")]
    GraphRateTransitionUnsupported,

    #[error("PipeWire timestamp arithmetic overflowed")]
    TimestampArithmeticOverflow,

    #[error("PipeWire capture buffer capacity was exhausted")]
    CaptureOverflow,

    #[error("PipeWire could not negotiate an application/control MIDI stream: {detail}")]
    PipeWireNegotiationFailed { detail: String },

    #[error("PipeWire source supplied an unsupported MIDI/control buffer format")]
    PipeWireUnsupportedControlFormat,

    #[error("PipeWire MIDI source disappeared during capture")]
    PipeWireSourceDisappeared,

    #[error("PipeWire daemon unavailable: {detail}")]
    PipeWireUnavailable { detail: String },

    #[error("PipeWire permission denied: {detail}")]
    PipeWirePermissionDenied { detail: String },

    #[error("no MIDI source is available")]
    NoSource,

    #[error("source selector \"{selector}\" is ambiguous; use one of: {}", candidates.join(", "))]
    AmbiguousSource {
        selector: String,
        candidates: Vec<String>,
    },

    #[error("MIDI source \"{selector}\" was not found")]
    SourceNotFound { selector: String },
}

impl AppError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::PipeWireUnavailable { .. }
            | Self::AlsaUnavailable { .. }
            | Self::AlsaBusy { .. }
            | Self::AlsaDisconnected => 3,
            Self::PipeWirePermissionDenied { .. } | Self::AlsaPermissionDenied { .. } => 4,
            Self::NoSource | Self::AmbiguousSource { .. } | Self::SourceNotFound { .. } => 5,
            _ => 1,
        }
    }
}
