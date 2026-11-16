//! Linux control panel for the Audient iD2+.

mod mix;
mod model;
mod theme;
mod views;
mod watch;
mod widgets;
mod worker;

use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use iced::{Element, Size, Subscription};

use idctl_protocol::codec;
use idctl_protocol::params::{input, mixer, output};
use idctl_protocol::ParamAddr;
use mix::{Edit, Writes};
use model::Model;
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
    /// GLOW: HMX and LIFT together on one preamp channel.
    Glow(u8, bool),
    Level(usize, f32),
    Pan(usize, f32),
    Mute(usize),
    Solo(usize),
    Link(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Mixer,
    Settings,
}

pub struct App {
    model: Model,
    tx: Sender<Command>,
    rx: Receiver<Event>,
    /// `None` while connected, otherwise why not.
    offline: Option<String>,
    tab: Tab,
    mix: usize,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = worker::spawn();
        Self {
            model: Model::default(),
            tx,
            rx,
            offline: Some("connecting".into()),
            tab: Tab::Mixer,
            mix: 0,
        }
    }

    fn write(&mut self, addr: ParamAddr, data: Vec<u8>) {
        self.model.assume(addr, &data);
        let _ = self.tx.send(Command::Write(addr, data));
    }

    fn write_all(&mut self, writes: Writes) {
        for (addr, data) in writes {
            self.write(addr, data);
        }
    }

    fn mixer_edit(&mut self, node: usize, edit: Edit) {
        let writes = self.model.mixer.edit(node, self.mix, edit);
        self.write_all(writes);
    }

    fn drain_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Connected => self.offline = None,
                Event::Disconnected(why) => self.offline = Some(why),
                Event::Param(addr, data) => self.model.apply(addr, data),
                Event::Loaded => self.model.mixer.detect_links(),
                Event::Meters(group, levels) => self.model.meters(group, levels),
            }
        }
    }
}

fn update(app: &mut App, msg: Message) {
    let db = |v: f32| codec::encode_db(v).to_vec();
    match msg {
        Message::Tick => app.drain_events(),
        Message::Tab(t) => app.tab = t,
        Message::SelectMix(m) => app.mix = m,
        Message::Flag(a, on) => app.write(a, vec![u8::from(on)]),
        Message::Byte(a, v) => app.write(a, vec![v]),
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
        Message::Level(n, v) => app.mixer_edit(n, Edit::Level(v)),
        Message::Pan(n, v) => app.mixer_edit(n, Edit::Pan(v)),
        Message::Mute(n) => app.mixer_edit(n, Edit::Mute),
        Message::Solo(n) => app.mixer_edit(n, Edit::Solo),
        Message::Link(pair) => {
            let writes = app.model.mixer.toggle_link(pair);
            app.write_all(writes);
        }
    }
}

fn view(app: &App) -> Element<'_, Message> {
    views::root(app)
}

fn subscription(_: &App) -> Subscription<Message> {
    iced::time::every(TICK).map(|_| Message::Tick)
}

fn main() -> iced::Result {
    iced::application(App::new, update, view)
        .subscription(subscription)
        .theme(|_: &App| theme::theme())
        .title("idctl")
        .window(iced::window::Settings {
            size: Size::new(views::layout::DEFAULT_WIDTH, views::layout::DEFAULT_HEIGHT),
            min_size: Some(Size::new(
                views::layout::MIN_WIDTH,
                views::layout::MIN_HEIGHT,
            )),
            ..iced::window::Settings::default()
        })
        .run()
}
