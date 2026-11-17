# Handover

State of D+ Control 0.2.0 (renamed from iD Mixer on 17 Nov 2026). Branch
`design/generic-ui` is merged into `main` and pushed; `main` and release
`v0.2.0` on GitHub are at `643fc8b`. Release `v0.1.0` and the old branches
are deleted from GitHub. Updated 17 Nov 2026.

## What it is

Linux control app (Rust, iced 0.14) for the Audient iD2+. Crates:
`idctl-protocol` (wire format, parameter map, DSP maths), `idctl-usb` (rusb
transport), `idctl-audio` (PipeWire graph: xruns, load, buffer, profiles),
`idctl-gui` (the app). MSRV 1.90. Public repo
`https://github.com/Kerf-Audio/idctl`, MIT, README with screenshots and a
known-issues list.

## Redesign and rename (17 Nov 2026)

After a legal discussion the app was changed so it no longer resembles the
vendor's: generic names, a new look, a new name.

- Names: Warm (was the vendor's tone feature), Tone A, Tone B, Input mode 1/2/3,
  Phase, Reverb, Auto gain. Routing uses "Pass-through" and "Computer outs".
  No vendor feature names or "iD" branding in code, docs, packaging or the
  release. Only the compatibility statement names the Audient iD2+.
- Look: blue-black palette, violet mics, amber optical, cyan computer, lime
  monitoring; flat keys, flat dial with dotted scale, slim fader with block
  meters (`theme.rs`, `widgets/knob.rs`, `widgets/fader.rs`). Controls dim when
  the interface is offline, so check screenshots with the device connected.
- Name D+ Control, icon `packaging/dplus-control.svg`, window class
  `dplus-control`, state folder `~/.local/state/dplus-control` (the old
  `id-mixer` folder is moved over on first start; tested in `state.rs`).
  Binary and crates stay `idctl-gui` and `idctl-*`; repo stays `idctl`.
- Old design archived: private repo `Kerf-Audio/idctl-old-design`, local branch
  `archive/old-design` (`03a410b`), WNAS `/volume2/Github Backups/idctl-old-design/`
  (old build, screenshots, git bundle), `~/Pictures/idctl-old-design/`. The old
  design is still in the public git history; history was not rewritten.
- Screenshots in `docs/images` were taken from the app itself with a throwaway
  patch (iced `window::screenshot`, not committed).

## Build, install, release

- `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --all`
- `packaging/install-local.sh` builds release and installs to `~/.local`.
- `packaging/build-release.sh` builds the Linux tarball in a Debian 12
  container (binary needs glibc 2.35), strips it and packs an allow-list
  (binary, desktop entry, icons, udev rule, `install.sh`, `uninstall.sh`,
  `INSTALL.txt`, LICENSE) into `dist/` with `SHA256SUMS`. Needs Docker on
  x86_64. Published as release `v0.2.0` (version from `Cargo.toml`).
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
  idctl-gui 59.

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
The release tarball's contents were checked but its installed binary was not
launched. The redesigned keys, meters and the Tone card were seen only in my
own screenshots, not yet in your hands on a long session. Copy and Paste profile were covered by tests only.

## Outside the repo

- Danny has emailed Audient about the project and reported a bug that the
  headphone light ring does not match the audio level or the software.
- The repo social preview (`docs/images/social-preview.png`, now D+ Control)
  and the GitHub profile avatar (`~/Pictures/fmw-github-avatar.png`) still need
  uploading by hand in GitHub settings.
- The email to Audient (sent earlier) used the old name iD Mixer; the repo and
  release are now D+ Control. A follow-up is Danny's call.

## Next steps

1. Walk the hardware checklist (Task 16) and the untested list above.
2. Wire the sweep into the page with Keep/Revert (Tasks 11, 12, 14, 15).
3. Capture the official app for the four missing features.
