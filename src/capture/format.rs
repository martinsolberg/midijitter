use serde::{Deserialize, Serialize};

use crate::AppError;

use super::{
    CapturedEvent, EnvironmentMetadata, GraphTransition, SourceMetadata, TimestampMetadata,
};

pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CaptureFile {
    pub format_version: u32,
    pub backend: String,
    pub source: SourceMetadata,
    pub timestamp_method: String,
    pub ppqn: u32,
    pub application_version: String,
    pub environment: EnvironmentMetadata,
    pub transitions: Vec<GraphTransition>,
    pub events: Vec<CapturedEvent>,
}

impl CaptureFile {
    pub fn from_json_str(input: &str) -> Result<Self, AppError> {
        let capture: Self = serde_json::from_str(input)?;
        capture.validate()?;
        Ok(capture)
    }

    pub fn validate(&self) -> Result<(), AppError> {
        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(AppError::UnsupportedCaptureFormat(self.format_version));
        }

        if self.source.identity.trim().is_empty() {
            return Err(AppError::InvalidCapture(
                "source identity must not be empty".to_owned(),
            ));
        }

        if self.ppqn != 24 {
            return Err(AppError::InvalidCapture(
                "PPQN must be exactly 24".to_owned(),
            ));
        }

        for transition in &self.transitions {
            validate_rate_denominator(transition.rate_denom)?;
        }

        let mut previous_sequence = None;
        for event in &self.events {
            if previous_sequence.is_some_and(|previous| event.sequence <= previous) {
                return Err(AppError::InvalidCapture(
                    "event sequence numbers must be strictly increasing".to_owned(),
                ));
            }
            previous_sequence = Some(event.sequence);

            let TimestampMetadata::PipeWire(timestamp) = &event.timestamp_metadata;
            validate_rate_denominator(timestamp.rate_denom)?;
        }

        Ok(())
    }
}

fn validate_rate_denominator(rate_denom: u32) -> Result<(), AppError> {
    if rate_denom == 0 {
        return Err(AppError::InvalidCapture(
            "PipeWire rate denominator must be positive".to_owned(),
        ));
    }

    Ok(())
}
