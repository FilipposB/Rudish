use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, RwLock};
use std::sync::atomic::AtomicBool;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use crate::cache::Cache;

pub mod udp_server;
pub mod tcp_server;

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

 fn treat(
     worker_id: u8,
     data: Vec<u8>,
     mut cache: Arc<RwLock<Cache>>,
     mut stream: &TcpStream,
 ) -> Option<Vec<u8>> {
    if let Some(first_byte) = data.get(0) {
        // Extract first 2 bits (highest bits in the first byte)
        let intention_bits = (first_byte & 0b1100_0000) >> 6;

        println!("[Worker {}] Received {} bytes", worker_id, intention_bits);

        let response = match intention_bits {
            0b00 => {
                println!("[Worker {}] Intention bits: {:02b} (Hello)", worker_id, intention_bits);
                Some(vec![0b01]) // "Hello" response
            }
            0b01 => {

                let second_top_bits = (first_byte & 0b0011_0000) >> 4;

                let mut data = vec![0u8; 2];
                if let Err(e) = stream.read_exact(&mut data) {
                    eprintln!("Failed to read extra bytes from stream: {:?}", e);
                    return None;
                }

                let topic_len = data.get(0).copied().unwrap_or(0) as usize;
                let key_len = data.get(1).copied().unwrap_or(0) as usize;

                let topic_position = (3, 3 + topic_len);
                let key_position = (topic_position.1, topic_position.1 + key_len);

                // Prevent panic by checking the bounds before slicing
                if data.len() < key_position.1 + second_top_bits as usize {
                    eprintln!("Insufficient data for header size bytes");
                    return None;
                }

                let size_bytes = &data[key_position.1..key_position.1 + second_top_bits as usize];
                let size = size_bytes.iter().fold(0usize, |acc, &b| (acc << 8) | b as usize);

                let header_end = key_position.1 + second_top_bits as usize;
                let expected_body_len = size.saturating_sub(header_end);

                let already_have = data.len().saturating_sub(header_end);
                let missing_bytes = expected_body_len.saturating_sub(already_have);

                // Read missing bytes if needed
                let extra = if missing_bytes > 0 {
                    let mut buf = vec![0u8; missing_bytes];
                    if let Err(e) = stream.read_exact(&mut buf) {
                        eprintln!("Failed to read extra bytes from stream: {:?}", e);
                        return None;
                    }
                    buf
                } else {
                    Vec::new()
                };

                // Assemble the body
                let mut body = Vec::with_capacity(expected_body_len);
                if already_have > 0 && header_end <= data.len() {
                    body.extend_from_slice(&data[header_end..]);
                }
                body.extend_from_slice(&extra);

                // Extract strings safely
                let topic = extract_string(&data[topic_position.0..topic_position.1]);
                let key = extract_string(&data[key_position.0..key_position.1]);
                let data_string = extract_string(&body);

                // Cache the result
                cache.write().unwrap().put(&*topic, &*key, &*data_string);

                println!(
                    "[Worker {}] Intention bits: {:02b} (Add Data)",
                    worker_id, intention_bits
                );

                None
            }
            0b10 => {

                println!("dd");

                // Read 2 bytes
                let mut buf = vec![0u8; 2];
                if let Err(e) = stream.read_exact(&mut buf) {
                    eprintln!("Failed to read extra bytes from stream: {:?}", e);
                    return None;
                }

                let topic_position = (0, buf[0] as usize);
                let key_position = (topic_position.1, topic_position.1 + buf[1] as usize);

                let mut buf = vec![0u8; topic_position.1 + buf[1] as usize];
                if let Err(e) = stream.read_exact(&mut buf) {
                    eprintln!("Failed to read extra bytes from stream: {:?}", e);
                    return None;
                }


                let topic = extract_string(&buf[topic_position.0..topic_position.1]);
                let key = extract_string(&buf[key_position.0..key_position.1]);

                let result = cache.read().unwrap().get(&*topic, &*key);
                let data = match result {
                    Ok(value) => {
                        println!("[Worker {}] Topic: {:?} Key: {:?}. Value {} Found", worker_id, topic, key, value);
                        value.as_bytes().to_vec()
                    }
                    Err(error) => {
                        println!("[Worker {}] Topic: {:?} Key: {:?}. {}", worker_id, topic, key, error);
                        key.as_bytes().to_vec() // Error marker
                    }
                };

                if !data.is_empty() {
                    Some(data)
                } else {
                    None
                }
            }
            0b11 => {
                println!("[Worker {}] Intention bits: {:02b} (Error)", worker_id, intention_bits);
                None
            }
            _ => {
                println!("[Worker {}] Intention bits: {:02b} (Unknown)", worker_id, intention_bits);
                None
            }
        };

        response
    } else {
        println!("[Worker {}] No data received.", worker_id);
        None
    }
}
