use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, RwLock};
use std::sync::atomic::AtomicBool;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use crate::cache::Cache;

pub mod tcp_server;
mod tcp_session_handler;
mod rudish_session_handler;

pub trait Server<T> {
     fn run(&self) -> std::io::Result<()>;
     fn close(&self) -> std::io::Result<()>;
     fn new(config: T, terminate: Arc<AtomicBool>) -> Self;
}

pub fn extract_string(data: &[u8]) -> String {
    // Extract bytes up to the first 0 byte (null terminator)
    let trimmed_data = data.iter().take_while(|&&b| b != 0).cloned().collect::<Vec<u8>>();
    String::from_utf8_lossy(&trimmed_data).to_string()
}

fn compress_to_binary(input: &str) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    if let Err(e) = encoder.write_all(input.as_bytes()) {
        eprintln!("Compression failed: {}", e);
        return Vec::new(); // Return empty on error
    }
    encoder.finish().unwrap_or_default() // Fallback to empty vector on failure
}

fn decompress_binary(input: &[u8]) -> String {
    let mut decoder = ZlibDecoder::new(input);
    let mut buffer = Vec::new();
    if let Err(e) = decoder.read_to_end(&mut buffer) {
        eprintln!("Decompression failed: {}", e);
        return String::new(); // Return empty string on error
    }
    String::from_utf8_lossy(&buffer).to_string()
}

pub(crate) const BUFFER_SIZE: usize = 2048;