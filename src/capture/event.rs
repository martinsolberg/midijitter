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
