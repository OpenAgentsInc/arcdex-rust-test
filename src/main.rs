//! A simple example of hooking up stdin/stdout to a WebSocket stream.
//!
//! This example will connect to a server specified in the argument list and
//! then forward all data read on stdin to the server, printing out all data
//! received on stdout.
//!
//! Note that this is not currently optimized for performance, especially around
//! buffer management. Rather it's intended to show an example of working with a
//! client.
//!
//! You can use this example together with the `server` example.

extern crate serde;
extern crate serde_json;

use arcdex::event::Event;
use std::env;

use futures_util::{future, pin_mut, StreamExt};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() {
    // Get the connection address from the first command line argument
    let connect_addr = env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("this program requires at least one argument"));

    // Parse the connection address as a URL
    let url = url::Url::parse(&connect_addr).unwrap();

    // Create a channel for sending data from stdin to the WebSocket
    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();

    // Spawn a task to read data from stdin and send it over the channel
    tokio::spawn(read_stdin(stdin_tx));

    // Connect to the WebSocket server
    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    // Split the WebSocket stream into a sink for sending data and a stream for receiving data
    let (write, read) = ws_stream.split();

    // Create a stream for forwarding data from stdin to the WebSocket
    let stdin_to_ws = stdin_rx.map(Ok).forward(write);

    // Create a stream for forwarding data from the WebSocket to stdout
    let ws_to_stdout = read.for_each_concurrent(None, |message| {
        let text = message.unwrap().into_text().unwrap();
        // let _event: Event = serde_json::from_str(&text).unwrap();
        // Do something with event, e.g. save it to a database
        async move {
            tokio::io::stdout()
                .write_all(&text.as_bytes())
                .await
                .unwrap();
        }
    });

    // Pin the streams to prevent them from being moved
    pin_mut!(stdin_to_ws, ws_to_stdout);

    // Wait for either stdin_to_ws or ws_to_stdout to complete and cancel the other
    // this allows the program to exit cleanly when stdin is closed or the websocket
    // connection is closed.
    future::select(stdin_to_ws, ws_to_stdout).await;
}

// Our helper method which will read data from stdin and send it along the
// sender provided.
async fn read_stdin(tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let mut stdin = tokio::io::stdin();

    // Generate a unique 8-character string to use as subscription ID
    let sub_id: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(8)
        .map(|x| x as char)
        .collect();

    // Send a message to the connected websocket server after connecting
    tx.unbounded_send(Message::text(format!(
        "[\"REQ\",\"{}\",{{\"kinds\":[0,1]}}]",
        sub_id
    )))
    .unwrap();

    // Continuously read data from stdin and send it as binary data over the websocket connection
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            // If there is an error reading from stdin or the read is empty, break out of the loop
            Err(_) | Ok(0) => break,
            // Otherwise, continue with the read
            Ok(n) => n,
        };
        // Truncate the buffer to the number of bytes read
        buf.truncate(n);
        // Send the binary data over the websocket connection
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}
