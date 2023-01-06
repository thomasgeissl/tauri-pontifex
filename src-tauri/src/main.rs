#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

extern crate midir;
extern crate rosc;

use std::error::Error;
use std::io::{stdin, stdout, Write};
use std::sync::mpsc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::thread::sleep;
use std::time::Duration;

use midir::{MidiOutput, MidiOutputConnection, MidiOutputPort};

use rosc::{OscBundle, OscMessage, OscPacket, OscType};
use std::env;
use std::net::{SocketAddrV4, UdpSocket};
use std::str::FromStr;
use tauri::Manager;
// use tokio::sync::mpsc;

// the payload type must implement `Serialize` and `Clone`.
#[derive(Clone, serde::Serialize)]
struct Payload {
    message: String,
}
struct OscPacketPayload {
    packet: OscPacket,
    port: i32,
}

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(name: &str) -> String {
    println!("hello from rust");
    format!("Hello, {}! You've been greeted from Rust!", name)
}

fn main() {
    let (osc_package_sender, oscPackageReceiver): (
        Sender<OscPacketPayload>,
        Receiver<OscPacketPayload>,
    ) = channel();

    tauri::Builder::default()
        .setup(|app| {
            // listen to the `event-name` (emitted on any window)
            let id = app.listen_global("event-name", |event| {
                println!("got event-name with payload {:?}", event.payload());
            });
            // unlisten to the event using the `id` returned on the `listen_global` function
            // an `once_global` API is also exposed on the `App` struct
            //   app.unlisten(id);

            tauri::async_runtime::spawn(async move {
                // A loop that takes output from the async process and sends it
                // to the webview via a Tauri Event
                let addr = match SocketAddrV4::from_str("127.0.0.1:9010") {
                    Ok(addr) => addr,
                    Err(_) => panic!("{}", "could not listen to port 9010"),
                };
                let sock = UdpSocket::bind(addr).unwrap();
                println!("Listening to {}", addr);

                let mut buf = [0u8; rosc::decoder::MTU];

                loop {
                    match sock.recv_from(&mut buf) {
                        Ok((size, addr)) => {
                            println!("Received packet with size {} from: {}", size, addr);
                            let (_, packet) = rosc::decoder::decode_udp(&buf[..size]).unwrap();
                            let payload = OscPacketPayload {
                                packet: packet,
                                port: 9010,
                            };
                            osc_package_sender.send(payload);
                            // send_cc(2,64);
                        }
                        Err(e) => {
                            println!("Error receiving from socket: {}", e);
                            break;
                        }
                    }
                }
            });


            tauri::async_runtime::spawn(async move {
                // create midi sender
                let mut midi_output: MidiOutputConnection;
                match create_midi_output() {
                    Ok(midi) => (midi_output = midi),
                    Err(err) => {
                        println!("Error: {}", err);
                        panic!("no midi output device! ");
                    }
                }
                let mut send_note_on = |note: u8, velocity: u8| {
                    const TYPE: u8 = 0x90;
                    let _ = midi_output.send(&[TYPE, note, velocity]);
                };
                let mut send_note_off = |note: u8, velocity: u8| {
                    const TYPE: u8 = 0x80;
                    let _ = midi_output.send(&[TYPE, note, velocity]);
                };
                let mut send_cc = |controller: u8, value: u8| {
                    const TYPE: u8 = 0xB0;
                    let _ = midi_output.send(&[TYPE, controller, value]);
                };

                let mut handle_osc_message = |port: i32, msg: OscMessage| {
                    if (port == 9010 && msg.addr == "/kls/io/crank") {
                        for arg in msg.args {
                            match arg {
                                OscType::Int(val) => {
                                    println!("int: {}", val)
                                }
                                OscType::Float(val) => {
                                    println!("float: {}", val);
                                    send_cc(2, val as u8);
                                }
                                _ => println!("type not yet implemented"),
                            }
                        }
                    }
                };

                loop {
                    match oscPackageReceiver.recv() {
                        Ok((payload)) => {
                            let packet = payload.packet;
                            match packet {
                                OscPacket::Message(msg) => {
                                    handle_osc_message(payload.port, msg);
                                }
                                OscPacket::Bundle(bundle) => {
                                    for message in bundle.content {
                                        match message {
                                            OscPacket::Message(msg) => {
                                                handle_osc_message(payload.port, msg);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });

            // emit the `event-name` event to all webview windows on the frontend
            app.emit_all(
                "event-name",
                Payload {
                    message: "Tauri is awesome!".into(),
                },
            )
            .unwrap();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("failed to run app");
}

fn handle_osc_packet(packet: OscPacket) {
    match packet {
        OscPacket::Message(msg) => {
            println!("OSC address: {}", msg.addr);
            println!("OSC arguments: {:?}", msg.args);
        }
        OscPacket::Bundle(bundle) => {
            println!("OSC Bundle: {:?}", bundle);
            // bundle.content.
        }
    }
}

// fn run() -> Result<(), Box<dyn Error>> {
fn create_midi_output() -> Result<MidiOutputConnection, Box<dyn Error>> {
    let midi_out = MidiOutput::new("midi_output")?;

    // Get an output port (read from console if multiple are available)
    let out_ports = midi_out.ports();
    let out_port: &MidiOutputPort = match out_ports.len() {
        0 => return Err("no output port found".into()),
        1 => {
            println!(
                "Choosing the only available output port: {}",
                midi_out.port_name(&out_ports[0]).unwrap()
            );
            &out_ports[0]
        }
        _ => {
            // println!("\nAvailable output ports:");
            // for (i, p) in out_ports.iter().enumerate() {
            //     println!("{}: {}", i, midi_out.port_name(p).unwrap());
            //     if(midi_out.port_name(p).unwrap() == "IAC Driver Bus 1"){
            //         out_ports.get(i).ok_or("could not find IAC driver")
            //     }
            // }
            // // print!("Please select output port: ");
            // // stdout().flush()?;
            // // let mut input = String::new();
            // // stdin().read_line(&mut input)?;
            // // out_ports.get(input.trim().parse::<usize>()?)
            // //          .ok_or("invalid output port selected")?
            &out_ports[0]
        }
    };

    println!("\nOpening connection");
    let mut conn_out = midi_out.connect(out_port, "midir-test")?;
    Ok(conn_out)
}
