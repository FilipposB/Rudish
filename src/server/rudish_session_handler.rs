use crate::cache::Cache;
use crate::server::{extract_string, BUFFER_SIZE};
use std::collections::VecDeque;
use std::io::Write;
use std::net::TcpStream;
use std::ops::Add;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
enum Signal {
    Hello,
    Put(u8),
    Get,
    None,
}

pub struct RudishSessionHandler {
    handler_id: usize,
    cache: Arc<RwLock<Cache>>,
    master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
}

impl RudishSessionHandler {
    pub fn new(
        handler_id: usize,
        cache: Arc<RwLock<Cache>>,
        master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
    ) -> Self {
        Self {
            handler_id,
            cache,
            master_buffer,
        }
    }
    pub fn run(self, should_terminate: Arc<AtomicBool>, master_buffer_size: Arc<AtomicUsize>, mut stream: TcpStream) {
        let mut data: VecDeque<u8> = VecDeque::new();
        let mut required_bytes: usize = 1;
        let mut signal: Signal= Signal::None;

        while !should_terminate.load(Ordering::Relaxed) {
            let mut buffer = [0u8; BUFFER_SIZE];

            let bytes = {
                let mut locked_master_buffer = self.master_buffer.write().unwrap();
                let current_size = master_buffer_size.load(Ordering::Relaxed);

                if current_size == 0 {
                    drop(locked_master_buffer);
                    thread::sleep(Duration::from_micros(10));
                    continue;
                }

                let to_copy = buffer.len().min(current_size);

                buffer[..to_copy].copy_from_slice(&locked_master_buffer[..to_copy]);

                let remaining = current_size - to_copy;
                locked_master_buffer.copy_within(to_copy..current_size, 0);

                locked_master_buffer[remaining..current_size].fill(0);

                master_buffer_size.store(remaining, Ordering::Relaxed);

                to_copy
            };


            data.extend(buffer[..bytes].iter().cloned());

            if data.len() >= required_bytes {
                process(&mut data, signal.clone(), self.cache.clone(), &mut stream).map(|x |{
                    signal = x.1;
                    required_bytes = x.0;
                });
            }
        }
    }
}

fn process(data: &mut VecDeque<u8>, signal: Signal, cache: Arc<RwLock<Cache>>, stream: &mut TcpStream) -> Option<(usize, Signal)> {

    let mut current_signal = signal;

    if data.is_empty() {
        return None;
    }

    loop {
        match current_signal {
            //No previous signal
            Signal::None => {

                if data.is_empty() {
                    return Some((1, Signal::None));
                }

                let first_byte = data.pop_front().unwrap();
                let intention_bits = (first_byte & 0b1100_0000) >> 6;
                current_signal = match intention_bits {
                    0b01 => Signal::Put(first_byte.clone()),
                    0b10 => Signal::Get,
                    _ => Signal::None,
                };

                continue;
            }
            Signal::Put(value) => {
                let second_top_bits = ((value & 0b0011_0000) >> 4) ;

                let mut needed_bytes = 2;

                if data.len() < needed_bytes {
                    return Some((needed_bytes, current_signal));
                }

                let topic_len = data.get(0).copied().unwrap_or(0) as usize;
                let key_len = data.get(1).copied().unwrap_or(0) as usize;

                let topic_position = (needed_bytes, needed_bytes + topic_len);
                let key_position = (topic_position.1, topic_position.1 + key_len);

                 needed_bytes = topic_len + key_len + needed_bytes + second_top_bits as usize;

                if data.len() < needed_bytes {
                    return Some((needed_bytes, current_signal));
                }

                let value_size = data
                    .iter()
                    .skip(needed_bytes-1)
                    .take(second_top_bits as usize)
                    .fold(0usize, |acc, &b| (acc << 8) | b as usize);

                let header_end = needed_bytes + second_top_bits as usize -1;
                let expected_value_len = value_size.add(header_end);

                if data.len() < expected_value_len {
                    return Some((expected_value_len, current_signal));
                }

                let mut str_value = Vec::with_capacity(value_size);

                for i in header_end..expected_value_len {
                    str_value.push(data[i]);
                }

                let contiguous = data.make_contiguous();
                let topic = extract_string(&contiguous[topic_position.0..topic_position.1]);
                let key = extract_string(&contiguous[key_position.0..key_position.1]);

                let data_string = extract_string(&str_value);

                println!("topic: {:?}, key: {:?}, data_string: {:?}", topic, key, data_string);

                cache.try_write().unwrap().put(&topic, &key, &data_string);


                for _ in 0..expected_value_len {
                    data.pop_front();
                }

                current_signal = Signal::None
            }
            Signal::Get => {

                let mut needed_bytes = 2;

                if data.len() < needed_bytes {
                    return Some((needed_bytes, current_signal));
                }

                let topic_len = data.get(0).copied().unwrap_or(0) as usize;
                let key_len = data.get(1).copied().unwrap_or(0) as usize;

                let topic_position = (needed_bytes, needed_bytes + topic_len);
                let key_position = (topic_position.1, topic_position.1 + key_len);

                needed_bytes = topic_len + key_len + needed_bytes;

                if data.len() < needed_bytes {
                    return Some((needed_bytes, current_signal));
                }

                let contiguous = data.make_contiguous();


                let topic = extract_string(&contiguous[topic_position.0..topic_position.1]);
                let key = extract_string(&contiguous[key_position.0..key_position.1]);

                let result = cache.read().unwrap().get(&*topic, &*key);


                match result {
                    Ok(value) => {

                        println!("topic: {:?}, key: {:?}", topic, key);


                        stream.write(value.as_bytes()).unwrap();
                    }
                    _ => {}
                }
                data.drain(0..key_position.1);


                current_signal = Signal::None
            }
            _ => {}
        }

    }

}
