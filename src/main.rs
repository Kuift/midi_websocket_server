/*
https://github.com/snapview/tokio-tungstenite/tree/master/examples
https://docs.rs/tokio-tungstenite/0.15.0/tokio_tungstenite/index.html
https://github.com/tokio-rs/tokio
https://tokio.rs/tokio/tutorial/io
*/
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream, windows::named_pipe::PipeMode};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_tungstenite::accept_async;
use tokio::time::{sleep, Duration};

use std::io::{stdin, stdout, Write};
use std::error::Error;

use midir::{MidiInput, Ignore};

type Tx = UnboundedSender<tokio_tungstenite::tungstenite::Message>;
type PeerMap = Arc<Mutex<HashMap<SocketAddr, Tx>>>;

#[tokio::main]
async fn main() {
    let state = PeerMap::new(Mutex::new(HashMap::new()));
    
    let midiTask = tokio::spawn(midi_routine(state.clone()));
    sleep(Duration::from_millis(1000)).await;
    let addr = "localhost:3012";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let addr = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", addr);

        tokio::spawn(accept_connection(state.clone(),addr, stream));
    }
    midiTask.abort();
}

/*
************Web socket handling************
*/

async fn accept_connection(peer_map: PeerMap, addr: SocketAddr, stream: TcpStream) {
    handle_connection(peer_map, addr, stream).await; 
}

async fn handle_connection(peer_map: PeerMap, addr: SocketAddr, stream: TcpStream) {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    let (tx, rx) = unbounded_channel();
    peer_map.lock().unwrap().insert(addr, tx);

    println!("New WebSocket connection: {}", addr);

    while let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap_or(tokio_tungstenite::tungstenite::protocol::Message::Text("".to_string()));
        if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await.unwrap_or_default();
        }
    }
    println!("{} disconnected", addr);
    peer_map.lock().unwrap().remove(&addr);
}


fn server_send_msg_to_all_clients(msg: tokio_tungstenite::tungstenite::Message, peer_map: PeerMap){
    let peers = peer_map.lock().unwrap();

    let broadcast_recipients = peers.iter().map(|(_, ws_sink)| ws_sink);

    for recp in broadcast_recipients {
        recp.send(msg.clone()).expect("sending messages failed");
    }
}

/*
*************midi stuff below*************
*/
enum MidiCommand{
    KeyDown(u8,u8),
    KeyUp(u8,u8),
    Unknown
}

impl MidiCommand {
    fn new(command:&[u8]) -> MidiCommand
    {
        match command[0]{
            128 => MidiCommand::KeyUp(command[1],command[2]), 
            144 => MidiCommand::KeyDown(command[1],command[2]),
            _ => MidiCommand::Unknown,
        }
    }
}

async fn midi_routine(peer_map: PeerMap)
{
    read_midi(peer_map).unwrap();
}

fn read_midi<'a>(peer_map:PeerMap) -> Result<(), Box<dyn Error>>{
    let mut input = String::new();
    
    let mut midi_in = MidiInput::new("midir reading input")?;
    midi_in.ignore(Ignore::None);
    //TODO: make the port selection part into a separate function
    // Get an input port (read from console if multiple are available)
    let in_ports = midi_in.ports();
    let in_port = match in_ports.len() {
        0 => return Err("no input port found".into()),
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

    let mut piano_char_vec = vec![b'0'; 88];
    
    let in_port_name = midi_in.port_name(in_port)?;
    // _conn_in needs to be a named parameter, because it needs to be kept alive until the end of the scope
    let _conn_in = midi_in.connect(in_port, "midir-read-input", move |_stamp, message, _| {
        if message.len() > 1{
            let command = MidiCommand::new(message);
            match command{
                MidiCommand::KeyDown(key,vel) => piano_char_vec[(key-21) as usize] = (((vel as f32)/127.0*8.0+1.0) as u8).to_string().as_bytes()[0],
                MidiCommand::KeyUp(key,_vel) => piano_char_vec[(key-21) as usize] = b'0',
                _ => ()
            };
            let piano_string = String::from_utf8(piano_char_vec.clone()).expect("Error while converting u8 array to utf-8");
            println!("{} ; {:?}",piano_string , message);
            server_send_msg_to_all_clients(tokio_tungstenite::tungstenite::Message::Text(piano_string),peer_map.clone());
        }
    }, ())?;
    
    println!("Connection open, reading input from '{}' ", in_port_name);

    input.clear();
    loop {
        stdin().read_line(&mut input)?; // wait for next enter key press
    }

    println!("Closing connection");
    Ok(())
}