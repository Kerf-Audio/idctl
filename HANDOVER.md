# Handover

State of iD Mixer at commit `9a527b621f` (branch `main`).

## What it is

Linux control app (Rust, iced 0.14) for the Audient iD2+. Crates:
`idctl-protocol` (wire format, parameter map, DSP maths), `idctl-usb` (rusb
transport), `idctl-gui` (the app). MSRV 1.90.

## Build and install

- `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --all`
- `packaging/install-local.sh` builds release and installs to `~/.local`
  (binary `idctl-gui`, desktop entry `id-mixer.desktop`, icon `id-mixer`).
- `packaging/70-id2plus.rules` lets a normal user open the device.
- CI: `.github/workflows/ci.yml` (fmt, clippy, test, MSRV). Never run on GitHub;
  its apt package list is a guess.

## How it hangs together

- `worker.rs`: the only code that touches USB. Ordered write queue, newest write
  per parameter wins, transient USB errors absorbed (5 strikes then reconnect),
  `Quit` flushes the queue (3 retries) and acknowledges whether it all landed.
  The `Transport` trait has a fake in the tests.
- `main.rs`: `App` / `update`. While offline the UI is dimmed and writes are
  dropped (`Message::writes_device`). +48V asks for confirmation first.
- `talkback.rs`: while TB is on the mic is sent at 0 dB to Mix A and B; the
  previous sends are restored on release, on close, and (from
  `$XDG_STATE_HOME/id-mixer/talkback`) on the next connect if the app died.
  Hand edits to that send during TB are adopted into the restore.
- The unit reports DIM itself during talkback; the Dim key stays dark then.
- `clock.rs`: sample rate by forcing the PipeWire graph rate with
  `pw-metadata`; released on exit if we forced it.

## Not done

- Polarity, Record FX, SmartGain, clock source. Blocked on protocol facts, not
  effort. `params.rs` has no polarity address; input params `0x08` and `0x0c`
  accept writes and could be it, but nothing observable says what they do.
  `record_source` (`0x0d`) has no known values. Clock ids are unknown (a wrong
  one can drop the clock). SmartGain has addresses and status values but not the
  trigger sequence the official app uses. Needs a capture of the official app,
  or a mic signal to test polarity with.
- No ramp or ceiling on monitor/headphone level writes (knobs stop at 0 dB).
- Dim key presses are ignored while TB is on.
- `update()` is one large match; `Message` carries `ParamAddr`.

## Untested

Nothing has been exercised against the physical iD2+ since the review fixes:
talkback restore (including from the state file), transient-error retries,
offline dimming, the +48V dialog, close handling, and the Dim-during-TB change.
Dim-during-TB is inferred from code and a user report.

## Next steps

1. Run the app against the hardware and walk through the list above.
2. Capture the official app for the four missing features.
3. Push and watch the first CI run; fix the apt list if it fails.
