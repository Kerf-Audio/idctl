//! iD Mixer: Linux control panel for the iD2+ audio interface.

mod channel_dsp;
mod latency;
mod mix;
mod model;
mod routing;
mod talkback;
mod theme;
mod views;
mod watch;
mod widgets;
mod worker;

use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iced::{Element, Size, Subscription, Task};

use channel_dsp::{ChannelDsp, DspEdit, DSP_CHANNELS};
use idctl_protocol::codec;
use idctl_protocol::dsp::{self, comp};
use idctl_protocol::params::{input, mixer, output, ANALOGUE_INPUTS};
use idctl_protocol::ParamAddr;
use mix::{Edit, Writes, NODES, REVERB_MIX};
use model::Model;
use talkback::Talkback;
use worker::{Command, Event};

const TICK: Duration = Duration::from_millis(33);

#[derive(Debug, Clone)]
pub enum Message {
    Tick,
    Tab(Tab),
    SelectMix(usize),
    Flag(ParamAddr, bool),
    Byte(ParamAddr, u8),
    Db(ParamAddr, f32),
    Unit(ParamAddr, f32),
    Hz(ParamAddr, f32),
    Master(usize, f32),
    Phones(f32),
    /// Headphone source: base sample-bus id of the stereo pair.
    PhonesSource(u8),
    SampleRate(u32),
    SampleRateSet(Result<(), String>),
    /// Mixer node of the talkback mic, or none.
    TalkbackSource(Option<usize>),
    /// GLOW: HMX and LIFT together on one preamp channel.
    Glow(u8, bool),
    Level(usize, f32),
    Pan(usize, f32),
    Mute(usize),
    Solo(usize),
    Link(usize),
    /// Reverb send level of a node.
    Send(usize, f32),
    /// Reverb send on or off.
    SendOn(usize),
    /// Reverb return on or off in the selected mix.
    ReturnOn(bool),
    Dsp(u8, DspEdit),
    /// Compressor attack or release in ms.
    Time(ParamAddr, f32),
    Ratio(u8, f32),
    CompOn(u8, bool),
    Open(Editor),
    Close,
    Routing(routing::Msg),
    Latency(latency::Msg),
    /// Drag of the monitoring bar: grab, new rack height, release.
    RackGrab,
    RackDrag(f32),
    RackRelease,
    /// A vertical wheel over the strips that nothing else took.
    StripsWheel(iced::mouse::ScrollDelta),
    /// +48V key pressed: switching on needs confirmation.
    Phantom(u8, bool),
    PhantomAnswer(bool),
    CloseRequested,
}

impl Message {
    /// Whether handling the message writes to the device.
    fn writes_device(&self) -> bool {
        !matches!(
            self,
            Self::Tick
                | Self::Tab(_)
                | Self::SelectMix(_)
                | Self::SampleRate(_)
                | Self::SampleRateSet(_)
                | Self::Latency(_)
                | Self::Open(_)
                | Self::Close
                | Self::Routing(routing::Msg::Page(_))
                | Self::RackGrab
                | Self::RackDrag(_)
                | Self::RackRelease
                | Self::StripsWheel(_)
                | Self::PhantomAnswer(_)
                | Self::CloseRequested
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mixer,
    Routing,
    Settings,
    Latency,
}

/// Full-size processor editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editor {
    Eq(u8),
    Comp(u8),
    Reverb,
}

pub struct App {
    model: Model,
    tx: Sender<Command>,
    rx: Receiver<Event>,
    /// `None` while connected, otherwise why not.
    offline: Option<String>,
    /// Why the last sample-rate change failed.
    clock_error: Option<String>,
    tab: Tab,
    mix: usize,
    dsp: [ChannelDsp; DSP_CHANNELS],
    /// Processor editor shown over the mixer.
    editor: Option<Editor>,
    routing: routing::State,
    talkback: Talkback,
    /// Height of the card rack above the monitoring section.
    rack: f32,
    /// The monitoring bar is being dragged.
    resizing_rack: bool,
    /// Mic input waiting for the user to confirm phantom power.
    confirm_phantom: Option<u8>,
    /// PipeWire's graph rate is forced by us.
    rate_forced: bool,
    latency: latency::State,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = worker::spawn();
        Self {
            model: Model::default(),
            tx,
            rx,
            offline: Some("connecting".into()),
            clock_error: None,
            tab: Tab::Mixer,
            mix: 0,
            dsp: [ChannelDsp::default(); DSP_CHANNELS],
            editor: None,
            routing: routing::State::default(),
            talkback: Talkback::default(),
            rack: views::layout::SECTIONS,
            resizing_rack: false,
            confirm_phantom: None,
            rate_forced: false,
            latency: latency::State::default(),
        }
    }

    fn write(&mut self, addr: ParamAddr, data: Vec<u8>) {
        if self.offline.is_some() {
            return;
        }
        self.model.assume(addr, &data);
        self.send(Command::Write(addr, data));
    }

