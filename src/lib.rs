pub mod analysis;
pub mod backend;
pub mod capture;
pub mod cli;
pub mod error;

pub use analysis::{
    AnalysisOptions, AnalysisResult, AnalysisRow, ExclusionCounts, PeriodStatistics,
    PhaseStatistics, analyze,
};
pub use capture::{
    CaptureFile, CapturedEvent, EnvironmentMetadata, GraphTransition, MidiEvent, PipeWireTimestamp,
    SourceMetadata, TimestampMetadata,
};
pub use error::AppError;
