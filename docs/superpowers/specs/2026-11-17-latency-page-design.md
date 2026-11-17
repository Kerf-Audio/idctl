# Latency page: design

17 Nov 2026. Status: awaiting approval.

## Intent

People using an iD2+ on Linux want to run small buffers without clicks. Today
they have no way to see where a glitch comes from (the interface, the system or
the application), and tuning means hand-editing PipeWire and WirePlumber files
and guessing. iD Mixer gets a **Latency** page that shows what is happening,
lets the user change the buffer settings safely, and runs a repeatable test
that finds the lowest setting their machine holds cleanly.

It is for any iD Mixer user, not one machine. Everything it changes is
per-user, visible and revertible.

## Goals

1. **See it.** Live buffer size, sample rate, buffer latency in milliseconds,
   xruns, and how much of each period is used, split between the interface and
   each application playing through it.
2. **Change it.** Graph buffer size (quantum), and the interface's ALSA
   period size and headroom, from the page. One three-way switch (Safe,
   Balanced, Aggressive) with the raw values editable underneath. The tuning
   is a profile file each user can export, import and share.
3. **Test it.** A run that steps through settings, holds each for a fixed time,
   and reports xruns and period use per setting, then offers the lowest clean
   one.
4. **Explain the rest.** A read-only check of system settings that affect
   low-latency audio, each with a plain status and what to do about it.

## Non-goals

- Measuring true analogue round-trip latency with a loopback cable. The page
  shows buffer latency (what the settings add), labelled as such. A measured
  round trip can come later.
- Changing anything that needs root: kernel command line, IRQ priorities,
  limits files, CPU governor. The page reports these and says how to fix them.
- Tuning other interfaces. The page acts on the iD2+ the app is connected to.
- JACK-only or plain-ALSA systems. PipeWire is required, as it already is for
  the sample rate. Without PipeWire the page says so and shows nothing else.
- Fixing an application that misses its own deadline. The page shows that this
  is the cause; it cannot cure it.

## Anti-goals

- Leaving the system forced to a buffer size after the app exits or crashes.
- Editing or deleting a configuration file the app did not create.
- A test run that plays an audible signal out of the monitors or headphones.
- Changing a setting without the user asking, including "helpful" automatic
  tuning in the background.
- Slowing the mixer UI or the USB worker. Monitoring runs only while the
  Latency page is open or a test is running.

## Constraints

- Rust, iced 0.14, MSRV 1.90, as the rest of the app. No root. No new C
  library dependency (see Approaches).
- The kernel driver owns the streaming interface. The app controls streaming
  only through PipeWire, as `clock.rs` does for the sample rate.
- Public repository: nothing machine-specific in defaults or paths.
- Tests must not open audio devices or need hardware. Hardware checks are a
  manual walk-through.

## Approaches

**A. Drive PipeWire's own command-line tools (chosen).** `pw-top` in batch
mode gives, per node and per second: quantum, rate, wait time, busy time, the
share of the period used, and a cumulative error count. `pw-dump` gives node
properties and reported latency. `pw-metadata` forces the quantum, live.
`pw-cli` sets the interface node's period size and headroom. This is how
`clock.rs` already works, adds no build dependencies, and runs wherever
PipeWire is installed. Cost: `pw-top` output is text meant for people, so the
parser must tolerate column changes between PipeWire versions, and resolution
is one second.

**B. Link libpipewire through its Rust bindings.** Reads the profiler stream
directly: per-cycle timing, exact xrun times, no text parsing. Cost: a C
library and bindgen at build time for everyone who builds the app, a larger
CI package list, and several times the code. Worth revisiting only if one
second resolution proves too coarse.

**C. Test the ALSA device directly, outside PipeWire.** Gives the interface's
own floor with nothing else in the way. Cost: PipeWire has to release the
device for each test, and the result is not the setup people actually run.
Dropped.

