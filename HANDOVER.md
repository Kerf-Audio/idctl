# Handover

State of D+ Control at commit `03a410b` plus this handover (branch
`latency/page`; `main` and release `v0.1.0` on GitHub are at `03a410b`).
Updated 17 Nov 2026.

## What it is

Linux control app (Rust, iced 0.14) for the Audient iD2+. Crates:
`idctl-protocol` (wire format, parameter map, DSP maths), `idctl-usb` (rusb
transport), `idctl-audio` (PipeWire graph: xruns, load, buffer, profiles),
`idctl-gui` (the app). MSRV 1.90. Public repo
`https://github.com/Kerf-Audio/idctl`, MIT, README with screenshots and a
known-issues list.

## Build, install, release

- `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --all`
- `packaging/install-local.sh` builds release and installs to `~/.local`.
- `packaging/build-release.sh` builds the Linux tarball in a Debian 12
  container (binary needs glibc 2.35), strips it and packs an allow-list
  (binary, desktop entry, icons, udev rule, `install.sh`, `uninstall.sh`,
  `INSTALL.txt`, LICENSE) into `dist/` with `SHA256SUMS`. Needs Docker on
  x86_64. Published as release `v0.1.0`.
- CI: `.github/workflows/ci.yml` runs on pushes to `main` and passes. Still to
  do: `permissions: contents: read`, pin actions by SHA, optional audit job.

## Latency page (new, 17 Nov 2026)

Design `docs/superpowers/specs/2026-11-17-latency-page-design.md`, plan
`docs/superpowers/plans/2026-11-17-latency-page.md`, ledger (git-ignored)
`.superpowers/sdd/2026-11-17-latency-page/progress.md`.

- `idctl-audio`: `top` (pw-top parser, strict header, bounded rows), `graph`
  (pw-dump), `stats` (xruns and load split interface vs apps, listeners marked,
  one load per app), `tuning` (presets 32/64/128/256, validate, crash record),
  `pw` (force quantum, period and headroom through `pw-metadata` and `pw-cli`,
  `pw-top` through `stdbuf -oL`), `sysload` (smoothed CPU, memory), `sweep`
  (test-run state machine, not wired up), `profile` (copy and paste JSON).
- GUI `latency.rs` and `views/latency.rs`. Buffer keys Off/32/64/128/256. Off
  hands the buffer back. What was found is written to
  `$XDG_STATE_HOME/dplus-control/latency` before forcing, put back on Off, on close
  (record kept if the restore fails) and on the next start (background task).
  Presets are kept in `.../profile.json`. Copy and Paste profile use the
  clipboard.
- Both reviews (security, correctness) were acted on. Tests: idctl-audio 78,
  idctl-gui 57.

## Not done

Latency page:
- Plan Task 11 (Keep/Revert and conflicting-file check), rest of 12, 14 (silent
  sine stress test, record and round-trip tests) and 15 (system check). The
  sweep is unwired, so profile suggestions from a test run do not exist yet.
  When wiring it, add a timeout for `pw-top` dying mid-run.
- Task 16, the hardware walk-through: pops when the buffer changes, compare
  with `pw-top`, `kill -9` then restart recovery.
- Closing the window can wait for PipeWire for a few seconds if it is wedged.
- `views/latency.rs` and `latency.rs` are at the 500-line guideline.

Device:
- Polarity, record effects, auto gain, clock source. Blocked on protocol facts, not
  effort. `params.rs` has no polarity address; input params `0x08` and `0x0c`
  accept writes and could be it, but nothing observable says what they do.
  `record_source` (`0x0d`) has no known values. Clock ids are unknown (a wrong
  one can drop the clock). Auto gain has addresses and status values but not the
  trigger sequence the official app uses. Needs a capture of the official app,
  or a mic signal to test polarity with.
- The FN button is not assigned. No ramp or ceiling on monitor and headphone
  level writes. Dim key presses are ignored while TB is on.
- `update()` is one large match; `Message` carries `ParamAddr`.

## Untested

Tested on one iD2+, one machine (Arch, Hyprland, PipeWire 1.6.9). Not
exercised against the hardware since the review fixes: talkback restore,
transient-error retries, offline dimming, the +48V dialog, close handling.
The release tarball was installed into a temp folder but its binary was not
launched. Copy and Paste profile were covered by tests only.

## Outside the repo

- Danny has emailed Audient about the project and reported a bug that the
  headphone light ring does not match the audio level or the software.
- The repo social preview (`docs/images/social-preview.png`) and the GitHub
  profile avatar still need uploading by hand in GitHub settings.

## Next steps

1. Walk the hardware checklist (Task 16) and the untested list above.
2. Wire the sweep into the page with Keep/Revert (Tasks 11, 12, 14, 15).
3. Capture the official app for the four missing features.
