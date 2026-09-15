use super::common::{
    CaptureState, STOP_NEGOTIATION_FAILURE, STOP_NONE, STOP_SOURCE_GONE, STOP_STREAM_ERROR,
    STOP_UNSUPPORTED_FORMAT, event_capacity, finish_capture, pipewire_unavailable, record_cycle,
    spa_sequence_from_bytes,
};
use crate::AppError;
use crate::backend::CaptureRequest;
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::buffer::meta::MetaControl;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::pod::{Pod, Property, Value};
use pw::spa::utils::{Direction, Id, SpaTypes};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(super) fn record(request: CaptureRequest) -> Result<crate::CaptureFile, AppError> {
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

    let state = Rc::new(RefCell::new(CaptureState::new(
        request.termination.clone(),
        event_capacity,
    )));
    let process_state = Rc::clone(&state);
    let state_change_state = Rc::clone(&state);
    let _listener = stream
        .add_local_listener_with_user_data(())
        .state_changed(move |_, _, _, new| {
            let mut state = state_change_state.borrow_mut();
            match new {
                pw::stream::StreamState::Streaming => state.mark_streamed(),
                pw::stream::StreamState::Error(message) => {
                    state.set_stream_error(message);
                    state.request_stop(STOP_STREAM_ERROR);
                }
                pw::stream::StreamState::Unconnected if state.is_streamed() => {
                    state.request_stop(STOP_SOURCE_GONE)
                }
                _ => {}
            }
        })
        .process(move |stream, _| process_buffer(stream, &process_state))
        .register();

    let values = midi_control_format()?;
    let mut params = [
        Pod::from_bytes(&values).ok_or(AppError::PipeWireNegotiationFailed {
            detail: "could not construct SPA application/control format".to_owned(),
        })?,
    ];
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
    stream
        .connect(Direction::Input, target_id, flags, &mut params)
        .map_err(|error| AppError::PipeWireNegotiationFailed {
            detail: error.to_string(),
        })?;

    if request.manual_connect {
        println!("Sink: midijitter-capture:input_1  (use a patchbay to link your source)");
    }

    // Ctrl-C / SIGTERM set an async-signal-safe flag; the timer below
    // observes it on the main-loop thread and quits cleanly, so the process
    // callback itself never handles signals.
    let interrupted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, std::sync::Arc::clone(&interrupted))
            .map_err(|error| AppError::SignalHandler(error.to_string()))?;
    }

    let stop_loop = main_loop.clone();
    let stop_state = Rc::clone(&state);
    let interrupted_state = std::sync::Arc::clone(&interrupted);
    let timer = main_loop.loop_().add_timer(move |_| {
        if stop_state.borrow().stop_reason() != STOP_NONE
            || interrupted_state.load(Ordering::Acquire)
        {
            stop_loop.quit();
        }
    });
    timer.update_timer(
        Some(Duration::from_millis(1)),
        Some(Duration::from_millis(1)),
    );
    main_loop.run();

    let interrupted = interrupted.load(Ordering::Acquire);
    let mut state = state.borrow_mut();
    finish_capture(&mut state, interrupted, &request)
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

fn process_buffer(stream: &pw::stream::Stream, state: &Rc<RefCell<CaptureState>>) {
    let Ok(time) = stream.time() else {
        state.borrow().request_stop(STOP_NEGOTIATION_FAILURE);
        return;
    };
    let raw_time = time.as_raw();
    let Ok(cycle_position) = i64::try_from(raw_time.ticks) else {
        state.borrow().request_stop(STOP_NEGOTIATION_FAILURE);
        return;
    };
    let rate_num = raw_time.rate.num;
    let rate_denom = raw_time.rate.denom;
    let Ok(quantum) = u32::try_from(raw_time.size) else {
        state.borrow().request_stop(STOP_NEGOTIATION_FAILURE);
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
    let (sequence, _source) = if let Some(control) = buffer.find_meta::<MetaControl>() {
        (Some(control.sequence()), "meta")
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
        state.borrow().request_stop(STOP_UNSUPPORTED_FORMAT);
        return;
    };
    let mut state = state.borrow_mut();
    record_cycle(
        &mut state,
        sequence,
        cycle_position,
        rate_num,
        rate_denom,
        quantum,
    );
}
