# Latency Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Latency tab to D+ Control that shows xruns and load split between the interface and each app, changes buffer settings safely, runs a repeatable test, and reports system settings.

**Architecture:** A new crate `idctl-audio` holds everything that is not GUI: pure parsers, statistics, the test-run state machine, profile and config-file text, system checks, and one module (`pw`) that spawns PipeWire's command-line tools. `idctl-gui` gains a `latency` state module and a `views/latency.rs` page fed by a subscription that runs only while the page is open or a test runs.

**Tech Stack:** Rust 1.90, iced 0.14, tokio (process, io-util), `serde` + `serde_json` (`pw-dump`), `toml` (profiles), `rfd` (file chooser). PipeWire tools: `pw-top`, `pw-dump`, `pw-metadata`, `pw-cli`, `pw-cat`.

**Spec:** `docs/superpowers/specs/2026-11-17-latency-page-design.md`. Read it first; this plan argues from it.

## Global Constraints

- MSRV 1.90. No root. No C library dependency. PipeWire is reached only through its command-line tools.
- Tests never open an audio device, spawn a PipeWire tool, or need hardware. `pw` functions that spawn are not unit-tested; their parsing halves are.
- While working, run only: `nice -n 19 cargo check -j 2`, `nice -n 19 cargo test -j 2 -p idctl-audio`, `nice -n 19 cargo test -j 2 -p idctl-gui`, `cargo fmt --all`, `nice -n 19 cargo clippy -j 2 -p <crate> --all-targets -- -D warnings`. No whole-workspace release build, no hardware test, without Danny asking.
- Nothing scripted writes to PipeWire or the device outside Tasks 6 and 16, which Danny runs or watches.
- Commits: subject starts `Latency: `. Author Danny Wincott. No AI or assistant names, trailers or co-author lines anywhere, in commits or code.
- Public repository: no machine-specific paths, user names or serial numbers in code, fixtures or defaults. Replace the serial in captured fixture node names with `00-00-0000-0000-00`. The vendor's brand name stays out of code and docs; the node-name fixtures are the one place it appears, as PipeWire reports it.
- Files the app may create or delete: `$XDG_CONFIG_HOME/dplus-control/latency.toml`, `$XDG_STATE_HOME/dplus-control/latency`, `$XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/50-dplus-control-latency.conf`, `$XDG_CONFIG_HOME/pipewire/pipewire.conf.d/50-dplus-control-latency.conf`. No others.
- Accepted ranges: buffer a power of two from 16 to 8192 frames, period 16 to 1024, headroom 0 to 1024.
- One file, one job; split a file that passes about 500 lines. Match the existing code's style (see `talkback.rs`, `clock.rs`, `views/settings.rs`).

## Review Focus

1. **PipeWire absent or stopped** (`pw-top` missing, exits, or the daemon restarts): the page shows "PipeWire is not running" and retries every 2 s; no panic, no busy loop, the mixer keeps working. Test in Task 5.
2. **Interface unplugged or idle during a test or while forced**: the test aborts with a reason and restores what it found; restoring a node that no longer exists is not an error. Tests in Tasks 8 and 13.
3. **`pw-top` rows that are not the expected shape** (extra or missing columns, node names with spaces, `---` cells, a locale decimal comma, a truncated last line): the row is parsed or skipped, never a panic, never a mis-attributed count. Tests in Task 1.
4. **App killed while a force is active**: the next start writes the recorded values back before anything else, and a record that does not decode is deleted, not retried forever. Tests in Tasks 7 and 9.
5. **A profile file from someone else** (wrong version, values out of range, non-power-of-two buffer, 50 MB of junk, valid TOML of the wrong shape): refused with the reason, active profile untouched. Tests in Task 10.

## File Structure

```
crates/idctl-audio/
  Cargo.toml
  src/lib.rs          module list only
  src/top.rs          pw-top batch text -> Row, Frames
  src/graph.rs        pw-dump JSON -> Interface (the iD2+ device and its nodes)
  src/stats.rs        Monitor: frames -> Snapshot, baselines, history, buffer_ms
  src/tuning.rs       Tuning, Mode, Modes, validate, Found (+ state-file text)
  src/profile.rs      Profile, Run, Step, Verdict, Kind; TOML in and out
  src/keep.rs         drop-in file text, write, remove, conflict scan
  src/sweep.rs        test-run state machine, suggest, derive_modes
  src/syscheck.rs     Facts -> Vec<Check>, and the reader that fills Facts
  src/pw.rs           the only module that spawns PipeWire tools
  fixtures/           captured pw-top and pw-dump output
crates/idctl-gui/src/
  latency.rs          page state, Msg, update
  views/latency.rs    the page
  state.rs            state_path(name), shared with talkback.rs
  clock.rs            deleted (moves to idctl-audio::pw)
profiles/README.md    what example profiles are, how to import one
```

---

## Phase 1: Monitor

### Task 1: Crate and `pw-top` parser