    fn send(&mut self, cmd: Command) {
        if self.tx.send(cmd).is_err() {
            self.offline = Some("device thread stopped".into());
        }
    }

    /// Let the device thread apply what is queued and stop, and give the
    /// PipeWire rate back if we forced it. True when the queue was written.
    fn shutdown(&mut self) -> bool {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        let written = self.tx.send(Command::Quit(ack_tx)).is_ok()
            && ack_rx.recv_timeout(Duration::from_secs(1)) == Ok(true);
        if self.rate_forced {
            idctl_audio::pw::release_rate();
        }
        written
    }

    fn write_all(&mut self, writes: Writes) {
        for (addr, data) in writes {
            self.write(addr, data);
        }
    }

    fn mixer_edit(&mut self, node: usize, edit: Edit) {
        self.mixer_edit_in(node, self.mix, edit);
    }

    fn mixer_edit_in(&mut self, node: usize, mix: usize, edit: Edit) {
        self.talkback.adopt(node, mix, &edit);
        let writes = self.model.mixer.edit(node, mix, edit);
        self.write_all(writes);
    }

    fn dsp_edit(&mut self, ch: u8, edit: DspEdit) {
        for w in self.dsp[usize::from(ch)].edit(ch, edit) {
            match w {
                dsp::Write::Set(addr, data) => self.write(addr, data),
                dsp::Write::Mem(addr, data) => {
                    self.send(Command::Mem(addr, data));
                }
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Connected => {
                    self.offline = None;
                    self.model.mixer.reset_seen();
                }
                Event::Disconnected(why) => {
                    self.offline = Some(why);
                    self.confirm_phantom = None;
                }
                Event::Param(addr, data) => self.model.apply(addr, data),
                Event::Loaded => {
                    self.model.mixer.detect_links();
                    self.talkback.resume();
                    let model = &self.model;
                    self.routing.detect(|slot| model.byte(output::source(slot)));
                }
                Event::Meters(group, levels) => self.model.meters(group, levels),
            }
        }
        self.follow_talkback();
    }

    fn follow_talkback(&mut self) {
        let on = self.model.talking();
        let writes = self.talkback.follow(&mut self.model.mixer, on);
        self.write_all(writes);
        self.sync_talkback_file();
    }

    /// Keep the held talkback sends on disk while they are held. A release
    /// only counts once the device was there to receive it.
    fn sync_talkback_file(&mut self) {
        match self.talkback.take_change() {
            Some(Some(held)) => talkback::store(Some(&held)),
            Some(None) if self.offline.is_none() => talkback::store(None),
            _ => {}
        }
    }
}

