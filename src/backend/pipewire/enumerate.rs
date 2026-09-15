use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pipewire as pw;
use pw::types::ObjectType;

use crate::AppError;
use crate::backend::MidiSource;

pub(super) fn midi_sources() -> Result<Vec<MidiSource>, AppError> {
    pw::init();

    let main_loop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_unavailable)?;
    let context = pw::context::ContextRc::new(&main_loop, None).map_err(pipewire_unavailable)?;
    let core = context.connect_rc(None).map_err(pipewire_unavailable)?;
    let registry = core.get_registry_rc().map_err(pipewire_unavailable)?;

    let sources = Rc::new(RefCell::new(Vec::new()));
    let callback_sources = Rc::clone(&sources);
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.type_ != ObjectType::Port {
                return;
            }

            let Some(properties) = global.props.as_ref() else {
                return;
            };
            if properties.get("media.type") != Some("Midi")
                || properties.get("port.direction") != Some("out")
            {
                return;
            }

            let Some(node_id) = properties.get("node.id").and_then(|id| id.parse().ok()) else {
                return;
            };
            let node_name = properties
                .get("node.name")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("node-{node_id}"));
            let port_name = properties
                .get("port.name")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("port-{}", global.id));
            let display_name = properties
                .get("node.description")
                .or_else(|| properties.get("node.nick"))
                .unwrap_or(&node_name)
                .to_owned();

            callback_sources.borrow_mut().push(MidiSource {
                display_name,
                node_name,
                port_name,
                object_serial: properties.get("object.serial").map(str::to_owned),
                node_id,
                port_id: global.id,
            });
        })
        .register();

    let completed = Rc::new(Cell::new(false));
    let pending_sync = Rc::new(Cell::new(None));
    let remote_error = Rc::new(RefCell::new(None));
    let done_loop = main_loop.clone();
    let done_completed = Rc::clone(&completed);
    let done_pending_sync = Rc::clone(&pending_sync);
    let error_loop = main_loop.clone();
    let error_message = Rc::clone(&remote_error);
    let _core_listener = core
        .add_listener_local()
        .done(move |id, sequence| {
            if id == pw::core::PW_ID_CORE && done_pending_sync.get() == Some(sequence.seq()) {
                done_completed.set(true);
                done_loop.quit();
            }
        })
        .error(move |_id, _sequence, _result, message| {
            *error_message.borrow_mut() = Some(message.to_owned());
            error_loop.quit();
        })
        .register();

    pending_sync.set(Some(core.sync(0).map_err(pipewire_unavailable)?.seq()));
    main_loop.run();

    if let Some(message) = remote_error.borrow_mut().take() {
        return Err(pipewire_remote_error(message));
    }
    if !completed.get() {
        return Err(AppError::PipeWireUnavailable {
            detail: "PipeWire did not complete registry discovery".to_owned(),
        });
    }

    let mut sources = source_snapshot(&sources);
    sources.sort_by_key(MidiSource::stable_identity);
    Ok(sources)
}

fn source_snapshot(sources: &Rc<RefCell<Vec<MidiSource>>>) -> Vec<MidiSource> {
    sources.borrow().clone()
}

fn pipewire_unavailable(error: pw::Error) -> AppError {
    AppError::PipeWireUnavailable {
        detail: error.to_string(),
    }
}

fn pipewire_remote_error(message: String) -> AppError {
    if message.to_ascii_lowercase().contains("permission") {
        AppError::PipeWirePermissionDenied { detail: message }
    } else {
        AppError::PipeWireUnavailable { detail: message }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_snapshot_does_not_require_sole_rc_ownership() {
        let sources = Rc::new(RefCell::new(vec![MidiSource {
            display_name: "USB MIDI".to_owned(),
            node_name: "usb-midi".to_owned(),
            port_name: "out".to_owned(),
            object_serial: Some("101".to_owned()),
            node_id: 10,
            port_id: 20,
        }]));
        let retained_callback_reference = Rc::clone(&sources);

        let snapshot = source_snapshot(&sources);

        assert_eq!(snapshot, *retained_callback_reference.borrow());
    }
}
