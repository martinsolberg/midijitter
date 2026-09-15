use std::f64::consts::PI;

use crate::{
    AppError, CaptureFile, CapturedEvent, EnvironmentMetadata, MidiEvent, PipeWireTimestamp,
    SourceMetadata, TimestampMetadata,
};

const SIMULATED_RATE_DENOM: u32 = 48_000;
const SIMULATED_QUANTUM: u32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulateConfig {
    pub bpm: f64,
    pub duration_s: u64,
    pub jitter_std_ns: f64,
    pub periodic_jitter_ns: f64,
    pub periodic_hz: f64,
    pub missing_rate: f64,
    pub duplicate_rate: f64,
    /// Relative period change per second, as a fraction (1 ppm/s = 1e-6).
    pub drift_per_second: f64,
    pub seed: u64,
}

/// Parses durations like `10ns`, `500us`, `1ms`, `2s`. A bare `0` is allowed.
pub fn parse_duration_ns(input: &str) -> Result<f64, AppError> {
    let error = || {
        AppError::InvalidCapture(format!(
            "invalid duration {input:?}; use a number with ns, us, ms or s (e.g. 1ms)"
        ))
    };
    if input == "0" {
        return Ok(0.0);
    }
    let split = input
        .find(|character: char| character.is_alphabetic())
        .ok_or_else(error)?;
    let (number, unit) = input.split_at(split);
    let value: f64 = number.parse().map_err(|_| error())?;
    if !value.is_finite() || value < 0.0 {
        return Err(error());
    }
    let multiplier = match unit {
        "ns" => 1.0,
        "us" => 1_000.0,
        "ms" => 1_000_000.0,
        "s" => 1_000_000_000.0,
        _ => return Err(error()),
    };
    Ok(value * multiplier)
}

/// Parses drift rates like `10ppm/s`. A bare `0` disables drift.
pub fn parse_drift_per_second(input: &str) -> Result<f64, AppError> {
    if input == "0" {
        return Ok(0.0);
    }
    let value: &str = input.strip_suffix("ppm/s").ok_or_else(|| {
        AppError::InvalidCapture(format!(
            "invalid drift {input:?}; use ppm/s (e.g. 10ppm/s) or 0"
        ))
    })?;
    let ppm: f64 = value.parse().map_err(|_| {
        AppError::InvalidCapture(format!(
            "invalid drift {input:?}; use ppm/s (e.g. 10ppm/s) or 0"
        ))
    })?;
    if !ppm.is_finite() {
        return Err(AppError::InvalidCapture(format!(
            "invalid drift {input:?}; use ppm/s (e.g. 10ppm/s) or 0"
        )));
    }
    Ok(ppm / 1_000_000.0)
}

/// Minimal deterministic PRNG (splitmix64) so simulation needs no dependency.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn next_unit(&mut self) -> f64 {
        const DIVISOR: f64 = (1_u64 << 53) as f64;
        ((self.next_u64() >> 11) as f64) / DIVISOR
    }

    /// Standard-normal sample via Box-Muller.
    fn gaussian(&mut self) -> f64 {
        let uniform1 = self.next_unit().max(f64::MIN_POSITIVE);
        let uniform2 = self.next_unit();
        (-2.0 * uniform1.ln()).sqrt() * (2.0 * PI * uniform2).cos()
    }
}

/// Generates a deterministic synthetic capture honoring every option.
///
/// The RNG is consumed in a fixed per-tick order (jitter, missing decision,
/// then duplicate decision unless the tick was dropped), so identical
/// configs always produce identical files.
pub fn generate(config: SimulateConfig) -> Result<CaptureFile, AppError> {
    if !config.bpm.is_finite() || config.bpm <= 0.0 {
        return Err(AppError::InvalidCapture(
            "simulate requires a positive BPM".to_owned(),
        ));
    }
    if config.duration_s == 0 {
        return Err(AppError::InvalidCapture(
            "simulate requires a positive duration".to_owned(),
        ));
    }
    for (name, rate) in [
        ("missing-rate", config.missing_rate),
        ("duplicate-rate", config.duplicate_rate),
    ] {
        if !rate.is_finite() || rate < 0.0 || rate > 1.0 {
            return Err(AppError::InvalidCapture(format!(
                "simulate {name} must be between 0 and 1"
            )));
        }
    }
    if !config.jitter_std_ns.is_finite() || config.jitter_std_ns < 0.0 {
        return Err(AppError::InvalidCapture(
            "simulate jitter-std must be non-negative".to_owned(),
        ));
    }
    if !config.periodic_jitter_ns.is_finite() || config.periodic_jitter_ns < 0.0 {
        return Err(AppError::InvalidCapture(
            "simulate periodic-jitter must be non-negative".to_owned(),
        ));
    }
    if !config.periodic_hz.is_finite() || config.periodic_hz < 0.0 {
        return Err(AppError::InvalidCapture(
            "simulate periodic-hz must be non-negative".to_owned(),
        ));
    }
    if !config.drift_per_second.is_finite() {
        return Err(AppError::InvalidCapture(
            "simulate drift must be finite".to_owned(),
        ));
    }

    let period_ns = 60_000_000_000.0 / (config.bpm * 24.0);
    let duration_ns = config.duration_s as f64 * 1_000_000_000.0;
    let tick_count = (duration_ns / period_ns).ceil() as u64;

    let mut rng = Rng(config.seed);
    let mut events = Vec::new();
    let mut base_ns: Option<f64> = None;
    let mut push_event = |timestamp_ns: f64, events: &mut Vec<CapturedEvent>| {
        let base = *base_ns.get_or_insert(timestamp_ns);
        let relative_ns = (timestamp_ns - base).round() as i128;
        let position =
            (timestamp_ns / 1_000_000_000.0 * f64::from(SIMULATED_RATE_DENOM)).round() as i64;
        events.push(CapturedEvent {
            sequence: events.len() as u64,
            timestamp_ns: relative_ns,
            event: MidiEvent::Clock,
            timestamp_metadata: TimestampMetadata::PipeWire(PipeWireTimestamp {
                cycle_position: position,
                event_offset: 0,
                event_position: position,
                rate_num: 1,
                rate_denom: SIMULATED_RATE_DENOM,
                quantum: SIMULATED_QUANTUM,
            }),
        });
    };

    for tick in 0..tick_count {
        let tick_time = tick as f64 * period_ns;
        // Integrated linear drift: period grows by `drift_per_second`
        // relative per second, so phase gains a quadratic term. Times are in
        // nanoseconds, hence the squared term is scaled back to seconds.
        let ideal = tick_time + config.drift_per_second * tick_time * tick_time / 2_000_000_000.0;
        let jitter = rng.gaussian() * config.jitter_std_ns
            + config.periodic_jitter_ns
                * (2.0 * PI * config.periodic_hz * ideal / 1_000_000_000.0).sin();
        if rng.next_unit() < config.missing_rate {
            continue;
        }
        push_event(ideal + jitter, &mut events);
        if rng.next_unit() < config.duplicate_rate {
            // A duplicate is a second clock 1 ms later, far below any real
            // period, so analysis flags it instead of indexing it as a tick.
            push_event(ideal + jitter + 1_000_000.0, &mut events);
        }
    }

    if events.is_empty() {
        return Err(AppError::NoClockEvents);
    }
    Ok(CaptureFile {
        format_version: crate::capture::CURRENT_FORMAT_VERSION,
        backend: "simulate".to_owned(),
        source: SourceMetadata {
            identity: "simulate".to_owned(),
            display_name: "Simulated MIDI clock".to_owned(),
        },
        timestamp_method: "synthetic (simulate command)".to_owned(),
        ppqn: 24,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        environment: EnvironmentMetadata {
            operating_system: std::env::consts::OS.to_owned(),
            pipewire_version: None,
        },
        transitions: Vec::new(),
        events,
    })
}