**Files:**
- Create: `crates/idctl-audio/Cargo.toml`, `src/lib.rs`, `src/top.rs`, `fixtures/top-idle.txt`, `fixtures/top-driving.txt`
- Modify: `Cargo.toml` (workspace member)

**Interfaces:**
- Produces:
  ```rust
  pub struct Row { pub id: u32, pub quantum: u32, pub rate: u32,
      pub load: Option<f32>,   // the B/Q column: share of the period spent processing
      pub errors: u64, pub follower: bool, pub name: String }
  pub struct Header { /* index of each known column */ }
  pub fn parse_header(line: &str) -> Option<Header>;
  pub fn parse_row(header: &Header, line: &str) -> Option<Row>;
  #[derive(Default)] pub struct Frames { /* header + rows so far */ }
  impl Frames {
      /// Feed one line. Returns the finished frame when the next header arrives.
      pub fn push(&mut self, line: &str) -> Option<Vec<Row>>;
  }
  ```

Format facts (PipeWire 1.6): the header is `S   ID  QUANT   RATE    WAIT    BUSY   W/Q   B/Q  ERR FORMAT           NAME`. Columns up to `ERR` are whitespace-separated and always present. After `ERR` comes an optional format (three tokens such as `S32LE 2 48000`), then an optional follower marker `=` or `+`, then the name, which is the rest of the line and may contain spaces. Idle nodes show `---` in the timing columns. Followers are listed directly under their driver.

- [ ] **Step 1: Capture fixtures.** `top-idle.txt`: output of `pw-top -b -n 2` with the interface suspended. `top-driving.txt`: hand-built from the same format with the interface driving at quantum 64, one follower `= My DAW` with `B/Q` 1.04 and `ERR` 3, and one follower whose name has a space. Replace the serial as Global Constraints say.
- [ ] **Step 2: Write failing tests** in `top.rs`:
  - `header_columns_are_found_by_name`: the real header parses; a header with an extra column `XTRA` between `B/Q` and `ERR` still yields the right `errors` for a matching row.
  - `idle_row_has_no_load`: `---` cells give `load == None`, `errors == 0`.
  - `follower_marker_and_name_with_spaces`: `... 3 F32LE 2 48000  = My DAW` gives `follower == true`, `name == "My DAW"`, `errors == 3`, `load == Some(1.04)`.
  - `row_without_format_keeps_its_name`: a driver row with no format tokens gives the full node name.
  - `bad_rows_are_skipped`: empty line, truncated line, non-numeric id, and `0,31` in `B/Q` all return `None` or a row with `load == None`; none panic.
  - `frames_complete_on_the_next_header`: pushing `top-driving.txt` line by line returns one frame per header after the first, with every row of that iteration.
- [ ] **Step 3: Run** `nice -n 19 cargo test -j 2 -p idctl-audio top` and see them fail to compile.
- [ ] **Step 4: Implement** `top.rs`. No dependencies. The first status column is ignored.
- [ ] **Step 5: Run** the tests; expect PASS. Run fmt and clippy on the crate.
- [ ] **Step 6: Commit** `Latency: parse pw-top batch output`.

### Task 2: `pw-dump` parser

**Files:**
- Create: `crates/idctl-audio/src/graph.rs`, `fixtures/dump-output-only.json`, `fixtures/dump-pro-audio.json`
- Modify: `crates/idctl-audio/Cargo.toml` (`serde`, `serde_json`), `src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Node { pub id: u32, pub name: String, pub capture: bool,
      pub running: bool, pub period: u32, pub headroom: u32 }
  pub struct Interface { pub device_name: String, pub profile: String, pub nodes: Vec<Node> }
  /// The device with this USB vendor and product id, and the nodes that belong to it.
  pub fn interface(dump: &str, vendor: u16, product: u16) -> Result<Option<Interface>, String>;
  ```

Shape facts: a device is an object of `type` `PipeWire:Interface:Device` with `info.props["device.vendor.id"]` and `["device.product.id"]` as strings like `"0x2708"`; its active profile is `info.params.Profile[0].name`. Its nodes are `PipeWire:Interface:Node` objects whose `info.props["device.id"]` equals the device's `id`. `capture` is `media.class == "Audio/Source"`. `running` is `info.state == "running"`. Period and headroom are in `info.params.Props[*].params`, a flat array alternating key and value: `"api.alsa.period-size", 32, "api.alsa.headroom", 32`.

- [ ] **Step 1: Capture fixtures.** Trim real `pw-dump` output to the device, its nodes and two unrelated nodes. `dump-pro-audio.json` is the same with the profile `pro-audio` and both a sink and a source node.
- [ ] **Step 2: Write failing tests:** `finds_the_interface_and_its_nodes` (output-only fixture: one node, `capture == false`, period 32, headroom 32, profile `output:multichannel-output`); `pro_audio_has_both_directions`; `another_vendor_is_not_ours` (returns `Ok(None)`); `garbage_is_an_error_not_a_panic` (`"not json"`, `"[]"`, a node with no `props`).
- [ ] **Step 3: Run**, see FAIL. **Step 4: Implement** with `serde_json::Value`; no derive structs for the dump. **Step 5: Run**, PASS, fmt, clippy.
- [ ] **Step 6: Commit** `Latency: find the interface in pw-dump output`.

