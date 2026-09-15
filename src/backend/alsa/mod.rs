mod capture;
mod enumerate;
pub mod timing;

use crate::backend::{CaptureBackend, CaptureRequest, MidiSource};
use crate::{AppError, CaptureFile};

#[derive(Debug, Default, Clone, Copy)]
pub struct AlsaRawBackend;

impl CaptureBackend for AlsaRawBackend {
    fn enumerate(&self) -> Result<Vec<MidiSource>, AppError> {
        enumerate::rawmidi_sources()
    }

    fn record(&self, request: CaptureRequest) -> Result<CaptureFile, AppError> {
        capture::record(request)
    }
}

fn negative_errno(expected: libc::c_int) -> i32 {
    -expected
}

pub(super) fn alsa_unavailable(error: alsa::Error) -> AppError {
    AppError::AlsaUnavailable {
        detail: error.to_string(),
    }
}

/// Maps errors from opening or configuring a device.
pub(super) fn map_open_error(error: &alsa::Error, device: &str) -> AppError {
    let detail = format!("cannot open {device}: {error}");
    match error.errno() {
        errno if errno == negative_errno(libc::EBUSY) => AppError::AlsaBusy { detail },
        errno if errno == negative_errno(libc::EACCES) || errno == negative_errno(libc::EPERM) => {
            AppError::AlsaPermissionDenied { detail }
        }
        _ => AppError::AlsaUnavailable { detail },
    }
}

/// Maps errors while a capture is running; a vanished device is reported as
/// a disconnect rather than a generic failure.
pub(super) fn map_capture_error(error: &alsa::Error, device: &str) -> AppError {
    if error.errno() == negative_errno(libc::ENODEV) {
        return AppError::AlsaDisconnected;
    }
    map_open_error(error, device)
}

#[cfg(test)]
mod tests {
    use super::{map_capture_error, map_open_error};

    fn alsa_error(func: &'static str, errno: libc::c_int) -> alsa::Error {
        alsa::Error::new(func, -errno)
    }

    #[test]
    fn busy_devices_report_a_busy_error() {
        let error = map_open_error(&alsa_error("snd_rawmidi_open", libc::EBUSY), "hw:1,0,0");
        assert!(matches!(error, crate::AppError::AlsaBusy { .. }));
        assert_eq!(error.exit_code(), 3);
    }

    #[test]
    fn permission_errors_are_distinct() {
        for errno in [libc::EACCES, libc::EPERM] {
            let error = map_open_error(&alsa_error("snd_rawmidi_open", errno), "hw:1,0,0");
            assert!(
                matches!(error, crate::AppError::AlsaPermissionDenied { .. }),
                "errno {errno} should map to permission denied"
            );
            assert_eq!(error.exit_code(), 4);
        }
    }

    #[test]
    fn missing_devices_are_unavailable() {
        let error = map_open_error(&alsa_error("snd_rawmidi_open", libc::ENOENT), "hw:9,9,9");
        assert!(matches!(error, crate::AppError::AlsaUnavailable { .. }));
    }

    #[test]
    fn vanished_devices_report_a_disconnect() {
        let error = map_capture_error(&alsa_error("snd_rawmidi_tread", libc::ENODEV), "hw:1,0,0");
        assert!(matches!(error, crate::AppError::AlsaDisconnected));
    }
}