A is chosen because it reuses a pattern the app already has and keeps the
build simple for people who compile it themselves. The parsing sits in its own
module, so B could replace the data source later without touching the page.

## Design

### What the user sees

A fourth tab, **Latency**, after Mixer, Routing and Settings.

```
 LATENCY                                              [ Revert to system ]

 Now      48 kHz   64 frames   1.3 ms out / 2.7 ms in + out   (buffers only)

 Mode     ( Safe )  ( Balanced )  ( Aggressive )        [ Advanced v ]
          Buffer 64   Period 64   Headroom 32

 Xruns    Interface   0        last: none
          Apps        3        last: 14:02:11  "My DAW"
 Load     Interface   |##........|  18 %   peak 31 %
          My DAW      |#######...|  71 %   peak 104 %   <- over the period

 [ history strip: xruns per second, last 5 minutes ]

 Test     ( Interface only )  ( With my session )     [ Run test  ~3 min ]
          Buffer   Period  Headroom   Xruns   Peak load   Result
          256      256     256        0       9 %         clean
          128      128     64         0       17 %        clean
          64       64      32         0       31 %        clean   <- suggested
          32       32      32         12      88 %        glitches
                                                         [ Use suggested ]

 System   ok    Realtime scheduling available (priority 88)
          ok    CPU governor: performance
          warn  IRQ threading off: add threadirqs to the kernel command line
          warn  Card profile is not Pro Audio
          ok    USB power saving off for the interface
```

- **Now** is what the graph is actually running at, not what was asked for.
- **Xruns** are counted from when the page opened or the counter was reset.
  Interface xruns point at the device, USB or system. App xruns with a clean
  interface point at the application.
- **Load** is the share of each period spent processing. Over 100 % is a
  missed deadline.
- **Revert to system** lifts every force and removes the app's own
  configuration file, returning PipeWire to whatever it would do without
  iD Mixer.

**Who gets the blame.** "Interface" is the iD2+'s playback and capture nodes
added together. "Apps" is every other node in the graph the iD2+ is driving,
listed by name. Counts are deltas from a per-node baseline: a node that
disappears and comes back (an app restarting, the stream reopening after a
settings change) starts a new baseline and is not counted as a glitch. If the
iD2+ is idle, or something else is driving the graph, the page says so
instead of showing counts that would point at the wrong thing.

### Settings and modes

**Changed 17 Nov 2026 (Danny): buffer-size presets replace the three named
modes.** The keys are Off, 32, 64, 128 and 256. Each buffer size has its own
period and headroom, tuned for that size; those two are what "optimised"
means, and what the test run measures and sets per size. Where this document
says Safe, Balanced or Aggressive, read the buffer-size presets. Starting
values: 32/32/32, 64/64/32, 128/128/64, 256/256/256 (buffer/period/headroom).

Three values are controlled:

| Value | How it is applied | Takes effect |
|---|---|---|
| Buffer (graph quantum) | `pw-metadata -n settings 0 clock.force-quantum N` | at once |
| ALSA period size | Props param `api.alsa.period-size` on the interface's nodes | to be confirmed on hardware |
| ALSA headroom | Props param `api.alsa.headroom` on the interface's nodes | to be confirmed on hardware |

Both ALSA values appear in the nodes' Props params in `pw-dump` (checked on
PipeWire 1.6), so they can be written with `pw-cli set-param`. Whether a write
reaches a running stream, or only the next time it opens, is an open item.

The buffer force is graph-wide: while it is on, it overrides the buffer size
an application asks for, including a DAW's own setting. That is deliberate (a
test has to control the buffer, and the page is the one place to set it), and
the page says so next to the mode switch. **Off** lifts the force and lets
applications choose again.

