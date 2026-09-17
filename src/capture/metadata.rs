use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SourceMetadata {
    pub identity: String,
    pub display_name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EnvironmentMetadata {
    pub operating_system: String,
    pub pipewire_version: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GraphTransition {
    pub event_sequence: u64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimestampMetadata {
    #[serde(rename = "pipewire")]
    PipeWire(PipeWireTimestamp),
    #[serde(rename = "alsa")]
    Alsa(AlsaTimestamp),
}

/// Raw ALSA RawMIDI timestamp for one captured event.
///
/// `absolute_ns` is the kernel (or userspace fallback) timestamp in
/// `CLOCK_MONOTONIC_RAW` nanoseconds. The normalized `timestamp_ns` on the
/// enclosing event stays relative to the first captured event.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AlsaTimestamp {
    pub absolute_ns: i128,
    pub clock: String,
    pub timestamped_read: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PipeWireTimestamp {
    pub cycle_position: i64,
    pub event_offset: u32,
    pub event_position: i64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

/// The graph clock and timing tuple shared by every stream in a paired
/// capture. `origin_position` is the sole epoch for both event arrays.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CommonGraphMetadata {
    pub clock_id: u64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
    pub origin_position: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GraphTiming {
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphTransitionKind {
    Initial,
    RateChanged,
    QuantumChanged,
    ClockChanged,
}

/// Typed v2 graph transition metadata. V1's `GraphTransition` remains a
/// separate type because its wire format is part of the compatibility API.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PairedGraphTransition {
    pub event_sequence: u64,
    pub kind: GraphTransitionKind,
    pub clock_id: u64,
    pub timing: GraphTiming,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CompletionStatus {
    Complete,
    Interrupted,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CaptureCompletion {
    pub status: CompletionStatus,
    pub reason: Option<String>,
}

impl CommonGraphMetadata {
    pub fn validate(&self) -> Result<(), crate::AppError> {
        if self.rate_num == 0 || self.rate_denom == 0 || self.quantum == 0 {
            return Err(crate::AppError::InconsistentCommonTimebase(
                "graph rate and quantum must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}

impl GraphTiming {
    pub fn validate(&self) -> Result<(), crate::AppError> {
        if self.rate_num == 0 || self.rate_denom == 0 || self.quantum == 0 {
            return Err(crate::AppError::InconsistentCommonTimebase(
                "graph transition rate and quantum must be positive".to_owned(),
            ));
        }
        Ok(())
    }
}
