use midijitter::capture::{MidiEvent, MidiParser};

#[test]
fn clock_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xf8), Some(MidiEvent::Clock));
    assert_eq!(parser.push(0x7f), None);
}

#[test]
fn start_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xfa), Some(MidiEvent::Start));
    assert_eq!(parser.push(0x7f), None);
}

#[test]
fn continue_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xfb), Some(MidiEvent::Continue));
    assert_eq!(parser.push(0x7f), None);
}

#[test]
fn stop_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xfc), Some(MidiEvent::Stop));
    assert_eq!(parser.push(0x7f), None);
}

#[test]
fn active_sensing_is_emitted_inside_a_channel_message() {
    let mut parser = MidiParser::default();
    assert_eq!(parser.push(0x90), None);
    assert_eq!(parser.push(0x3c), None);
    assert_eq!(parser.push(0xfe), Some(MidiEvent::ActiveSensing));
    assert_eq!(parser.push(0x7f), None);
}
