use super::common::{
    CaptureData, STOP_COMPLETE, STOP_NEGOTIATION_FAILURE, STOP_NONE, STOP_SOURCE_GONE,
    STOP_STREAM_ERROR, STOP_UNSUPPORTED_FORMAT, SharedControl, event_capacity, finish_capture,
    link_announcement_needed, pipewire_unavailable, record_cycle, spa_sequence_from_bytes,
};
use crate::backend::{CaptureRequest, CaptureTermination};
use crate::{AppError, CaptureFile};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::buffer::meta::MetaControl;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::pod::{Pod, Property, Value};
use pw::spa::utils::{Direction, Id, SpaTypes};
use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Grace added to `--duration` for the liveness bound that ends runs which
/// never produce callbacks (e.g. an unlinked manual-connect sink), instead of
/// hanging. Evaluated on the main-loop thread only; only applies while zero
/// events are captured, so it can never discard timestamped data.
const LIVENESS_GRACE: Duration = Duration::from_secs(5);

pub(super) fn record(request: CaptureRequest) -> Result<CaptureFile, AppError> {
    let event_capacity = event_capacity(&request.termination)?;
    pw::init();

    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_unavailable)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(pipewire_unavailable)?;
    let core = context.connect_rc(None).map_err(pipewire_unavailable)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "midijitter-capture",
        properties! {
            *pw::keys::NODE_NAME => "midijitter-capture",
            *pw::keys::MEDIA_TYPE => "Midi",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::FORMAT_DSP => "8 bit raw midi",
        },
    )
    .map_err(pipewire_unavailable)?;

    // Ctrl-C / SIGTERM set an async-signal-safe flag; the timer below
    // observes it on the main-loop thread and quits cleanly, so the process
    // callback itself never handles signals. Registered before any raw
    // allocation so `?` exits cannot leak it.
    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, std::sync::Arc::clone(&interrupted))
            .map_err(|error| AppError::SignalHandler(error.to_string()))?;
    }

    // Format negotiation happens before any raw allocation so `?` exits
    // cannot leak it.
    let values = midi_control_format()?;
    let mut params = [
        Pod::from_bytes(&values).ok_or(AppError::PipeWireNegotiationFailed {
            detail: "could not construct SPA application/control format".to_owned(),
        })?,
    ];

    // Capture data is owned exclusively by the realtime callback side while
    // the graph runs (see SAFETY below); the main-loop thread only touches
    // the Sync control block. This replaces the previous Rc<RefCell>
    // sharing, which could panic when the timer and the process callback
    // borrowed concurrently.
    let data_ptr: *mut CaptureData = Box::into_raw(Box::new(CaptureData::new(
        request.termination.clone(),
        event_capacity,
    )));
    let control = Arc::new(SharedControl::new());
    let state_change_control = Arc::clone(&control);
    let process_control = Arc::clone(&control);
    let listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, _, new| match new {
            pw::stream::StreamState::Streaming => state_change_control.mark_streamed(),
            pw::stream::StreamState::Error(message) => {
                state_change_control.set_error(message);
                state_change_control.request_stop(STOP_STREAM_ERROR);
            }
            pw::stream::StreamState::Unconnected if state_change_control.is_streamed() => {
                state_change_control.request_stop(STOP_SOURCE_GONE)
            }
            _ => {}
        })
        .process(move |stream, _| {
            // SAFETY: `data_ptr` is reclaimed only after the stream is
            // disconnected and the listener dropped below, so no callback
            // can be running when it is freed. The main-loop thread never
            // dereferences it.
            process_buffer(stream, data_ptr, &process_control);
        })
        .register()
        .map_err(|error| {
            // SAFETY: as in the connect-failure path below.
            drop(unsafe { Box::from_raw(data_ptr) });
            AppError::PipeWireNegotiationFailed {
                detail: error.to_string(),
            }
        })?;

    let (target_id, flags) = if request.manual_connect {
        (
            None,
            pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::RT_PROCESS,
        )
    } else {
        (
            Some(request.source.node_id),
            pw::stream::StreamFlags::AUTOCONNECT
                | pw::stream::StreamFlags::MAP_BUFFERS
                | pw::stream::StreamFlags::RT_PROCESS,
        )
    };
    if let Err(error) = stream.connect(Direction::Input, target_id, flags, &mut params) {
        teardown(&stream, listener);
        // SAFETY: callbacks never ran (connect failed before any could
        // fire), so reclaiming is exclusive. See the process closure above.
        drop(unsafe { Box::from_raw(data_ptr) });
        return Err(AppError::PipeWireNegotiationFailed {
            detail: error.to_string(),
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
    teardown(&stream, listener);

    let interrupted = interrupted.load(Ordering::Acquire);
    // SAFETY: same contract as the process closure above; the stream is
    // disconnected and the listener dropped, so this is now exclusive.
    let mut data = unsafe { Box::from_raw(data_ptr) };
    finish_capture(&mut data, &control, interrupted, &request)
}

/// Stops callbacks synchronously, then unregisters them. Must precede any
/// reclamation of the realtime-owned capture data.
fn teardown(stream: &pw::stream::Stream, listener: pw::stream::StreamListener<()>) {
    let _ = stream.disconnect();
    drop(listener);
}

fn midi_control_format() -> Result<Vec<u8>, AppError> {
    let object = spa::pod::Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: vec![
            Property::new(
                FormatProperties::MediaType.0,
                Value::Id(Id(MediaType::Application.as_raw())),
            ),
            Property::new(
                FormatProperties::MediaSubtype.0,
                Value::Id(Id(MediaSubtype::Control.as_raw())),
            ),
            Property::new(
                spa::sys::SPA_FORMAT_CONTROL_types,
                Value::Int(1 << spa::sys::SPA_CONTROL_Midi),
            ),
        ],
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|serialized| serialized.0.into_inner())
    .map_err(|error| AppError::PipeWireNegotiationFailed {
        detail: error.to_string(),
    })
}

fn process_buffer(stream: &pw::stream::Stream, data: *mut CaptureData, control: &SharedControl) {
    let Ok(time) = stream.time() else {
        control.request_stop(STOP_NEGOTIATION_FAILURE);
        return;
    };
    let raw_time = time.as_raw();
    let Ok(cycle_position) = i64::try_from(raw_time.ticks) else {
        control.request_stop(STOP_NEGOTIATION_FAILURE);
        return;
    };
    let rate_num = raw_time.rate.num;
    let rate_denom = raw_time.rate.denom;
    let Ok(quantum) = u32::try_from(raw_time.size) else {
        control.request_stop(STOP_NEGOTIATION_FAILURE);
        return;
    };
    // Before a link is active, the process callback may be invoked with
    // a zeroed graph position; just wait for a real cycle.
    if rate_num == 0 || rate_denom == 0 || quantum == 0 {
        return;
    }

    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let (sequence, _source) = if let Some(control_meta) = buffer.find_meta::<MetaControl>() {
        (Some(control_meta.sequence()), "meta")
    } else {
        (
            buffer
                .datas_mut()
                .first_mut()
                .and_then(|data| data.data())
                .and_then(|bytes| spa_sequence_from_bytes(bytes)),
            "data",
        )
    };
    let Some(sequence) = sequence else {
        control.request_stop(STOP_UNSUPPORTED_FORMAT);
        return;
    };
    // SAFETY: see the process closure above; the main thread never touches
    // this pointer while callbacks can run.
    let data = unsafe { &mut *data };
    record_cycle(
        data,
        control,
        sequence,
        cycle_position,
        rate_num,
        rate_denom,
        quantum,
    );
}
