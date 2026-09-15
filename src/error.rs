use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unsupported capture format version {0}")]
    UnsupportedCaptureFormat(u32),

    #[error("invalid capture: {0}")]
    InvalidCapture(String),

    #[error("invalid capture JSON: {0}")]
    CaptureJson(#[from] serde_json::Error),

    #[error("clock analysis did not converge")]
    AnalysisDidNotConverge,

    #[error("PipeWire graph-rate transitions are unsupported in v0.1")]
    GraphRateTransitionUnsupported,

    #[error("PipeWire timestamp arithmetic overflowed")]
    TimestampArithmeticOverflow,

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
            Self::PipeWireUnavailable { .. } => 3,
            Self::PipeWirePermissionDenied { .. } => 4,
            Self::NoSource | Self::AmbiguousSource { .. } | Self::SourceNotFound { .. } => 5,
            _ => 1,
        }
    }
}
