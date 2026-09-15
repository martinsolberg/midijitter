use alsa::Direction;

use crate::AppError;
use crate::backend::MidiSource;

use super::alsa_unavailable;

/// Lists ALSA RawMIDI input devices as `hw:card,device,subdevice` sources.
pub(super) fn rawmidi_sources() -> Result<Vec<MidiSource>, AppError> {
    let mut sources = Vec::new();
    for card in alsa::card::Iter::new() {
        let card = card.map_err(alsa_unavailable)?;
        let index = card.get_index();
        let card_name = card.get_name().unwrap_or_else(|_| format!("card-{index}"));
        let ctl = match alsa::Ctl::new(&format!("hw:{index}"), false) {
            Ok(ctl) => ctl,
            // A card that disappears mid-enumeration is skipped.
            Err(_) => continue,
        };
        for info in alsa::rawmidi::Iter::new(&ctl) {
            let info = info.map_err(alsa_unavailable)?;
            if info.get_stream() != Direction::Capture {
                continue;
            }
            let device = info.get_device();
            let subdevice = info.get_subdevice();
            let identifier = format!("hw:{index},{device},{subdevice}");
            let display_name = info
                .get_subdevice_name()
                .or_else(|_| info.get_id())
                .unwrap_or_else(|_| identifier.clone());
            sources.push(MidiSource {
                display_name,
                node_name: card_name.clone(),
                port_name: identifier.clone(),
                object_serial: None,
                node_id: index as u32,
                port_id: device as u32,
                alsa_device: Some(identifier),
            });
        }
    }

    sources.sort_by_key(MidiSource::stable_identity);
    Ok(sources)
}
