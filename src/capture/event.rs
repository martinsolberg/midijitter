use serde::{Deserialize, Serialize};

use super::TimestampMetadata;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MidiEvent {
    Clock,
    Start,
    Continue,
    Stop,
    ActiveSensing,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CapturedEvent {
    pub sequence: u64,
    pub timestamp_ns: i128,
    pub event: MidiEvent,
    pub timestamp_metadata: TimestampMetadata,
}

/// An event in one side of a v2 paired capture. The containing array supplies
/// the stream role; the raw graph position is retained in the metadata.
pub type PairedEvent = CapturedEvent;