### Task 3: Monitor statistics

**Files:**
- Create: `crates/idctl-audio/src/stats.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `top::Row`.
- Produces:
  ```rust
  pub const HISTORY_SECS: usize = 300;
  #[derive(Clone, PartialEq)] pub enum Driving { Idle, Other(String), Interface }
  #[derive(Clone)] pub struct Party { pub name: String, pub xruns: u64,
      pub last_xrun: Option<u64>, pub load: f32, pub peak: f32 }
  #[derive(Clone)] pub struct Snapshot { pub driving: Driving, pub quantum: u32, pub rate: u32,
      pub interface: Party, pub apps: Vec<Party> }
  pub struct Monitor { /* ... */ }
  impl Monitor {
      pub fn new(interface_nodes: Vec<String>) -> Self;
      pub fn set_interface(&mut self, names: Vec<String>);
      /// One pw-top frame, stamped with unix seconds.
      pub fn frame(&mut self, rows: &[Row], at: u64) -> Snapshot;
      pub fn reset(&mut self);
      /// Xruns per second (interface + apps), oldest first, at most HISTORY_SECS.
      pub fn history(&self) -> impl Iterator<Item = u32> + '_;
  }
  /// Milliseconds added by buffering in one direction.
  pub fn buffer_ms(quantum: u32, headroom: u32, rate: u32) -> f32;
  ```

Rules from the spec: the interface party is the sum over the interface's nodes; apps are the follower rows listed under an interface node that is a driver row with `quantum > 0`. `Driving::Idle` when no interface node has `quantum > 0`; `Driving::Other(name)` when interface nodes are followers of another driver. Counts are deltas from a per-node baseline taken at first sight or `reset`; a node id that vanishes and returns, or whose `errors` drops, gets a new baseline and adds nothing. `peak` is the maximum `load` since reset.

- [ ] **Step 1: Write failing tests:** `interface_and_apps_are_counted_apart`; `baseline_hides_errors_from_before_the_page_opened`; `a_returning_node_is_not_a_glitch` (errors 5, gone, back at 0, then 2: total 2 more, not 7 and not negative); `idle_interface_reports_idle`; `another_driver_is_named`; `peak_load_is_kept`; `history_is_capped_at_300`; `buffer_ms_matches_the_arithmetic` (`buffer_ms(64, 32, 48_000) == 2.0`); `reset_zeroes_counts_and_peaks`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: split xruns and load between interface and apps`.

### Task 4: PipeWire runner, and move the clock into it

**Files:**
- Create: `crates/idctl-audio/src/pw.rs`
- Delete: `crates/idctl-gui/src/clock.rs`
- Modify: `crates/idctl-audio/Cargo.toml` (`tokio` with `process`, `io-util`), `crates/idctl-gui/Cargo.toml` (`idctl-audio`), `crates/idctl-gui/src/main.rs`, `crates/idctl-gui/src/views/settings.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const RATES: [u32; 6];                               // moved unchanged
  pub async fn force_rate(hz: u32) -> Result<(), String>;  // was clock::force
  pub fn release_rate();                                   // was clock::release
  pub async fn dump() -> Result<String, String>;           // pw-dump stdout
  pub struct Top { /* child + buffered stdout */ }
  impl Top {
      pub fn spawn() -> std::io::Result<Top>;              // `pw-top -b`, kill_on_drop
      pub async fn line(&mut self) -> Option<String>;      // None when pw-top exits
  }
  ```

- [ ] **Step 1: Move** `clock.rs` into `pw.rs` with the two renames; keep the system-binary preference and generalise it to `fn tool(name: &str) -> PathBuf` (prefer `/usr/bin/<name>`).
- [ ] **Step 2: Add** `dump` and `Top`. `Top::spawn` sets `LC_ALL=C` so numbers use a decimal point.
- [ ] **Step 3: Update** the three call sites (`main.rs` lines near 189 and 309, `settings.rs` `RATES`).
- [ ] **Step 4: Run** `nice -n 19 cargo check -j 2` and `nice -n 19 cargo test -j 2 -p idctl-gui`; expect the existing 22 tests to pass. fmt, clippy on both crates.
- [ ] **Step 5: Commit** `Latency: one runner for PipeWire tools`.

### Task 5: The Latency tab, monitor section

**Files:**
- Create: `crates/idctl-gui/src/latency.rs`, `crates/idctl-gui/src/views/latency.rs`
- Modify: `crates/idctl-gui/src/main.rs` (`Tab::Latency`, `Message::Latency(latency::Msg)`, `App.latency`, subscription), `crates/idctl-gui/src/views/mod.rs` (tab key, body match; hide the monitor side panel on this tab as Routing does)

