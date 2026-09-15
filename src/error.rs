use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("unsupported capture format version {0}")]
    UnsupportedCaptureFormat(u32),

    #[error("invalid capture: {0}")]
    InvalidCapture(String),

    #[error("invalid capture JSON: {0}")]
    CaptureJson(#[from] serde_json::Error),
}
