use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::{io};
use rudish::server::Server;
use rudish::server::tcp_server::{TCPServer, TCPServerSettings};


fn main() -> io::Result<()> {
    let should_stop = Arc::new(AtomicBool::new(false));
    let stop_flag = should_stop.clone();
    tracing_subscriber::fmt()
        .with_thread_names(true)
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Register Ctrl+C handler
    ctrlc::set_handler(move || {
        stop_flag.store(true, Ordering::SeqCst);
        println!("\nShutting down gracefully...");
    }).expect("Error setting Ctrl+C handler");

    let server = TCPServer::new(TCPServerSettings::new(8080, 50), should_stop.clone());

    if let Err(e) = server.run() {
        eprintln!("Server error: {}", e);
    }

    Ok(())
}