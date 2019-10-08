use crossbeam_channel::bounded;
use gdk::WindowTypeHint;
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindowBuilder, WindowType};
use jack::{AudioIn, AudioOut, Client, ClientOptions};

macro_rules! slider {
    ($title:expr) => {{
        let slider = gtk::Scale::new_with_range(gtk::Orientation::Vertical, 0., 1.2, 0.01);
        slider.set_inverted(true);
        slider.set_value(1.);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let label = gtk::LabelBuilder::new().label($title).build();
        container.pack_start(&label, false, false, 10);
        container.pack_start(&slider, true, true, 10);
        (container, slider)
    }};
}

enum Channel {
    AL,
    AR,
    BL,
    BR,
}

enum Message {
    Volume((Channel, f32)),
    Crossfade(f32),
}

struct State {
    a_l: f32,
    a_r: f32,
    b_l: f32,
    b_r: f32,
    crossfade: f32,
}

fn main() {
    let application = Application::new(
        Some("com.github.bennetthardwick.rust-mixer"),
        Default::default(),
    )
    .expect("failed to initialize GTK application");

    let (send_command, commands) = bounded::<Message>(10);

    application.connect_activate(move |app| {
        let window = ApplicationWindowBuilder::new()
            .application(app)
            .title("Mixer")
            .type_hint(WindowTypeHint::Utility)
            .default_width(350)
            .width_request(350)
            .default_height(800)
            .height_request(800)
            .resizable(false)
            .type_(WindowType::Toplevel)
            .build();

        let header = gtk::HeaderBarBuilder::new().title("Mixer").build();

        let container = gtk::Box::new(gtk::Orientation::Vertical, 50);
        let upper = gtk::Box::new(gtk::Orientation::Horizontal, 50);

        let (a_l, slider) = slider!("A - Left");
        {
            let send_command = send_command.clone();
            slider.connect_value_changed(move |scale| {
                send_command
                    .send(Message::Volume((Channel::AL, scale.get_value() as f32)))
                    .unwrap();
            });
        }

        let (a_r, slider) = slider!("A - Right");
        {
            let send_command = send_command.clone();
            slider.connect_value_changed(move |scale| {
                send_command
                    .send(Message::Volume((Channel::AR, scale.get_value() as f32)))
                    .unwrap();
            });
        }

        let (b_l, slider) = slider!("B - Left");
        {
            let send_command = send_command.clone();
            slider.connect_value_changed(move |scale| {
                send_command
                    .send(Message::Volume((Channel::BL, scale.get_value() as f32)))
                    .unwrap();
            });
        }

        let (b_r, slider) = slider!("B - Right");
        {
            let send_command = send_command.clone();
            slider.connect_value_changed(move |scale| {
                send_command
                    .send(Message::Volume((Channel::BR, scale.get_value() as f32)))
                    .unwrap();
            });
        }

        let label = gtk::LabelBuilder::new().label("Cross Fader").build();

        let fader = gtk::Scale::new_with_range(gtk::Orientation::Horizontal, -1., 1., 0.01);
        let send_command = send_command.clone();
        fader.connect_value_changed(move |scale| {
            if let Err(e) = send_command.try_send(Message::Crossfade(scale.get_value() as f32)) {
                println!("An error occurred in BL, {}", e);
            }
        });

        let fader_container = gtk::Box::new(gtk::Orientation::Vertical, 50);
        let fader_slider_container = gtk::Box::new(gtk::Orientation::Horizontal, 50);
        fader.set_value(0.);
        fader_slider_container.pack_start(&fader, true, true, 50);
        fader_container.pack_start(&label, false, false, 0);
        fader_container.pack_start(&fader_slider_container, false, false, 0);

        upper.pack_start(&a_l, true, true, 25);
        upper.pack_start(&a_r, true, true, 25);
        upper.pack_start(&b_l, true, true, 25);
        upper.pack_start(&b_r, true, true, 25);

        container.pack_start(&header, false, false, 0);
        container.pack_start(&upper, true, true, 0);
        container.pack_start(&fader_container, false, false, 50);

        window.add(&container);
        window.show_all();
    });

    let client = Client::new("rust_mixer", ClientOptions::NO_START_SERVER)
        .unwrap()
        .0;

    let in_spec = AudioIn::default();
    let out_spec = AudioOut::default();

    let a_l_port = client.register_port("A - Left", in_spec).unwrap();
    let a_r_port = client.register_port("A - Right", in_spec).unwrap();

    let b_l_port = client.register_port("B - Left", in_spec).unwrap();
    let b_r_port = client.register_port("B - Right", in_spec).unwrap();

    let mut out_l_port = client.register_port("Left", out_spec).unwrap();
    let mut out_r_port = client.register_port("Right", out_spec).unwrap();

    let mut state = State {
        a_l: 1.,
        a_r: 1.,
        b_l: 1.,
        b_r: 1.,
        crossfade: 0.,
    };

    let process = jack::ClosureProcessHandler::new(
        move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
            for message in commands.try_iter() {
                match message {
                    Message::Volume((port, amount)) => match port {
                        Channel::AL => {
                            state.a_l = amount;
                        }
                        Channel::AR => {
                            state.a_r = amount;
                        }
                        Channel::BL => {
                            state.b_l = amount;
                        }
                        Channel::BR => {
                            state.b_r = amount;
                        }
                    },
                    Message::Crossfade(amount) => {
                        state.crossfade = amount;
                    }
                }
            }

            let a_l = a_l_port.as_slice(ps);
            let a_r = a_r_port.as_slice(ps);

            let b_l = b_l_port.as_slice(ps);
            let b_r = b_r_port.as_slice(ps);

            let out_l = out_l_port.as_mut_slice(ps);
            let out_r = out_r_port.as_mut_slice(ps);

            for (((a_l, a_r), (b_l, b_r)), (l, r)) in a_l
                .iter()
                .zip(a_r.iter())
                .zip(b_l.iter().zip(b_r.iter()))
                .zip(out_l.iter_mut().zip(out_r.iter_mut()))
            {
                *l = (a_l * state.a_l) + (b_l * state.b_l);
                *r = (a_r * state.a_r) + (b_r * state.b_r);
            }

            jack::Control::Continue
        },
    );

    let active = client.activate_async((), process).unwrap();
    application.run(&[]);
    active.deactivate().unwrap();
}