Before the first change, the app records the values it found (an existing
quantum force, the nodes' period and headroom). Lifting a session change, or
stopping a test, writes those back; it does not assume zero or the defaults.
Accepted ranges: buffer a power of two from 16 to 8192 frames, period 16 to
1024, headroom 0 to 1024.

Modes are named starting points for those three values. Starting values (to
be revised once measured on hardware; see Open items):

| Mode | Buffer | Period | Headroom |
|---|---|---|---|
| Safe | 256 | system default | system default |
| Balanced | 128 | 128 | 64 |
| Aggressive | 32 | 32 | 32 |

After a test run, the page offers to redefine the modes from that machine's
results: Safe is the lowest buffer that was clean with peak load under 50 %,
Balanced the lowest clean buffer, Aggressive one step below Balanced. Accepted
values are written to the active profile.

### Profiles

A **profile** is one file holding a machine's tuning: the three mode
definitions, the mode in use, the hold time, and the summary of the test run
they came from (date, kernel, PipeWire version, CPU model, the result table).
It holds no paths, user names or serial numbers.

- The active profile lives at `$XDG_CONFIG_HOME/id-mixer/latency.toml`. The
  app reads it at start and writes it when a mode changes or a test finishes.
  A hand edit is picked up on the next start.
- **Export** saves the active profile to a file the user chooses. **Import**
  loads one. This is how a user keeps one profile per machine, moves a setup
  to a reinstall, or shares what works on a given computer.
- An imported profile from a different machine (CPU model differs) is marked
  **untested on this machine** on the page until a test run confirms it. It
  is applied only when the user picks a mode, never on import alone.
- Without a profile the app uses the built-in starting values below.
- The repository carries example profiles under `profiles/`, named by
  machine, starting with the development machine's once it has been measured.
  They are examples to import, not defaults, and are not installed.

A file that does not parse, or has values outside what the interface accepts,
is refused with the reason shown; the active profile is left as it was.

**Session versus kept.** A change applies for the session only: it is lifted
when the app exits, as the forced sample rate is. **Keep** writes one
WirePlumber drop-in owned by the app
(`$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/50-id-mixer-latency.conf`)
holding the period and headroom, and one PipeWire drop-in for the default
quantum, so the settings hold without the app running. Revert deletes those
two files and nothing else. If another file already sets the same properties
for the interface, the page names it and says its values win or lose by file
order; it does not touch it.

**Crash safety.** While a force is active, the app records it in
`$XDG_STATE_HOME/id-mixer/latency` (the talkback restore file is the model).
The record holds the values found before the change. On the next start a
leftover record means the app died: those values are written back before
anything else.

The kept WirePlumber rule matches the interface's node names as PipeWire
reports them for the connected device at the time Keep is pressed; nothing is
hard-coded for one unit. The profile file carries a format version and gives
sizes in frames.

### Test run

Two kinds:

- **Interface only.** The app starts a silent duplex stream on the interface
  so the graph runs with the iD2+ as its driver and nothing else loading it.
  Result: the floor for this machine and this interface. If the tool needed
  for the silent stream is not installed, this kind is greyed out with the
  reason and the other still works.
- **With my session.** The user starts their DAW playing first. The app only
  steps the settings and counts. Result: what that session can hold.

For each step: apply the settings, wait for the graph to settle (discard the
first two seconds, which always carry start-up errors), hold for the chosen
time (default 20 s, choices 10 / 20 / 60 s), record xruns for the interface
and for each app, and average and peak load. Steps run from the largest
buffer down and stop two steps after the first one that glitches. Stop is
always available and restores the settings from before the run.

Verdicts: **clean** is zero xruns in the hold. **glitches** is anything else.
The suggestion is the lowest clean step whose peak load is under 80 %.

Results are kept (last ten runs, with date, kernel and PipeWire version) and
can be copied as text for a bug report or forum post.

### System check

Read-only, refreshed when the page opens. Each line is ok, warn or unknown,
with one sentence on why it matters and what to do.

- PipeWire running, version, and whether its data thread has realtime
  scheduling.
- Realtime limits for the user (rtprio, memlock).
- Kernel preemption model and whether IRQ threading is on.
- CPU governor.
- USB power saving for the interface.
- Whether the interface shares a USB controller with other busy devices.
- Card profile (Pro Audio avoids channel remixing and keeps input and output
  on one clock).
- Any other configuration file that sets period or headroom for the
  interface.

### Structure

- New crate **`idctl-audio`**: no GUI types.
  - `parse`: `pw-top` batch lines and `pw-dump` JSON into plain structs. Pure.
  - `stats`: counters, deltas since reset, load averages and peaks, verdicts,
    the suggestion rule, mode derivation. Pure.
  - `sweep`: the test run as a state machine driven by ticks and samples.
    Pure; time and commands are injected.
  - `syscheck`: each check as a function from file contents to a status.
    Pure; a thin reader supplies the contents.
  - `pw`: the only module that spawns PipeWire tools. `clock.rs` moves here so
    rate and quantum share one runner.
- **`idctl-gui`**: `views/latency.rs`, a `Tab::Latency`, messages, and a
  subscription that runs the monitor while the page is open or a test runs.
- New dependency: `serde_json` for `pw-dump`.

### Testing

- Parsers: fixtures captured from real `pw-top` and `pw-dump` output
  (PipeWire 1.6), including a suspended interface, an app over its period,
  and a header with an unknown extra column.
- `stats` and `sweep`: unit tests with a fake command runner and injected
  time, covering stop mid-run, a tool that fails, and the interface vanishing.
- `syscheck`: one test per check per status.
- Crash safety: leftover state file is lifted on start.
- Hardware walk-through on an iD2+ (manual): monitor against `pw-top` by eye,
  each mode, Keep and Revert, both test kinds, unplug during a test.

### Order of work

1. Monitor (read-only): crate, parsers, page with Now, Xruns, Load, history.
2. Controls: buffer, period, headroom, modes, session force, crash safety,
   Keep and Revert.
3. Test run and saved results.
4. System check.

Each step is usable on its own. Step 1 already answers "is it the interface or
the app?".

## Open items for the hardware pass

**Measured 17 Nov 2026** (iD2+, PipeWire 1.6.9, silent stream):

- Period size and headroom written with `pw-cli set-param <node> Props` are
  accepted and reported back at once, with no WirePlumber restart. The ALSA
  device picks the new period up at the next reconfiguration, and a change of
  the forced quantum is one. So a session change writes the node values, then
  the quantum; if the quantum is already the wanted value it is moved one
  step away and back.
- The device's real ALSA period is half of `api.alsa.period-size` (32 gives
  16, 256 gives 128): PipeWire halves it for USB devices.
