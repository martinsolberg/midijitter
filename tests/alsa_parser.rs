//! ALSA-side MIDI byte-stream parsing: realtime messages arrive embedded in
//! arbitrary RawMIDI byte streams and must be extracted without disturbing
//! channel-message state. The shared parser owns this behavior; these tests
//! pin it through ALSA-shaped chunks, including realtime bytes interleaved
//! inside a multi-byte channel message split across reads.

use midijitter::capture::{MidiEvent, MidiParser};

fn feed(parser: &mut MidiParser, chunk: &[u8]) -> Vec<MidiEvent> {
    chunk.iter().filter_map(|byte| parser.push(*byte)).collect()
}

#[test]
fn clock_bytes_are_extracted_from_a_raw_stream() {
    let mut parser = MidiParser::default();
    assert_eq!(feed(&mut parser, &[0xf8]), vec![MidiEvent::Clock]);
}

#[test]
fn realtime_bytes_interrupt_a_channel_message_without_corrupting_it() {
    let mut parser = MidiParser::default();
    // Note-on status + first data byte, then an F8, then the second data
    // byte arriving in a later RawMIDI read.
    assert!(feed(&mut parser, &[0x90, 0x3c, 0xf8]).contains(&MidiEvent::Clock));
    assert!(feed(&mut parser, &[0x7f]).is_empty());
}

#[test]
fn transport_events_are_recognized_between_message_bytes() {
    let mut parser = MidiParser::default();
    let mut events = feed(&mut parser, &[0xb0, 0xfa, 0x07]);
    events.extend(feed(&mut parser, &[0xfb, 0x40]));
    events.extend(feed(&mut parser, &[0xfc]));
    assert_eq!(
        events,
        vec![MidiEvent::Start, MidiEvent::Continue, MidiEvent::Stop]
    );
}

#[test]
fn active_sensing_is_preserved_but_separate_from_clock() {
    let mut parser = MidiParser::default();
    let events = feed(&mut parser, &[0xfe, 0xf8, 0xfe]);
    assert_eq!(
        events,
        vec![
            MidiEvent::ActiveSensing,
            MidiEvent::Clock,
            MidiEvent::ActiveSensing
        ]
    );
}
