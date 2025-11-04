use anyhow::{anyhow, Result};
use midir::{MidiInput, MidiInputConnection, Ignore};
use std::io::{stdin, stdout, Write};
use std::sync::mpsc::Sender;

use crate::app::{AppMessage, TuiMessage};

/// Formats any MIDI message as a readable string.
fn format_midi_message(message: &[u8]) -> String {
    let mut s = String::new();
    for (i, byte) in message.iter().enumerate() {
        s.push_str(&format!("0x{:02X}", byte));
        if i < message.len() - 1 {
            s.push(' ');
        }
    }

    // Add a basic interpretation
    match message.get(0) {
        Some(0x90..=0x9F) => s.push_str(" (Note On)"),
        Some(0x80..=0x8F) => s.push_str(" (Note Off)"),
        Some(0xB0..=0xBF) => s.push_str(" (Control Change)"),
        Some(0xE0..=0xEF) => s.push_str(" (Pitch Bend)"),
        _ => s.push_str(" (Other)"),
    }
    s
}

pub fn setup_midi_input(
    audio_tx: Sender<AppMessage>,
    tui_tx: Sender<TuiMessage>,
) -> Result<MidiInputConnection<()>> {
    let mut midi_in = MidiInput::new("grandorgue-rs-input")?;
    midi_in.ignore(Ignore::ActiveSense);

    let in_ports = midi_in.ports();
    let in_port = match in_ports.len() {
        0 => return Err(anyhow!("No MIDI input ports found!")),
        1 => {
            println!("Choosing the only available MIDI port: {}", midi_in.port_name(&in_ports[0])?);
            &in_ports[0]
        },
        _ => {
            println!("\nAvailable MIDI input ports:");
            for (i, p) in in_ports.iter().enumerate() {
                println!("{}: {}", i, midi_in.port_name(p)?);
            }
            print!("Please select port number: ");
            stdout().flush()?;
            let mut input = String::new();
            stdin().read_line(&mut input)?;
            let port_index: usize = input.trim().parse()?;
            in_ports.get(port_index).ok_or_else(|| anyhow!("Invalid port number"))?
        }
    };

    println!("Opening MIDI connection...");
    let port_name = midi_in.port_name(in_port)?;

    let connection = midi_in.connect(in_port, &port_name, move |_timestamp, message, _| {
        // 1. Log the formatted message to the TUI thread
        let log_msg = format_midi_message(message);
        // We don't want to panic if the TUI is gone, so we ignore the error
        let _ = tui_tx.send(TuiMessage::MidiLog(log_msg));
        
        // 2. Parse and send to Audio thread
        if message.len() >= 3 {
            match message[0] {
                0x90..=0x9F => { // Note On (channel 1-16)
                    let note = message[1];
                    let velocity = message[2];
                    audio_tx.send(AppMessage::NoteOn(note, velocity)).unwrap_or_else(|e| {
                        let _ = tui_tx.send(TuiMessage::Error(format!("Failed to send NoteOn: {}", e)));
                    });
                },
                0x80..=0x8F => { // Note Off (channel 1-16)
                    let note = message[1];
                    audio_tx.send(AppMessage::NoteOff(note)).unwrap_or_else(|e| {
                        let _ = tui_tx.send(TuiMessage::Error(format!("Failed to send NoteOff: {}", e)));
                    });
                },
                _ => {} // Ignore other messages
            }
        }
    }, ())
    .map_err(|e| anyhow!("Failed to connect to MIDI input: {}", e))?;
    
    Ok(connection)
}

