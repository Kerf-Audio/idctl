# iD Mixer

A Linux control app for the iD2+ audio interface (2026 range, USB
`2708:0016`): the monitor controller, preamps, tone, the mixer and reverb.

This is an independent project. It is not made, endorsed or supported by
Audient. Audient and iD are trademarks of Audient Ltd.

## Features

- **Monitor:** volume, mute, dim, reference level, mono, M/S, talkback,
  input source and headphone level.
- **Preamps:** gain, +48V, pad, high-pass filter with adjustable cutoff.
- **Tone:** GLOW, HMX, LIFT and ORBIT (Punch, Smooth, Vintage), and
  V-PHASE on the mic inputs.
- **Channel DSP:** a four-band EQ (low shelf, two peaks, high shelf) with a
  draggable graph, and a compressor with threshold, ratio, attack, release,
  makeup and a gain-reduction meter. Swap the EQ/compressor order per
  channel. The DSP runs at sample rates up to 96 kHz.
- **Mixer:** 26 channels (2 mic, 8 optical, 14 DAW, reverb return) into
  Main, Mix A and Mix B, with level, pan, mute, solo, stereo link and meters.
- **Comfort Reverb:** a send on each channel, plus decay, damping and blend
  on the return strip.
- **Routing:** choose the source for each output pair.
- **Settings:** sample rate (through PipeWire), headphone level and source,
  and optical input and output mode (ADAT or S/PDIF).
- **Two-way:** changes made on the hardware show up in the app.

Not done yet: polarity, Record FX, SmartGain and clock source.

## Install

Build with a recent stable Rust:

```sh
cargo build --release
```

The binary is `target/release/idctl-gui`.

Let your user open the device without root by installing the udev rule:

```sh
sudo cp packaging/70-id2plus.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

Replug the interface after installing the rule.

## Layout

| Crate | Job |
|---|---|
| `idctl-protocol` | Parameter map and value encodings. No I/O. |
| `idctl-usb` | Vendor control transfers over `rusb`. |
| `idctl-gui` | The [iced](https://iced.rs) app. |

## Safety

The app writes to the interface only when you move a control. Take care
with +48V and with monitor and headphone levels.

## Licence

MIT. See [LICENSE](LICENSE).