**Interfaces:**
- Consumes: `top::Frames`, `graph::interface`, `stats::{Monitor, Snapshot, Driving, buffer_ms}`, `pw::{Top, dump}`.
- Produces:
  ```rust
  // latency.rs
  pub enum Feed { Line(String), Down }          // from the pw-top subscription
  pub enum Msg { Feed(Feed), Dumped(Result<String, String>), ResetCounts }
  pub struct State { pub snapshot: Option<Snapshot>, pub interface: Option<Interface>,
      pub down: bool, /* monitor, frames */ }
  impl State {
      pub fn update(&mut self, msg: Msg, now: u64) -> iced::Task<Msg>;
      pub fn history(&self) -> impl Iterator<Item = u32> + '_;
  }
  pub fn feed() -> iced::Subscription<Feed>;    // spawns Top; on exit emits Down, waits 2 s, respawns
  ```

Behaviour: the subscription is in the batch only when `app.tab == Tab::Latency` (Task 14 adds "or a test is running"). Opening the tab requests a `dump`; a dump is requested again when a frame shows an interface node id the state does not know. Vendor and product ids come from `idctl-usb`'s constants. Page copy, exact:
- no PipeWire: `PipeWire is not running. This page needs it.`
- `Driving::Idle`: `The interface is idle. Play something through it to see xruns and load.`
- `Driving::Other(n)`: `The interface is not driving the graph ("{n}" is). Counts below would point at the wrong thing, so none are shown.`
- under Now: `(buffers only)`

Layout follows the spec's sketch: Now line, Xruns block (Interface, Apps, each with last time as `HH:MM:SS` local), Load bars with percentage and peak (a bar turns `p::MUTE_ON` at 100 % or more), history strip (canvas, one column per second), `Reset counts` key. Reuse `card`, `key`, `caption` and the palette; add no new colours.

