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
}
