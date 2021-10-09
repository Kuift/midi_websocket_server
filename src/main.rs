/*
https://github.com/snapview/tokio-tungstenite/tree/master/examples
https://docs.rs/tokio-tungstenite/0.15.0/tokio_tungstenite/index.html
https://github.com/tokio-rs/tokio
https://tokio.rs/tokio/tutorial/io
*/
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
async fn accept_connection(peer: SocketAddr, stream: TcpStream) {
    handle_connection(peer, stream).await; 
}

async fn handle_connection(peer: SocketAddr, stream: TcpStream) {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    println!("New WebSocket connection: {}", peer);

    while let Some(msg) = ws_stream.next().await {
        let msg = msg.unwrap_or(tokio_tungstenite::tungstenite::protocol::Message::Text("".to_string()));
        if msg.is_text() || msg.is_binary() {
            ws_stream.send(msg).await.unwrap_or_default();
        }
    }
    println!("{} disconnected", peer);
}

#[tokio::main]
async fn main() {
    let addr = "127.0.0.1:3012";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    println!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", peer);

        tokio::spawn(accept_connection(peer, stream));
    }
}