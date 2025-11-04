# Rusty Pipes

## What is it?

Rusty Pipes is a digital organ instrument compatible with GrandOrgue sample sets. It features a TUI user interface and can be controlled via MIDI. Unlike GrandOrgue, Rusty Pipes streams samples from disk and does not load them into RAM.

## Compiling

```cargo build --release```

## Usage

Start Rusty Pipes with the path to a .organ file as parameter, then select a MIDI device to receive control input from. In the user interface, select one or more stops with the arrow keys and activate them with the space bar. Playing notes on your MIDI controller should play the selected organ stops.

