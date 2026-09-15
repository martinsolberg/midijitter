use super::common::{
    CaptureData, STOP_COMPLETE, STOP_NONE, STOP_SOURCE_GONE, STOP_STREAM_ERROR,
    STOP_UNSUPPORTED_FORMAT, SharedControl, event_capacity, finish_capture,
    link_announcement_needed, pipewire_unavailable, record_cycle, spa_sequence_from_bytes,
};
use crate::backend::{CaptureRequest, CaptureTermination};
use crate::{AppError, CaptureFile};
use pipewire as pw;
use std::cell::Cell;
use std::ffi::{CStr, CString, c_void};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Grace added to `--duration` for the liveness bound that ends runs whose
/// filter never reaches STREAMING (no link, hence no process callbacks, so
/// stream-time termination can never fire). Evaluated on the main-loop
/// thread only; never touches event timestamps.
const LIVENESS_GRACE: Duration = Duration::from_secs(5);

pub(super) struct PositionTiming {
    pub cycle_ticks: i64,
    pub rate_num: u32,
    pub rate_denom: u32,
    pub quantum: u32,
}

pub(super) fn position_timing(
    position: &pw::spa::sys::spa_io_position,
) -> Result<PositionTiming, AppError> {
    let clock = &position.clock;
    let (rate_num, rate_denom) = (clock.rate.num, clock.rate.denom);
    let quantum =
        u32::try_from(clock.duration).map_err(|_| AppError::TimestampArithmeticOverflow)?;
    if rate_num == 0 || rate_denom == 0 || quantum == 0 {
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire filter reported an invalid graph rate or quantum".to_owned(),
        });
    }
    let cycle_ticks =
        i64::try_from(clock.position).map_err(|_| AppError::TimestampArithmeticOverflow)?;
    Ok(PositionTiming {
        cycle_ticks,
        rate_num,
        rate_denom,
        quantum,
    })
}

/// Handle shared with the realtime process callback. Capture data is owned
/// exclusively by the callback side while the graph runs; the main-loop
/// thread only touches the Sync control block. This replaces the previous
/// Rc<RefCell> sharing, which could panic when the timer and the process
/// callback borrowed concurrently.
struct FilterShared {
    data: *mut CaptureData,
    control: Arc<SharedControl>,
    port: Cell<*mut c_void>,
}