- Still open: whether a buffer change pops the monitors (needs ears), and the
  mode table's starting values.

These need the device and are settled during step 2, not before:

- Whether period size and headroom set through Props apply without restarting
  WirePlumber, and what makes a running stream pick them up (reopening the
  stream, suspending the node, or neither). If there is no route that avoids
  restarting WirePlumber, the test run steps the buffer only, period and
  headroom become Keep-only settings, and the design comes back for a
  decision.
- Whether a buffer change makes the monitors pop. If it does, the app mutes
  the monitor output around each change.
- The starting values in the mode table.

## Decisions made by the author, not yet confirmed by Danny

- Buffer latency only, no measured round trip, in this version.
- Session-only by default, with an explicit Keep.
- Test steps run largest to smallest and stop two steps after the first
  glitch.
- Monitoring runs only while the page is open or a test is running.
- A new crate rather than more modules in `idctl-gui`.
- Picking a mode overrides the buffer size set inside a DAW until the mode is
  switched off.
- Profiles as described: one active file, export and import, examples in the
  repository, imported profiles marked untested until a test run.

## Left to the builder

Exact widget layout within the sketch, message and state naming, the history
strip's drawing, fixture file layout, and how the silent duplex stream is
started (`pw-cat`, `pw-loopback` or equivalent), provided nothing audible
reaches an output.
