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
    PipeWire(PipeWireTimestamp),
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