pub(super) fn run_filter_capture(request: &CaptureRequest) -> Result<CaptureFile, AppError> {
    let capacity = event_capacity(&request.termination)?;
    pw::init();

    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_unavailable)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(pipewire_unavailable)?;
    let _core = context.connect_rc(None).map_err(pipewire_unavailable)?;
    let raw_loop = unsafe { pw::sys::pw_main_loop_get_loop(main_loop.as_raw_ptr()) };

    // Ctrl-C / SIGTERM set an async-signal-safe flag; the timer below
    // observes it on the main-loop thread and quits cleanly, so the process
    // callback itself never handles signals. Registered before any raw
    // allocation so `?` exits cannot leak it.
    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, std::sync::Arc::clone(&interrupted))
            .map_err(|error| AppError::SignalHandler(error.to_string()))?;
    }

    let data_ptr: *mut CaptureData = Box::into_raw(Box::new(CaptureData::new(
        request.termination.clone(),
        capacity,
    )));
    let control = Arc::new(SharedControl::new());
    let shared = Box::into_raw(Box::new(FilterShared {
        data: data_ptr,
        control: Arc::clone(&control),
        port: Cell::new(ptr::null_mut()),
    }));

    let filter_props = unsafe { pw::sys::pw_properties_new(ptr::null()) };
    if filter_props.is_null() {
        abandon(data_ptr, shared);
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "could not allocate PipeWire filter properties".to_owned(),
        });
    }
    set_str(filter_props, "media.type", "Midi");
    set_str(filter_props, "media.category", "Capture");
    set_str(filter_props, "media.role", "Music");
    set_str(filter_props, "node.name", "midijitter-capture");

    let name = CString::new("midijitter-capture").map_err(|_| {
        abandon(data_ptr, shared);
        AppError::PipeWireNegotiationFailed {
            detail: "invalid filter node name".to_owned(),
        }
    })?;
    // SAFETY: `events` lives on this stack frame until after
    // `pw_filter_destroy` below, so the filter never outlives it.
    let mut events: pw::sys::pw_filter_events = unsafe { std::mem::zeroed() };
    events.version = pw::sys::PW_VERSION_FILTER_EVENTS;
    events.process = Some(on_process);
    events.state_changed = Some(on_state_changed);
    let filter = unsafe {
        pw::sys::pw_filter_new_simple(
            raw_loop,
            name.as_ptr(),
            filter_props,
            &events,
            shared as *mut c_void,
        )
    };
    if filter.is_null() {
        abandon(data_ptr, shared);
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire refused to create the MIDI filter".to_owned(),
        });
    }

    let port_props = unsafe { pw::sys::pw_properties_new(ptr::null()) };
    if port_props.is_null() {
        destroy(filter, shared);
        drop(unsafe { Box::from_raw(data_ptr) });
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "could not allocate PipeWire filter port properties".to_owned(),
        });
    }
    set_str(port_props, "format.dsp", "8 bit raw midi");
    set_str(port_props, "port.name", "input_1");
    if !request.manual_connect {
        set_str(
            port_props,
            "target.object",
            &request.source.node_id.to_string(),
        );
    }
    let port = unsafe {
        pw::sys::pw_filter_add_port(
            filter,
            pw::sys::PW_DIRECTION_INPUT,
            pw::sys::pw_filter_port_flags_PW_FILTER_PORT_FLAG_MAP_BUFFERS,
            0,
            port_props,
            ptr::null_mut(),
            0,
        )
    };
    if port.is_null() {
        destroy(filter, shared);
        drop(unsafe { Box::from_raw(data_ptr) });
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire refused to add the MIDI filter port".to_owned(),
        });
    }
    unsafe { &*shared }.port.set(port);

    let connected = unsafe {
        pw::sys::pw_filter_connect(
            filter,
            pw::sys::pw_filter_flags_PW_FILTER_FLAG_RT_PROCESS,
            ptr::null_mut(),
            0,
        )
    };
    if connected != 0 {
        destroy(filter, shared);
        drop(unsafe { Box::from_raw(data_ptr) });
        return Err(AppError::PipeWireNegotiationFailed {
            detail: "PipeWire refused to connect the MIDI filter".to_owned(),
        });
    }

    if request.manual_connect {
        println!("Sink: midijitter-capture:input_1  (use a patchbay to link your source)");
    }

    let started = Instant::now();
    let liveness_bound = match request.termination {
        CaptureTermination::DurationSeconds(seconds) => {
            Some(Duration::from_secs(seconds) + LIVENESS_GRACE)
        }
        CaptureTermination::Ticks(_) => None,
    };
    let manual_connect = request.manual_connect;
    let announced = Cell::new(false);
    let stop_loop = main_loop.clone();
    let timer_control = Arc::clone(&control);
    let interrupted_state = std::sync::Arc::clone(&interrupted);
    let timer = main_loop.loop_().add_timer(move |_| {
        if timer_control.stop_reason() != STOP_NONE || interrupted_state.load(Ordering::Acquire) {
            stop_loop.quit();
            return;
        }
        if link_announcement_needed(manual_connect, timer_control.is_streamed(), announced.get()) {
            println!("Link active: recording started.");
            announced.set(true);
        }
        // Liveness bound for runs whose filter never streams (no link, hence
        // no process callbacks): end cleanly with NoClockEvents instead of
        // hanging. Only applies while nothing was captured, so it can never
        // discard timestamped data.
        if let Some(bound) = liveness_bound
            && timer_control.event_count() == 0
            && started.elapsed() >= bound
        {
            timer_control.request_stop(STOP_COMPLETE);
            stop_loop.quit();
        }
    });
    timer.update_timer(
        Some(Duration::from_millis(1)),
        Some(Duration::from_millis(1)),
    );
    main_loop.run();
    destroy(filter, shared);

    let interrupted = interrupted.load(Ordering::Acquire);
    // SAFETY: same contract as the process callback below; the filter is
    // disconnected and destroyed, so this is now exclusive.
    let mut data = unsafe { Box::from_raw(data_ptr) };
    finish_capture(&mut data, &control, interrupted, request)
}