- [ ] **Step 1: Write failing tests** in `latency.rs`: `lines_become_a_snapshot` (feed `top-driving.txt` lines plus a dump message built from `dump-pro-audio.json`; snapshot has `Driving::Interface` and the app's 0 xruns after baseline); `down_then_line_recovers` (`Feed::Down` sets `down`, the next header line clears it and starts fresh frames, no panic on a half frame); `reset_clears_counts`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement** state, subscription and view. **Step 4: Run** `-p idctl-gui` tests, PASS; fmt, clippy.
- [ ] **Step 5: Build and look.** `nice -n 19 cargo run -j 2 -p idctl-gui`, open the tab, confirm the idle text shows and the mixer tabs still behave. Take a screenshot for the report (window capture steps are in the project notes).
- [ ] **Step 6: Commit** `Latency: tab with live xruns, load and history`.

### Task 6: Hardware checkpoint (Danny at the machine)

Not code. Needs the iD2+ and Danny's go-ahead for each write. Record the answers in the spec's "Open items" section and commit that edit as `Latency: record hardware findings`.

- [ ] **Monitor against `pw-top`:** with a DAW playing at 128, then 64, then 32, the page's interface and app xruns match `pw-top`'s `ERR` deltas over 60 s. Note which side glitches at 32.
- [ ] **Do period and headroom apply live?** Read a node's values, then `pw-cli set-param <id> Props '{ params = [ "api.alsa.headroom" 64 ] }'`, then check `pw-dump` and `/proc/asound/card*/pcm0p/sub0/hw_params` while playing, after stopping and restarting playback, and after `pw-cli set-param <id> Props` back. Answer one of: live / on stream reopen / only via WirePlumber rule.
- [ ] **Does a buffer change pop?** Change `clock.force-quantum` while audio plays at low monitor level. Answer yes or no.
- [ ] **Stop here if the answer to the second item is "only via WirePlumber rule".** The spec says the design comes back to Danny: Phase 2 then treats period and headroom as Keep-only and the test run steps the buffer only (`Tuning.period` and `.headroom` stay `None` in session changes).

---

## Phase 2: Controls

### Task 7: Tuning values and the crash record

**Files:**
- Create: `crates/idctl-audio/src/tuning.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
  pub struct Tuning { pub quantum: u32, pub period: Option<u32>, pub headroom: Option<u32> } // None: leave as found
  #[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
  pub enum Mode { Safe, Balanced, Aggressive }
  #[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
  pub struct Modes { pub safe: Tuning, pub balanced: Tuning, pub aggressive: Tuning }
  impl Modes { pub fn get(&self, mode: Mode) -> Tuning; }
  impl Default for Modes;   // Safe 256/None/None, Balanced 128/128/64, Aggressive 32/32/32
  pub fn validate(t: &Tuning) -> Result<(), String>;
  /// What was in force before the app changed anything.
  #[derive(Clone, PartialEq, Debug)]
  pub struct Found { pub force_quantum: u32, pub nodes: Vec<(String, u32, u32)> } // name, period, headroom
  pub fn encode(found: &Found) -> String;
  pub fn decode(text: &str) -> Option<Found>;
  ```

- [ ] **Step 1: Write failing tests:** `defaults_match_the_design_table`; `validate_accepts_the_edges` (16, 8192, period 16 and 1024, headroom 0 and 1024); `validate_refuses` (quantum 0, 48, 16384; period 8; headroom 2048) each with a message naming the field; `found_survives_the_state_file_format` (node names containing spaces and `=`); `decode_refuses_junk` (empty, truncated, binary bytes as lossy text).
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement**; the state format is line-based text like `talkback.rs`'s. **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: tuning values, modes and the restore record`.

### Task 8: Apply and restore

**Files:**
- Modify: `crates/idctl-audio/src/pw.rs`

**Interfaces:**
- Consumes: `tuning::{Tuning, Found}`, `graph::Interface`.
- Produces:
  ```rust
  pub fn parse_forced_quantum(metadata_out: &str) -> Option<u32>;   // pure
  pub async fn forced_quantum() -> Result<u32, String>;
  pub async fn find(iface: &Interface) -> Result<Found, String>;    // current force + node values
  pub async fn apply(t: &Tuning, iface: &Interface) -> Result<(), String>;
  pub async fn restore(found: &Found) -> Result<(), String>;
  pub fn restore_blocking(found: &Found);                           // shutdown: bounded like release_rate
  /// Which node ids to write for these recorded names, given a fresh dump. Pure.
  pub fn restore_targets(found: &Found, iface: Option<&Interface>) -> Vec<(u32, u32, u32)>;
  ```

Commands: quantum `pw-metadata -n settings 0 clock.force-quantum <N>` (0 lifts). Node values `pw-cli set-param <id> Props '{ params = [ "api.alsa.period-size" <P> "api.alsa.headroom" <H> ] }'`, passed as argv, never through a shell. `pw-metadata -n settings 0 clock.force-quantum` prints a line containing `key:'clock.force-quantum' value:'<N>'`. Apply order: node values first, then quantum. Restore order: quantum first, then nodes. If Task 6 found that node values need the stream reopened, `apply` and `restore` follow the node write with the step Task 6 recorded.

- [ ] **Step 1: Write failing tests:** `forced_quantum_is_read_from_metadata_output` (real line, value 0 and 64; missing key gives `None`); `restore_skips_nodes_that_are_gone` (`restore_targets` with `None` and with an interface missing one recorded name returns only the nodes present, no error).
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: apply and restore buffer settings`.

### Task 9: Session force and crash safety in the app

**Files:**
- Create: `crates/idctl-gui/src/state.rs`
- Modify: `crates/idctl-gui/src/talkback.rs` (use `state::path("talkback")`), `latency.rs`, `main.rs` (`shutdown`, startup)

**Interfaces:**
- Produces:
  ```rust
  // state.rs
  pub fn path(name: &str) -> Option<PathBuf>;   // $XDG_STATE_HOME/dplus-control/<name>, falling back to ~/.local/state
  // latency.rs additions
  pub enum Msg { /* ... */ Apply(Tuning), Applied(Result<(), String>), Off, Restored(Result<(), String>) }
  impl State {
      pub fn forced(&self) -> Option<&Found>;
      /// Call once at start: a leftover record means the app died while forcing.
      pub fn recover() -> iced::Task<Msg>;
  }
  ```

Behaviour: the first `Apply` in a session calls `pw::find`, stores the `Found` in memory and in the state file, then `pw::apply`. Later `Apply`s keep the first `Found`. `Off` and shutdown restore it and delete the file (`shutdown` uses `restore_blocking`). `recover` reads the file, restores, and deletes it; a file that does not decode is deleted. An `Apply` that fails validation shows the message and changes nothing. Apply errors show under the mode switch in `p::MUTE_ON`.

- [ ] **Step 1: Write failing tests** with the record path injected (a `tempfile`-free approach: a function taking `&Path`, tested against `std::env::temp_dir()` plus a unique name, removed at the end): `first_apply_records_what_was_found`; `second_apply_keeps_the_first_record`; `off_clears_the_record`; `undecodable_record_is_deleted`; `talkback_file_still_round_trips` (existing behaviour after the `state::path` move).
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: session changes are undone on exit and after a crash`.

### Task 10: Profiles

**Files:**
- Create: `crates/idctl-audio/src/profile.rs`
- Modify: `crates/idctl-audio/Cargo.toml` (`toml`), `src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const VERSION: u32 = 1;
  pub const MAX_BYTES: usize = 256 * 1024;
  pub const MAX_RUNS: usize = 10;
  pub enum Kind { InterfaceOnly, WithSession }
  pub enum Verdict { Clean, Glitches }
  pub struct Step { pub tuning: Tuning, pub interface_xruns: u64, pub app_xruns: u64,
      pub avg_load: f32, pub peak_load: f32, pub verdict: Verdict }
  pub struct Run { pub date: String, pub kernel: String, pub pipewire: String,
      pub kind: Kind, pub hold_secs: u32, pub steps: Vec<Step> }
  pub struct Profile { pub version: u32, pub machine: String, pub mode: Option<Mode>,
      pub hold_secs: u32, pub modes: Modes, pub runs: Vec<Run> }
  impl Profile {
      pub fn new(machine: String) -> Self;                 // default modes, hold 20, no mode, no runs
      pub fn parse(text: &str) -> Result<Profile, String>;
      pub fn to_toml(&self) -> String;
      pub fn untested_on(&self, machine: &str) -> bool;    // machine differs, or no runs
      pub fn push_run(&mut self, run: Run);                // keeps the last MAX_RUNS
  }
  /// CPU model from /proc/cpuinfo text; "unknown" if absent.
  pub fn machine(cpuinfo: &str) -> String;
  ```

All types derive `Serialize`, `Deserialize`, `Clone`, `PartialEq`, `Debug`. `parse` refuses, with a message a user can act on: text over `MAX_BYTES`; a `version` other than 1 (`This profile is version {n}; this D+ Control reads version 1.`); any mode failing `tuning::validate`; `hold_secs` not 10, 20 or 60; more than `MAX_RUNS` runs is truncated, not refused.

- [ ] **Step 1: Write failing tests:** `round_trips`; `new_profile_uses_the_defaults`; `refuses_other_versions`; `refuses_out_of_range_modes` (non-power-of-two buffer, period 4); `refuses_oversize_text`; `refuses_wrong_shape` (valid TOML with `modes = "fast"`); `untested_when_the_cpu_differs`; `keeps_the_last_ten_runs`; `machine_reads_model_name`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: profile file format`.

### Task 11: Keep, Revert and conflicting files

**Files:**
- Create: `crates/idctl-audio/src/keep.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const FILE: &str = "50-dplus-control-latency.conf";
  pub fn wireplumber_conf(node_names: &[String], period: u32, headroom: u32) -> String;
  pub fn pipewire_conf(quantum: u32) -> String;
  /// Other files that set period or headroom for this device. Pure: (path, contents) in.
  pub fn conflicts(files: &[(PathBuf, String)], device_name: &str) -> Vec<PathBuf>;
  pub fn write(config_home: &Path, iface: &Interface, t: &Tuning) -> std::io::Result<()>;
  pub fn remove(config_home: &Path) -> std::io::Result<()>;   // missing files are not an error
  pub fn kept(config_home: &Path) -> bool;
  ```

`wireplumber_conf` emits a `monitor.alsa.rules` block with one `matches` entry per node name (exact `node.name`, each name escaped for the config syntax) and `update-props` for `api.alsa.period-size` and `api.alsa.headroom`. `pipewire_conf` emits `context.properties` with `default.clock.quantum = N` and `default.clock.min-quantum = min(N, 32)`. Both files start with a comment: `# Written by D+ Control (Latency page). Revert removes this file.` A `Tuning` with `period` or `headroom` `None` writes only the PipeWire file. `conflicts` reports a file, other than `FILE`, whose text contains `api.alsa.period-size` or `api.alsa.headroom` and also a match pattern that covers the device (contains the device's serial-free name stem, for example `iD2`).

- [ ] **Step 1: Write failing tests:** `wireplumber_text_names_each_node`; `node_names_are_escaped` (a name containing `"`); `pipewire_text_lowers_the_minimum_only_when_needed` (16 gives min 16, 128 gives min 32); `finds_a_conflicting_rule` and `ignores_rules_for_other_cards`; `write_then_remove_leaves_nothing` and `remove_twice_is_fine` (under a unique temp directory); `remove_never_touches_other_files` (a sibling `99-x.conf` survives).
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: keep and revert configuration files`.

### Task 12: Controls on the page

**Files:**
- Modify: `crates/idctl-gui/src/latency.rs`, `views/latency.rs`, `crates/idctl-gui/Cargo.toml` (`rfd` with `default-features = false, features = ["xdg-portal", "tokio"]`)
- Create: `profiles/README.md`

**Interfaces:**
- Consumes: Tasks 7 to 11.
- Produces: `Msg` variants `Mode(Mode)`, `Advanced(bool)`, `Edit(Tuning)`, `Keep`, `Revert`, `Export`, `Import`, `Imported(Result<Profile, String>)`, `Exported(Result<(), String>)`; `State.profile: Profile`.

Behaviour: the active profile is loaded from `$XDG_CONFIG_HOME/dplus-control/latency.toml` at start (missing: `Profile::new`; unreadable: defaults in memory, the reason shown, the file left alone) and saved when a mode is chosen or edited. Mode keys: `Off`, `Safe`, `Balanced`, `Aggressive`. `Advanced` reveals three steppers for the selected mode's values; an edit validates, saves to the profile, and applies if that mode is on. `Keep` calls `keep::write` with the mode in force; `Revert to system` does `Off`, then `keep::remove`. Import reads through `Profile::parse`, replaces the active profile only on success, and never applies anything. If Task 6 found that a buffer change pops, `Apply` mutes the monitor output through the existing worker write path before the change and restores the previous mute state after `Applied`.

Exact copy:
- beside the mode keys: `While a mode is on, it sets the buffer for every app, including your DAW's own setting.`
- untested profile: `This profile came from a different computer. Run a test before trusting it.`
- kept: `Kept: these settings hold without D+ Control running. They apply fully after PipeWire restarts.`
- conflict: `{path} also sets period or headroom for this interface. Whichever file sorts later wins.`

- [ ] **Step 1: Write failing tests** in `latency.rs`: `choosing_a_mode_applies_its_tuning_and_saves_it`; `off_restores` ; `an_invalid_edit_changes_nothing`; `import_replaces_the_profile_but_applies_nothing`; `a_refused_import_keeps_the_active_profile`; `unreadable_profile_file_is_left_alone`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement** state and view. **Step 4: Run**, PASS, fmt, clippy on both crates.
- [ ] **Step 5: Write** `profiles/README.md`: what a profile is, that examples are not defaults, how to import one, and the "untested" rule. No model, assistant or vendor names.
- [ ] **Step 6: Build and look**, screenshot.
- [ ] **Step 7: Commit** `Latency: modes, keep, revert and profiles on the page`.

---

## Phase 3: Test run

### Task 13: Test-run state machine

**Files:**
- Create: `crates/idctl-audio/src/sweep.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `stats::{Snapshot, Driving}`, `tuning::{Tuning, Modes}`, `profile::{Step, Verdict}`.
- Produces:
  ```rust
  pub const SETTLE_SECS: u32 = 2;
  pub const STOP_AFTER_GLITCH: usize = 2;
  /// Largest to smallest. `buffer_only` when period and headroom cannot change in a session.
  pub fn ladder(buffer_only: bool) -> Vec<Tuning>;  // 256/256/256, 128/128/64, 64/64/32, 32/32/32, 16/16/32
  #[derive(PartialEq, Debug)]
  pub enum Action { Apply(Tuning), Wait, Done, Abort(String) }
  pub struct Sweep { /* ... */ }
  impl Sweep {
      pub fn new(steps: Vec<Tuning>, hold_secs: u32) -> Self;
      pub fn start(&mut self) -> Action;                           // Apply(first)
      pub fn applied(&mut self, result: Result<(), String>) -> Action;
      pub fn sample(&mut self, snap: &Snapshot) -> Action;         // once per second
      pub fn steps(&self) -> &[Step];                              // finished steps
      pub fn progress(&self) -> (usize, usize, u32);               // step, of, seconds left in it
  }
  pub fn suggest(steps: &[Step]) -> Option<usize>;     // lowest clean buffer with peak_load < 0.80
  pub fn derive_modes(steps: &[Step]) -> Option<Modes>;
  ```

Rules: after `applied(Ok)`, the next `SETTLE_SECS` samples are discarded and the xrun counts at the end of settling are the step's zero. The step then takes `hold_secs` samples. Verdict `Clean` is zero interface and zero app xruns in the hold. The run ends `Done` after the last step, or `STOP_AFTER_GLITCH` steps after the first `Glitches`. A sample with `Driving` other than `Interface` during settle or hold returns `Abort("The interface stopped driving the graph.")`; `applied(Err(e))` returns `Abort(e)`. After `Done` or `Abort` every call returns the same action. `derive_modes`: Safe is the lowest clean buffer with `peak_load < 0.50`, Balanced the lowest clean buffer, Aggressive the step below Balanced in the ladder (Balanced again if there is none); `None` when no step was clean.

- [ ] **Step 1: Write failing tests:** `ladder_runs_largest_first`; `buffer_only_ladder_leaves_period_alone`; `settling_xruns_are_not_counted`; `a_clean_hold_is_clean_and_a_single_xrun_is_not`; `app_xruns_alone_are_glitches_with_zero_interface_xruns`; `stops_two_steps_after_the_first_glitch`; `aborts_when_the_interface_goes_idle`; `aborts_when_apply_fails`; `done_is_sticky`; `suggests_the_lowest_clean_step_under_80_percent`; `derives_modes_from_results`; `no_clean_step_derives_nothing`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Commit** `Latency: test run state machine`.

### Task 14: Silent stream and the test on the page

**Files:**
- Modify: `crates/idctl-audio/src/pw.rs`, `crates/idctl-gui/src/latency.rs`, `views/latency.rs`, `main.rs` (subscription also runs while a test runs; close request during a test restores first)

**Interfaces:**
- Produces:
  ```rust
  // pw.rs
  pub fn silence_available() -> bool;                  // pw-cat is installed
  pub struct Silence { /* children, kill_on_drop */ }
  impl Silence { pub fn start(iface: &Interface, rate: u32) -> Result<Silence, String>; }
  // profile.rs
  pub fn text_report(run: &Run, machine: &str) -> String;   // plain table for pasting
  // latency.rs
  Msg::{ TestKind(Kind), Hold(u32), RunTest, StopTest, UseSuggested, AdoptModes }
  ```

`Silence::start` runs, per interface node, `pw-cat --playback --raw --format s32 --rate <rate> --channels 2 --target <node name> /dev/zero` for a sink and `pw-cat --record --raw ... --target <node name> /dev/null` for a source. First check `pw-cat --help` lists `--raw` on the installed version; if it does not, feed a generated silent WAV on stdin instead and note it in the commit. Digital silence only: nothing audible may reach an output.

Behaviour: `RunTest` records `Found` (as Task 9), starts `Silence` for `Kind::InterfaceOnly`, creates the `Sweep` from `ladder(buffer_only)` and the profile's `hold_secs`, and applies each `Action::Apply`. Every snapshot goes to `Sweep::sample`. `Done`, `Abort` and `StopTest` all drop `Silence` and restore `Found`; `Done` also pushes a `Run` to the profile (date as ISO 8601, kernel from `/proc/sys/kernel/osrelease`, PipeWire version from `pw-top --version`). The mode keys are disabled while a test runs. `Interface only` is greyed with `pw-cat is not installed.` when `silence_available()` is false. `With my session` shows `Start your session playing first.` if the interface is idle when Run is pressed, and does not start. The run-time estimate on the button is `steps * (hold + SETTLE_SECS)` seconds, rounded up to minutes. `Copy results` writes `text_report` to the clipboard.

- [ ] **Step 1: Write failing tests:** `report_is_a_plain_table` (header row, one line per step, kernel, PipeWire version and CPU model, no user name or path); in `latency.rs`: `stop_restores_what_was_found`; `abort_restores_and_shows_the_reason`; `done_stores_the_run_in_the_profile`; `modes_are_locked_during_a_test`; `session_test_refuses_to_start_on_an_idle_interface`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement.** **Step 4: Run**, PASS, fmt, clippy on both crates.
- [ ] **Step 5: Build and look** (no test run started without Danny), screenshot.
- [ ] **Step 6: Commit** `Latency: test run on the page`.

---

## Phase 4: System check and finish

### Task 15: System check

**Files:**
- Create: `crates/idctl-audio/src/syscheck.rs`
- Modify: `src/lib.rs`, `crates/idctl-gui/src/latency.rs`, `views/latency.rs`, `README.md`, `HANDOVER.md`

**Interfaces:**
- Produces:
  ```rust
  #[derive(PartialEq, Debug)] pub enum Status { Ok, Warn, Unknown }
  pub struct Check { pub status: Status, pub title: String, pub advice: String }
  #[derive(Default)]
  pub struct Facts {
      pub pipewire_version: Option<String>,
      pub data_thread_policy: Option<String>, // "SCHED_FIFO 88" style, from /proc/<pid>/task/*/stat of `data-loop.0`
      pub rtprio_limit: Option<u64>, pub memlock_unlimited: Option<bool>,   // /proc/self/limits
      pub kernel_version: Option<String>,     // /proc/version: contains PREEMPT_RT, PREEMPT_DYNAMIC or PREEMPT
      pub cmdline: Option<String>,            // /proc/cmdline: threadirqs, preempt=
      pub governor: Option<String>,           // cpu0 scaling_governor
      pub usb_power_control: Option<String>,  // the interface's power/control: "on" or "auto"
      pub usb_siblings: Vec<String>,          // other devices on the same root hub
      pub profile: Option<String>,
      pub conflicts: Vec<PathBuf>,
  }
  pub fn checks(facts: &Facts) -> Vec<Check>;       // pure, fixed order
  pub fn read(iface: Option<&Interface>, config_home: &Path) -> Facts;
  ```

One check per bullet in the spec's System check list, in that order. A missing fact gives `Unknown` with the title and `Could not be read.`, never `Warn`. `PREEMPT_RT` makes the IRQ-threading check `Ok` without `threadirqs`. Advice is one sentence saying what to change and that it needs administrator rights where it does; the app runs nothing.

- [ ] **Step 1: Write failing tests:** for each check, one test per status it can return (for example `irq_threading_ok_with_threadirqs`, `irq_threading_ok_on_rt_kernel`, `irq_threading_warns_without`, `irq_threading_unknown_without_cmdline`); `empty_facts_are_all_unknown`; `checks_keep_their_order`.
- [ ] **Step 2: Run**, FAIL. **Step 3: Implement** `checks`, then `read`, then the System block on the page (refreshed when the tab opens). **Step 4: Run**, PASS, fmt, clippy.
- [ ] **Step 5: Update** `README.md` (a short Latency section: what it shows, that changes are per-user and revertible, PipeWire required) and `HANDOVER.md` (structure, what is untested on hardware, that these merges had no independent review).
- [ ] **Step 6: Commit** `Latency: system check, README and handover`.

### Task 16: Hardware walk-through (Danny at the machine)

Not code. Each item is ticked only when seen on the iD2+.

- [ ] Each mode applies; Now shows the new buffer; Off restores what was there before.
- [ ] Keep writes the two files; Revert removes them and leaves other files alone.
- [ ] Kill the app (`kill -9`) while a mode is on; start it again; the force is gone.
- [ ] `Interface only` test runs to the end with nothing audible, and suggests a step.
- [ ] `With my session` test with the DAW playing; note which side glitches at each step.
- [ ] Unplug the interface during a test: it aborts with a reason, the app stays up.
- [ ] Export the profile; add it as `profiles/<machine>.toml`; import it back.
- [ ] System check lines match the machine.
- [ ] Commit the example profile and any fixes: `Latency: example profile for the development machine`.
