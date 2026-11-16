# idctl

A Linux control app for the Audient iD2+ (2026 range, USB `2708:0016`):
the monitor controller, preamps, tone, the mixer and Comfort Reverb.

This is an independent project. It is not made, endorsed or supported by
Audient. Audient and iD are trademarks of Audient Ltd.

## Features

- **Monitor:** volume, mute, dim, reference level, mono, M/S, talkback,
  input source and headphone level.
- **Preamps:** gain, +48V, pad, high-pass filter.
- **Tone:** GLOW, HMX, LIFT and ORBIT (Punch, Smooth, Vintage).
- **Mixer:** 26 channels (2 mic, 8 optical, 14 DAW, reverb return) into
  Main, Mix A and Mix B, with level, pan, mute, solo, stereo link and meters.
- **Settings:** optical input and output mode (ADAT or S/PDIF), Comfort
  Reverb size, damping and level.
- **Two-way:** changes made on the hardware show up in the app.

Not done yet: DSP EQ and compression, routing, SmartGain, and sample rate
or clock changes.

## Install

Build with a recent stable Rust:

```sh
cargo build --release
```

The binary is `target/release/idctl-gui`.

Let your user open the device without root by installing the udev rule:

```sh
sudo cp packaging/70-audient-id.rules /etc/udev/rules.d/
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