fn set_str(props: *mut pw::sys::pw_properties, key: &str, value: &str) {
    let key = CString::new(key).expect("static property key");
    let value = CString::new(value).expect("property value");
    // SAFETY: `props` is a live properties object; key/value are copied.
    unsafe { pw::sys::pw_properties_set(props, key.as_ptr(), value.as_ptr()) };
}

/// Reclaims both raw boxes on setup-failure paths where no callback could
/// have fired yet, so reclamation is trivially exclusive.
fn abandon(data: *mut CaptureData, shared: *mut FilterShared) {
    drop(unsafe { Box::from_raw(data) });
    drop(unsafe { Box::from_raw(shared) });
}

fn destroy(filter: *mut pw::sys::pw_filter, shared: *mut FilterShared) {
    // SAFETY: both pointers are live; disconnect stops callbacks before
    // destroy, and the shared box is reclaimed exactly once. The capture
    // data box is reclaimed separately by the caller.
    unsafe {
        pw::sys::pw_filter_disconnect(filter);
        pw::sys::pw_filter_destroy(filter);
    }
    drop(unsafe { Box::from_raw(shared) });
}

unsafe extern "C" fn on_process(data: *mut c_void, position: *mut pw::spa::sys::spa_io_position) {
    if data.is_null() {
        return;
    }
    // SAFETY: `data` is the leaked `FilterShared`, reclaimed only after the
    // main loop (and thus all callbacks) has stopped.
    let shared = unsafe { &*(data as *const FilterShared) };
    let port = shared.port.get();
    if port.is_null() {
        return;
    }
    // SAFETY: RT-safe dequeue; the buffer is recycled below in all paths.
    let buffer = unsafe { pw::sys::pw_filter_dequeue_buffer(port) };
    if buffer.is_null() {
        return;
    }
    process_filter_buffer(buffer, position, shared);
    // SAFETY: every dequeued buffer is returned exactly once.
    unsafe { pw::sys::pw_filter_queue_buffer(port, buffer) };
}

fn process_filter_buffer(
    buffer: *mut pw::sys::pw_buffer,
    position: *mut pw::spa::sys::spa_io_position,
    shared: &FilterShared,
) {
    if position.is_null() {
        return;
    }
    // SAFETY: PipeWire guarantees a valid position for the callback duration.
    let position = unsafe { &*position };
    let Ok(timing) = position_timing(position) else {
        return;
    };
    // SAFETY: `buffer` is a live dequeued buffer, recycled by the caller.
    let sequence = unsafe { filter_sequence(buffer) };
    let Some(sequence) = sequence else {
        shared.control.request_stop(STOP_UNSUPPORTED_FORMAT);
        return;
    };
    // SAFETY: capture data is owned by the callback side while the graph
    // runs; the main-loop thread only touches the Sync control block.
    let data = unsafe { &mut *shared.data };
    record_cycle(
        data,
        &shared.control,
        sequence,
        timing.cycle_ticks,
        timing.rate_num,
        timing.rate_denom,
        timing.quantum,
    );
}