#[cfg(test)]
mod tests {
    use super::{SimulateConfig, generate, parse_drift_per_second, parse_duration_ns};

    fn default_config() -> SimulateConfig {
        SimulateConfig {
            bpm: 120.0,
            duration_s: 5,
            jitter_std_ns: 0.0,
            periodic_jitter_ns: 0.0,
            periodic_hz: 1.0,
            missing_rate: 0.0,
            duplicate_rate: 0.0,
            drift_per_second: 0.0,
            seed: 42,
        }
    }

    #[test]
    fn identical_configs_produce_identical_captures() {
        let first = generate(default_config()).unwrap();
        let second = generate(default_config()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn different_seeds_produce_different_captures() {
        let mut jittered = default_config();
        jittered.jitter_std_ns = 1_000_000.0;
        let first = generate(jittered).unwrap();
        jittered.seed = 43;
        let second = generate(jittered).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn perfect_simulation_has_zero_jitter() {
        let capture = generate(default_config()).unwrap();
        let analysis = crate::analyze(&capture, crate::AnalysisOptions::default()).unwrap();
        assert!((analysis.measured_bpm - 120.0).abs() < 0.001);
        assert!(analysis.phase.rms_ns < 1_000.0);
        assert!(analysis.period.rms_error_ns < 1_000.0);
    }

    #[test]
    fn missing_ticks_are_detected() {
        let mut config = default_config();
        config.missing_rate = 0.05;
        let capture = generate(config).unwrap();
        let analysis = crate::analyze(&capture, crate::AnalysisOptions::default()).unwrap();
        assert!(analysis.exclusions.inferred_missing_ticks > 0);
    }

    #[test]
    fn duplicate_ticks_are_flagged() {
        let mut config = default_config();
        config.duplicate_rate = 0.05;
        let capture = generate(config).unwrap();
        let analysis = crate::analyze(&capture, crate::AnalysisOptions::default()).unwrap();
        assert!(analysis.exclusions.duplicate_events + analysis.exclusions.anomalous_events > 0);
    }

    #[test]
    fn rejects_invalid_configs() {
        let mut config = default_config();
        config.bpm = 0.0;
        assert!(generate(config).is_err());
        let mut config = default_config();
        config.duration_s = 0;
        assert!(generate(config).is_err());
        let mut config = default_config();
        config.missing_rate = 1.5;
        assert!(generate(config).is_err());
    }

    #[test]
    fn parses_duration_units() {
        assert_eq!(parse_duration_ns("0").unwrap(), 0.0);
        assert_eq!(parse_duration_ns("10ns").unwrap(), 10.0);
        assert_eq!(parse_duration_ns("500us").unwrap(), 500_000.0);
        assert_eq!(parse_duration_ns("1ms").unwrap(), 1_000_000.0);
        assert_eq!(parse_duration_ns("2s").unwrap(), 2_000_000_000.0);
        assert!(parse_duration_ns("1").is_err());
        assert!(parse_duration_ns("1m").is_err());
        assert!(parse_duration_ns("-1ms").is_err());
    }

    #[test]
    fn parses_drift_rates() {
        assert_eq!(parse_drift_per_second("0").unwrap(), 0.0);
        assert_eq!(parse_drift_per_second("10ppm/s").unwrap(), 0.000_01);
        assert!(parse_drift_per_second("10").is_err());
        assert!(parse_drift_per_second("10ppb/s").is_err());
    }
}
