use serde::{Deserialize, Serialize};

use crate::AppError;

use super::{
    CaptureCompletion, CapturedEvent, CommonGraphMetadata, EnvironmentMetadata, GraphTransition,
    PairedGraphTransition, SourceMetadata, TimestampMetadata,
};

pub const CURRENT_FORMAT_VERSION: u32 = 1;
pub const PAIRED_FORMAT_VERSION: u32 = 2;

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

    pub fn read_from_file(path: &std::path::Path) -> Result<Self, AppError> {
        let input = std::fs::read_to_string(path).map_err(|error| AppError::CaptureFileRead {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        Self::from_json_str(&input)
    }

    pub fn write_to_file(&self, path: &std::path::Path) -> Result<(), AppError> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).map_err(|error| AppError::FileWrite {
            path: path.display().to_string(),
            message: error.to_string(),
        })
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

            match &event.timestamp_metadata {
                TimestampMetadata::PipeWire(timestamp) => {
                    validate_rate_denominator(timestamp.rate_denom)?;
                }
                // NOTE: a future timestamp variant must extend this match.
                TimestampMetadata::Alsa(_) => {}
            }
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StreamRole {
    Reference,
    Returned,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PairedCapture {
    pub format_version: u32,
    pub backend: String,
    pub reference: SourceMetadata,
    pub returned: SourceMetadata,
    pub timestamp_method: String,
    pub ppqn: u32,
    pub application_version: String,
    pub environment: EnvironmentMetadata,
    pub common_graph: CommonGraphMetadata,
    pub transitions: Vec<PairedGraphTransition>,
    pub completion: CaptureCompletion,
    pub reference_events: Vec<CapturedEvent>,
    pub returned_events: Vec<CapturedEvent>,
}

impl PairedCapture {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.format_version != PAIRED_FORMAT_VERSION {
            return Err(AppError::UnsupportedCaptureFormat(self.format_version));
        }
        self.common_graph.validate()?;
        if self.reference.identity.trim().is_empty() || self.returned.identity.trim().is_empty() {
            return Err(AppError::InvalidCapture(
                "paired source identities must not be empty".to_owned(),
            ));
        }
        if self.reference.identity == self.returned.identity {
            return Err(AppError::InvalidCapture(
                "paired source identities must be distinct".to_owned(),
            ));
        }
        if self.ppqn != 24 {
            return Err(AppError::InvalidCapture(
                "PPQN must be exactly 24".to_owned(),
            ));
        }
        for transition in &self.transitions {
            transition.timing.validate()?;
            if transition.clock_id != self.common_graph.clock_id {
                return Err(AppError::InconsistentCommonTimebase(
                    "graph transition clock differs from common clock".to_owned(),
                ));
            }
        }
        validate_paired_events(&self.reference_events, &self.common_graph)?;
        validate_paired_events(&self.returned_events, &self.common_graph)?;
        Ok(())
    }

    pub fn from_json_str(input: &str) -> Result<Self, AppError> {
        let capture: Self = serde_json::from_str(input)?;
        capture.validate()?;
        Ok(capture)
    }

    pub fn read_from_file(path: &std::path::Path) -> Result<Self, AppError> {
        let input = std::fs::read_to_string(path).map_err(|error| AppError::CaptureFileRead {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        Self::from_json_str(&input)
    }

    pub fn write_to_file(&self, path: &std::path::Path) -> Result<(), AppError> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json).map_err(|error| AppError::FileWrite {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    }
}

fn validate_paired_events(
    events: &[CapturedEvent],
    common: &CommonGraphMetadata,
) -> Result<(), AppError> {
    let mut previous_sequence = None;
    for event in events {
        if previous_sequence.is_some_and(|previous| event.sequence <= previous) {
            return Err(AppError::InvalidCapture(
                "paired event sequence numbers must be strictly increasing".to_owned(),
            ));
        }
        previous_sequence = Some(event.sequence);
        let TimestampMetadata::PipeWire(timestamp) = &event.timestamp_metadata else {
            return Err(AppError::InconsistentCommonTimebase(
                "paired events must use PipeWire timestamps".to_owned(),
            ));
        };
        if timestamp.rate_num != common.rate_num
            || timestamp.rate_denom != common.rate_denom
            || timestamp.quantum != common.quantum
        {
            return Err(AppError::InconsistentCommonTimebase(
                "event timing differs from common graph timing".to_owned(),
            ));
        }
        let expected = timestamp
            .event_position
            .checked_sub(common.origin_position)
            .ok_or(AppError::TimestampArithmeticOverflow)?;
        let expected_ns = i128::from(expected)
            .checked_mul(i128::from(common.rate_num))
            .and_then(|value| value.checked_mul(1_000_000_000))
            .map(|value| value / i128::from(common.rate_denom))
            .ok_or(AppError::TimestampArithmeticOverflow)?;
        if event.timestamp_ns != expected_ns {
            return Err(AppError::InconsistentCommonTimebase(
                "event timestamp is not relative to the common origin".to_owned(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureDocument {
    V1(CaptureFile),
    V2(PairedCapture),
}

impl CaptureDocument {
    pub fn from_json_str(input: &str) -> Result<Self, AppError> {
        let value: serde_json::Value = serde_json::from_str(input)?;
        let version = value
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| AppError::InvalidCapture("missing format_version".to_owned()))?;
        match version as u32 {
            CURRENT_FORMAT_VERSION => Ok(Self::V1(CaptureFile::from_json_str(input)?)),
            PAIRED_FORMAT_VERSION => Ok(Self::V2(PairedCapture::from_json_str(input)?)),
            other => Err(AppError::UnsupportedCaptureFormat(other)),
        }
    }

    pub fn read_from_file(path: &std::path::Path) -> Result<Self, AppError> {
        let input = std::fs::read_to_string(path).map_err(|error| AppError::CaptureFileRead {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;
        Self::from_json_str(&input)
    }
}

/// Convert a raw PipeWire graph event position into nanoseconds relative to
/// the paired capture's common origin.
pub fn common_timestamp_ns(
    event_position: i64,
    origin_position: i64,
    rate_num: u32,
    rate_denom: u32,
) -> Result<i128, AppError> {
    if rate_num == 0 || rate_denom == 0 {
        return Err(AppError::InconsistentCommonTimebase(
            "graph rate must be positive".to_owned(),
        ));
    }
    let delta = i128::from(event_position) - i128::from(origin_position);
    delta
        .checked_mul(i128::from(rate_num))
        .and_then(|value| value.checked_mul(1_000_000_000))
        .map(|value| value / i128::from(rate_denom))
        .ok_or(AppError::TimestampArithmeticOverflow)
}

fn validate_rate_denominator(rate_denom: u32) -> Result<(), AppError> {
    if rate_denom == 0 {
        return Err(AppError::InvalidCapture(
            "PipeWire rate denominator must be positive".to_owned(),
        ));
    }

    Ok(())
}
