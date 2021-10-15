/*
https://github.com/snapview/tokio-tungstenite/tree/master/examples
https://docs.rs/tokio-tungstenite/0.15.0/tokio_tungstenite/index.html
https://github.com/tokio-rs/tokio
https://tokio.rs/tokio/tutorial/io
*/
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use futures_util::SinkExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;
use tokio::time::{sleep, Duration};

use std::io::{stdin, stdout, Write};
use std::error::Error;

use midir::{MidiInput, Ignore};

type PianoString = Arc<Mutex<Message>>;

#[tokio::main]
async fn main() {
    let state = PianoString::new(Mutex::new(Message::Text(String::from_utf8(vec![b'0'; 88]).unwrap())));
    
    let midi_task = tokio::spawn(midi_routine(state.clone()));
    sleep(Duration::from_millis(1000)).await;
    let addr = "127.0.0.1:3012";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let addr = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", addr);

        tokio::spawn(accept_connection(state.clone(),addr, stream));
    }
    midi_task.abort();
}

/*
************Web socket handling************
*/

async fn accept_connection(piano_string: PianoString, addr: SocketAddr, stream: TcpStream) {
    handle_connection(piano_string, addr, stream).await; 
}

async fn handle_connection(piano_string: PianoString, addr: SocketAddr, stream: TcpStream) {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    println!("New WebSocket connection: {}", addr);
    let mut old_piano_string = piano_string.lock().unwrap().clone();
    loop {
        let current_piano_string = piano_string.lock().unwrap().clone();
        if old_piano_string != current_piano_string{
            ws_stream.send(current_piano_string.clone()).await.unwrap_or_default();
            old_piano_string = current_piano_string;
        }
        sleep(Duration::from_millis(1)).await;
    }
    //println!("{} disconnected", addr);
}


/*
*************midi stuff below*************
*/
enum MidiCommand{
    KeyDown(u8,u8),
    KeyUp(u8,u8),
    Pedals(u8,u8),
    Unknown
}

impl MidiCommand {
    fn new(command:&[u8]) -> MidiCommand
    {
        match command[0]{
            128 => MidiCommand::KeyUp(command[1],command[2]), 
            144 => MidiCommand::KeyDown(command[1],command[2]),
            176 => MidiCommand::Pedals(command[1],command[2]),
            _ => MidiCommand::Unknown,
        }
    }
}

async fn midi_routine(piano_string: PianoString)
{
    if let Err(e) = read_midi(piano_string).await {
        println!("{}\nConnect your midi device(s) and re-execute this program.",e); 
    } 
}

async fn read_midi(piano_string:PianoString) -> Result<(), Box<dyn Error>>{
    let mut input = String::new();
    
    let mut midi_in = MidiInput::new("midir reading input")?;
    midi_in.ignore(Ignore::None);
    
    // Get an input port (read from console if multiple are available)
    loop {
        if midi_in.ports().len() > 0{
            break;
        }
        println!("No midi device found, connect a midi device to your computer to continue.\nRetrying in 10 seconds...");
        sleep(Duration::from_millis(10000)).await;
    };
    let in_ports = midi_in.ports();
    let in_port = match in_ports.len() {
        0 => return Err("no midi input port found".into()),
        1 => {
            println!("Choosing the only available input port: {}", midi_in.port_name(&in_ports[0]).unwrap());
            &in_ports[0]
        },
        _ => {
            println!("\nAvailable input ports:");
            for (i, p) in in_ports.iter().enumerate() {
                println!("{}: {}", i, midi_in.port_name(p).unwrap());
            }
            print!("Please select input port: ");
            stdout().flush()?;
            let mut input = String::new();
            stdin().read_line(&mut input)?;
            in_ports.get(input.trim().parse::<usize>()?)
                     .ok_or("invalid input port selected")?
        }
    };
    
    println!("\nOpening connection");

    let mut piano_char_vec = vec![b'0'; 90];
    
    let in_port_name = midi_in.port_name(in_port)?;
    // _conn_in needs to be a named parameter, because it needs to be kept alive until the end of the scope
    let _conn_in = midi_in.connect(in_port, "midir-read-input", move |_stamp, message, _| {
        if message.len() > 1{
            let command = MidiCommand::new(message);
            let normalize = |value:u8| {(((value as f32)/127.0*8.0) as u8).to_string().as_bytes()[0]};
            match command{
                MidiCommand::KeyDown(key,vel) => piano_char_vec[(key-21) as usize] = normalize(vel)+1,
                MidiCommand::KeyUp(key,_vel) => piano_char_vec[(key-21) as usize] = b'0',
                MidiCommand::Pedals(pedal, vel) => match pedal{
                    64 => piano_char_vec[88] = normalize(vel),
                    66 => piano_char_vec[89] = normalize(vel),
                    _ => ()
                }
                _ => ()
            };
            let piano_string_from_array = String::from_utf8(piano_char_vec.clone()).expect("Error while converting u8 array to utf-8");
            println!("{} ; {:?}", piano_string_from_array, message);
            *piano_string.lock().unwrap() = Message::Text(piano_string_from_array);
        }
    }, ())?;
    
    println!("Connection open, reading input from '{}'", in_port_name);
    
    input.clear();
    loop {
        stdin().read_line(&mut input)?; //loop so that the thread doesn't stop running the callback.
    }
}