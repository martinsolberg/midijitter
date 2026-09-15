//! Temporary spike (Task 3): does `target.object` auto-link a `pw_filter`
//! input port, and does the process callback receive a valid
//! `spa_io_position` clock? Deleted in Task 7.
//!
//! Usage: `cargo run --example filter_spike -- <source-port-id>`

use pipewire as pw;
use std::ffi::CString;
use std::os::raw::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};

static PORT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CALLS: AtomicBool = AtomicBool::new(false);
static CALL_COUNT: AtomicU64 = AtomicU64::new(0);
static SAW_CLOCK: AtomicBool = AtomicBool::new(false);
static PRINT_COUNT: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn on_process(_data: *mut c_void, position: *mut pw::spa::sys::spa_io_position) {
    CALLS.store(true, Ordering::Relaxed);
    let n = CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    if position.is_null() {
        return;
    }
    let clock = unsafe { &(*position).clock };
    if clock.rate.num != 0 && clock.rate.denom != 0 && clock.duration != 0 {
        SAW_CLOCK.store(true, Ordering::Relaxed);
        if PRINT_COUNT
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(100)
        {
            eprintln!(
                "process #{n}: rate={}/{} position={} duration={}",
                clock.rate.num, clock.rate.denom, clock.position, clock.duration
            );
        }
    }
    let port = PORT.load(Ordering::Relaxed);
    if !port.is_null() {
        let buffer = unsafe { pw::sys::pw_filter_dequeue_buffer(port) };
        if !buffer.is_null() {
            unsafe { pw::sys::pw_filter_queue_buffer(port, buffer) };
        }
    }
}

unsafe extern "C" fn on_state_changed(
    _data: *mut c_void,
    _old: pw::sys::pw_filter_state,
    new: pw::sys::pw_filter_state,
    _error: *const std::os::raw::c_char,
) {
    eprintln!("filter state -> {new}");
}

fn set(props: *mut pw::sys::pw_properties, key: &str, value: &str) {
    let key = CString::new(key).unwrap();
    let value = CString::new(value).unwrap();
    unsafe { pw::sys::pw_properties_set(props, key.as_ptr(), value.as_ptr()) };
}

fn main() {
    let target = std::env::args()
        .nth(1)
        .expect("usage: filter_spike <source-port-id>");
    pw::init();

    let main_loop = pw::main_loop::MainLoopRc::new(None).expect("main loop");
    let raw_loop = unsafe { pw::sys::pw_main_loop_get_loop(main_loop.as_raw_ptr()) };

    let props = unsafe { pw::sys::pw_properties_new(ptr::null()) };
    assert!(!props.is_null());
    set(props, "media.type", "Midi");
    set(props, "media.category", "Filter");
    set(props, "media.role", "DSP");
    set(props, "node.name", "midijitter-spike");

    let name = CString::new("midi-spike").unwrap();
    let mut events: pw::sys::pw_filter_events = unsafe { std::mem::zeroed() };
    events.version = pw::sys::PW_VERSION_FILTER_EVENTS;
    events.process = Some(on_process);
    events.state_changed = Some(on_state_changed);
    let filter = unsafe {
        pw::sys::pw_filter_new_simple(raw_loop, name.as_ptr(), props, &events, ptr::null_mut())
    };
    assert!(!filter.is_null());

    let port_props = unsafe { pw::sys::pw_properties_new(ptr::null()) };
    set(port_props, "format.dsp", "8 bit raw midi");
    set(port_props, "port.name", "input");
    if target != "manual" {
        set(port_props, "target.object", &target);
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
    assert!(!port.is_null());
    PORT.store(port, Ordering::Relaxed);

    let res = unsafe {
        pw::sys::pw_filter_connect(
            filter,
            pw::sys::pw_filter_flags_PW_FILTER_FLAG_RT_PROCESS,
            ptr::null_mut(),
            0,
        )
    };
    assert_eq!(res, 0, "pw_filter_connect failed");

    let stop_loop = main_loop.clone();
    let ticks = std::sync::Arc::new(AtomicU64::new(0));
    let timer = main_loop.loop_().add_timer(move |_| {
        let n = ticks.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 6 {
            stop_loop.quit();
        }
    });
    timer.update_timer(
        Some(std::time::Duration::from_secs(1)),
        Some(std::time::Duration::from_secs(1)),
    );
    main_loop.run();

    eprintln!(
        "VERDICT calls={} saw_clock={}",
        CALLS.load(Ordering::Relaxed),
        SAW_CLOCK.load(Ordering::Relaxed)
    );
    unsafe {
        pw::sys::pw_filter_disconnect(filter);
        pw::sys::pw_filter_destroy(filter);
    }
}
