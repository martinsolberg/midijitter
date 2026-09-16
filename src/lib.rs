pub mod analysis;
pub mod backend;
pub mod capture;
pub mod cli;
pub mod error;
pub mod output;
pub mod plot;
pub mod simulate;

pub use analysis::{
    AnalysisOptions, AnalysisResult, AnalysisRow, AnomalyDetail, AnomalySummary, EventDisposition,
    ExclusionCounts, IntervalDisposition, PeriodStatistics, PhaseStatistics, RollingPoint,
    StartupSummary, analyze, rolling_bpm,
};
pub use capture::{
    AlsaTimestamp, CaptureFile, CapturedEvent, EnvironmentMetadata, GraphTransition, MidiEvent,
    PipeWireTimestamp, SourceMetadata, TimestampMetadata,
};
pub use error::AppError;
