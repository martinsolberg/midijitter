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
    StartupSummary, analyze, is_clean_period, is_clean_phase, rolling_bpm,
};
pub use capture::{
    AlsaTimestamp, CaptureCompletion, CaptureDocument, CaptureFile, CapturedEvent,
    CommonGraphMetadata, CompletionStatus, EnvironmentMetadata, GraphTiming, GraphTransition,
    GraphTransitionKind, MidiEvent, PAIRED_FORMAT_VERSION, PairedCapture, PairedEvent,
    PairedGraphTransition, PipeWireTimestamp, SourceMetadata, StreamRole, TimestampMetadata,
    common_timestamp_ns,
};
pub use error::AppError;