/// Locates the MIDI control sequence in a dequeued filter buffer: sized-checked
/// `SPA_META_Control` metadata first, raw data-memory fallback second.
unsafe fn filter_sequence<'a>(
    buffer: *mut pw::sys::pw_buffer,
) -> Option<&'a pw::spa::sys::spa_pod_sequence> {
    // SAFETY: `buffer` is a live dequeued buffer for the duration of this call.
    let spa_buffer = unsafe { (*buffer).buffer };
    if spa_buffer.is_null() {
        return None;
    }
    let meta = unsafe {
        pw::spa::sys::spa_buffer_find_meta_data(
            spa_buffer,
            pw::spa::sys::SPA_META_Control,
            std::mem::size_of::<pw::spa::sys::spa_meta_control>(),
        )
    };
    if !meta.is_null() {
        // SAFETY: `find_meta_data` verified at least `spa_meta_control` bytes.
        let control = unsafe { &*(meta as *const pw::spa::sys::spa_meta_control) };
        return Some(&control.sequence);
    }
    // SAFETY: all pointer arithmetic below is bounds-checked against maxsize.
    unsafe {
        let spa_ref = &*spa_buffer;
        if spa_ref.n_datas == 0 || spa_ref.datas.is_null() {
            return None;
        }
        let data = &*spa_ref.datas;
        if data.data.is_null() || data.chunk.is_null() || data.maxsize == 0 {
            return None;
        }
        let chunk = &*data.chunk;
        let maxsize = data.maxsize as usize;
        let offset = chunk.offset as usize % maxsize;
        let size = (chunk.size as usize).min(maxsize.saturating_sub(offset));
        let bytes = std::slice::from_raw_parts(data.data as *const u8, maxsize);
        spa_sequence_from_bytes(&bytes[offset..offset + size])
    }
}

unsafe extern "C" fn on_state_changed(
    data: *mut c_void,
    _old: pw::sys::pw_filter_state,
    state: pw::sys::pw_filter_state,
    error: *const std::os::raw::c_char,
) {
    if data.is_null() {
        return;
    }
    // SAFETY: same lifetime contract as `on_process`.
    let shared = unsafe { &*(data as *const FilterShared) };
    match state {
        pw::sys::pw_filter_state_PW_FILTER_STATE_STREAMING => {
            shared.control.mark_streamed();
        }
        pw::sys::pw_filter_state_PW_FILTER_STATE_ERROR => {
            let detail = if error.is_null() {
                "PipeWire filter entered the error state".to_owned()
            } else {
                // SAFETY: PipeWire provides a valid NUL-terminated message.
                unsafe { CStr::from_ptr(error).to_string_lossy().into_owned() }
            };
            shared.control.set_error(detail);
            shared.control.request_stop(STOP_STREAM_ERROR);
        }
        pw::sys::pw_filter_state_PW_FILTER_STATE_UNCONNECTED if shared.control.is_streamed() => {
            shared.control.request_stop(STOP_SOURCE_GONE);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::position_timing;
    use pipewire as pw;

    fn position(
        rate_num: u32,
        rate_denom: u32,
        ticks: u64,
        duration: u64,
    ) -> pw::spa::sys::spa_io_position {
        // SAFETY: zeroed plain-data bindings struct, fully populated below.
        let mut position: pw::spa::sys::spa_io_position = unsafe { std::mem::zeroed() };
        position.clock.rate.num = rate_num;
        position.clock.rate.denom = rate_denom;
        position.clock.position = ticks;
        position.clock.duration = duration;
        position
    }

    #[test]
    fn position_timing_extracts_ticks_rate_and_quantum() {
        let timing = position_timing(&position(1, 48000, 480000, 1024)).unwrap();
        assert_eq!(
            (
                timing.cycle_ticks,
                timing.rate_num,
                timing.rate_denom,
                timing.quantum
            ),
            (480000, 1, 48000, 1024)
        );
    }

    #[test]
    fn position_timing_rejects_zero_rate_or_quantum() {
        assert!(position_timing(&position(0, 48000, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 0, 1, 1024)).is_err());
        assert!(position_timing(&position(1, 48000, 1, 0)).is_err());
    }

    #[test]
    #[ignore = "needs a live PipeWire daemon"]
    fn filter_capture_constructs_and_tears_down() {
        use crate::backend::{CaptureRequest, CaptureTermination, MidiSource};

        let source = MidiSource {
            display_name: "Spike".to_owned(),
            node_name: "spike".to_owned(),
            port_name: "out".to_owned(),
            object_serial: None,
            node_id: 0,
            port_id: 0,
            alsa_device: None,
        };
        let request = CaptureRequest::new(source, CaptureTermination::DurationSeconds(2))
            .unwrap()
            .manual_connect();
        // No link exists, so no callbacks arrive; the liveness bound must end
        // the run cleanly with NoClockEvents instead of hanging.
        let result = super::run_filter_capture(&request);
        assert!(matches!(result, Err(crate::AppError::NoClockEvents)));
    }
}