fn update(app: &mut App, msg: Message) -> Task<Message> {
    if app.offline.is_some() && msg.writes_device() {
        return Task::none();
    }
    let db = |v: f32| codec::encode_db(v).to_vec();
    match msg {
        Message::Tick => app.drain_events(),
        Message::Tab(t) => {
            app.tab = t;
            if t == Tab::Latency {
                return latency_update(app, latency::Msg::Opened);
            }
        }
        Message::Latency(m) => return latency_update(app, m),
        Message::SelectMix(m) => app.mix = m,
        Message::Flag(a, on) => {
            app.write(a, vec![u8::from(on)]);
            if let Some(ch) = (0..ANALOGUE_INPUTS).find(|&ch| input::hpf(ch) == a) {
                let hz = app.dsp[usize::from(ch)].hpf_hz;
                app.dsp_edit(ch, DspEdit::HpfHz(hz, on));
            }
            app.follow_talkback();
        }
        Message::Byte(a, v) => app.write(a, vec![v]),
        Message::TalkbackSource(source) => {
            let writes = app.talkback.follow(&mut app.model.mixer, false);
            app.write_all(writes);
            app.talkback.source = source;
            app.follow_talkback();
        }
        Message::Db(a, v) => app.write(a, db(v)),
        Message::Unit(a, v) => app.write(a, codec::encode_unit(v).to_vec()),
        Message::Hz(a, hz) => app.write(a, (hz.round() as u16).to_le_bytes().to_vec()),
        Message::Glow(ch, on) => {
            app.write(input::hmx(ch), vec![u8::from(on)]);
            app.write(input::lift(ch), vec![u8::from(on)]);
        }
        Message::Master(m, v) => {
            app.write(mixer::master(2 * m as u8), db(v));
            app.write(mixer::master(2 * m as u8 + 1), db(v));
        }
        Message::Phones(v) => {
            for slot in watch::PHONES {
                app.write(output::level(slot), db(v));
            }
        }
        Message::PhonesSource(base) => {
            for (side, slot) in (0..).zip(watch::PHONES) {
                app.write(output::source(slot), vec![base + side]);
            }
        }
        Message::SampleRate(hz) => {
            return Task::perform(idctl_audio::pw::force_rate(hz), Message::SampleRateSet);
        }
        Message::SampleRateSet(result) => {
            app.rate_forced |= result.is_ok();
            app.clock_error = result.err();
        }
        Message::Phantom(ch, on) => {
            if on {
                app.confirm_phantom = Some(ch);
            } else {
                app.write(input::phantom(ch), vec![0]);
            }
        }
        Message::PhantomAnswer(yes) => {
            if let Some(ch) = app.confirm_phantom.take() {
                if yes {
                    app.write(input::phantom(ch), vec![1]);
                }
            }
        }
        Message::CloseRequested => {
            // Put the talkback mic back before the device thread stops.
            let writes = app.talkback.follow(&mut app.model.mixer, false);
            app.write_all(writes);
            if app.shutdown() {
                app.sync_talkback_file();
            }
            return iced::exit();
        }
        Message::Level(n, v) => app.mixer_edit(n, Edit::Level(v)),
        Message::Pan(n, v) => app.mixer_edit(n, Edit::Pan(v)),
        Message::Mute(n) => app.mixer_edit(n, Edit::Mute),
        Message::Solo(n) => app.mixer_edit(n, Edit::Solo),
        Message::ReturnOn(on) => {
            for n in [NODES - 2, NODES - 1] {
                if app.model.mixer.send(n, app.mix).mute == on {
                    app.mixer_edit(n, Edit::Mute);
                }
            }
        }
        Message::Link(pair) => {
            let writes = app.model.mixer.toggle_link(pair);
            app.write_all(writes);
        }
        Message::Send(n, v) => app.mixer_edit_in(n, REVERB_MIX, Edit::Level(v)),
        Message::SendOn(n) => {
            let send = app.model.mixer.send(n, REVERB_MIX);
            if send.level <= mix::OFF_DB {
                app.mixer_edit_in(n, REVERB_MIX, Edit::Level(0.0));
                if send.mute {
                    app.mixer_edit_in(n, REVERB_MIX, Edit::Mute);
                }
            } else {
                app.mixer_edit_in(n, REVERB_MIX, Edit::Mute);
            }
        }
        Message::Dsp(ch, edit) => app.dsp_edit(ch, edit),
        Message::Time(a, ms) => app.write(a, comp::encode_time(ms).to_vec()),
        Message::Ratio(ch, r) => app.write(comp::ratio(ch), comp::encode_ratio(r).to_vec()),
        Message::CompOn(ch, on) => app.write(comp::mix(ch), comp::encode_mix(on, 1.0).to_vec()),
        Message::Open(e) => app.editor = Some(e),
        Message::Close => app.editor = None,
        Message::RackGrab => app.resizing_rack = true,
        Message::RackDrag(height) => app.rack = height.max(views::layout::RACK_MIN),
        Message::RackRelease => app.resizing_rack = false,
        Message::StripsWheel(delta) => {
            // Same speed as a scrollable's own wheel: 60 px per line.
            let y = match delta {
                iced::mouse::ScrollDelta::Lines { y, .. } => y * 60.0,
                iced::mouse::ScrollDelta::Pixels { y, .. } => y,
            };
            return iced::widget::operation::scroll_by(
                views::STRIPS,
                iced::widget::scrollable::AbsoluteOffset { x: -y, y: 0.0 },
            );
        }
        Message::Routing(m) => {
            let writes = routing::update(&mut app.routing, m);
            app.write_all(writes);
        }
    }
    Task::none()
}

fn latency_update(app: &mut App, msg: latency::Msg) -> Task<Message> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    app.latency.update(msg, now).map(Message::Latency)
}

fn view(app: &App) -> Element<'_, Message> {
    views::root(app)
}

fn subscription(app: &App) -> Subscription<Message> {
    // pw-top runs only while its page is showing.
    let latency = if app.tab == Tab::Latency {
        latency::feed().map(|f| Message::Latency(latency::Msg::Feed(f)))
    } else {
        Subscription::none()
    };
    Subscription::batch([
        iced::time::every(TICK).map(|_| Message::Tick),
        iced::window::close_requests().map(|_| Message::CloseRequested),
        latency,
    ])
}

fn main() -> iced::Result {
    iced::application(App::new, update, view)
        .subscription(subscription)
        .theme(|_: &App| theme::theme())
        .title("iD Mixer")
        .window(iced::window::Settings {
            size: Size::new(views::layout::DEFAULT_WIDTH, views::layout::DEFAULT_HEIGHT),
            min_size: Some(Size::new(
                views::layout::MIN_WIDTH,
                views::layout::MIN_HEIGHT,
            )),
            platform_specific: iced::window::settings::PlatformSpecific {
                application_id: "id-mixer".to_string(),
                ..Default::default()
            },
            exit_on_close_request: false,
            ..iced::window::Settings::default()
        })
        .run()
}
