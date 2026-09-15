use super::MidiEvent;

/// Extracts capture-relevant MIDI realtime events from a byte stream.
#[derive(Default)]
pub struct MidiParser {
    pending_channel_data_bytes: u8,
}

impl MidiParser {
    /// Processes one MIDI byte, returning recognized realtime events immediately.
    pub fn push(&mut self, byte: u8) -> Option<MidiEvent> {
        if (0xf8..=0xff).contains(&byte) {
            return match byte {
                0xf8 => Some(MidiEvent::Clock),
                0xfa => Some(MidiEvent::Start),
                0xfb => Some(MidiEvent::Continue),
                0xfc => Some(MidiEvent::Stop),
                0xfe => Some(MidiEvent::ActiveSensing),
                _ => None,
            };
        }

        match byte {
            0x80..=0xbf | 0xe0..=0xef => self.pending_channel_data_bytes = 2,
            0xc0..=0xdf => self.pending_channel_data_bytes = 1,
            0x00..=0x7f => {
                self.pending_channel_data_bytes = self.pending_channel_data_bytes.saturating_sub(1);
            }
            _ => self.pending_channel_data_bytes = 0,
        }

        None
    }
}
