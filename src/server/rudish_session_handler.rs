use crate::cache::Cache;
use crate::server::{BUFFER_SIZE};
use std::collections::VecDeque;
use std::ops::Add;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use rudish_lib::{drain_from_master_buffer, extract_string, safe_fill, PING, Signal};
use tracing::{trace};

fn add_get_data(data: &[u8]) -> Vec<u8> {
    let data_len = data.len();

    let bytes_needed =  ((data_len as f64).log2().ceil() as usize + 7) / 8;

    if bytes_needed > 3 {
        panic!("data too long");
    }

    let msg: u8 =  match bytes_needed {
        0 => {0b1000_0000}
        1 => {0b1001_0000}
        2 => {0b1010_0000}
        3 => {0b1011_0000}
        _ => {panic!("This Shouldn't happen")}
    };

    let mut size_in_bytes = Vec::with_capacity(bytes_needed);
    for i in (0..bytes_needed).rev() {
        let shift = i * 8;
        size_in_bytes.push(((data_len >> shift) & 0xFF) as u8);
    }

    let mut final_message = vec![msg];
    final_message.append(&mut size_in_bytes);
    final_message.extend_from_slice(data);
    final_message
}


pub struct RudishSessionHandler {
    cache: Arc<RwLock<Cache>>,
    master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
    write_master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
}



impl RudishSessionHandler {
    pub fn new(
        cache: Arc<RwLock<Cache>>,
        master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
        write_master_buffer: Arc<RwLock<[u8; BUFFER_SIZE]>>,
    ) -> Self {
        Self {
            cache,
            master_buffer,
            write_master_buffer
        }
    }
    pub fn run(mut self, should_terminate: Arc<AtomicBool>, master_buffer_size: Arc<AtomicUsize>, write_master_buffer_size: Arc<AtomicUsize>) {
        let mut data: VecDeque<u8> = VecDeque::new();
        let mut required_bytes: usize = 1;
        let mut signal: Signal= Signal::None;
        let mut last_interaction = Instant::now();

        while !should_terminate.load(Ordering::Relaxed) {
            let mut buffer = [0u8; BUFFER_SIZE];

            let bytes =
                match drain_from_master_buffer(&self.master_buffer, &master_buffer_size, &mut buffer){
                    Some(bytes) => bytes,
                    None => {
                        if last_interaction.elapsed() >  Duration::from_millis(100){
                            thread::sleep(Duration::from_millis(10));
                        }
                        else{
                            thread::yield_now();
                        }
                        continue;
                    },
                };


            data.extend(buffer[..bytes].iter().cloned());

            if data.len() >= required_bytes {
                process(&should_terminate, &mut data, signal.clone(), self.cache.clone(), &mut self.write_master_buffer, &write_master_buffer_size).map(|x |{
                    signal = x.1;
                    required_bytes = x.0;
                });
            }

            last_interaction = Instant::now();
        }
    }
}

fn process(should_terminate: &AtomicBool, data: &mut VecDeque<u8>, signal: Signal, cache: Arc<RwLock<Cache>>, output_data: &mut Arc<RwLock<[u8; BUFFER_SIZE]>>, write_master_buffer_size: &Arc<AtomicUsize>) -> Option<(usize, Signal)> {

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
                    0b10 => Signal::Get(first_byte.clone()),
                    0b11 => Signal::Ping,
                    _ => Signal::None,
                };

                continue;
            }
            Signal::Put(value) => {
                let second_top_bits = (value & 0b0011_0000) >> 4 ;

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
                    .skip(needed_bytes-second_top_bits as usize)
                    .take(second_top_bits as usize)
                    .fold(0usize, |acc, &b| (acc << 8) | b as usize);

                let header_end = needed_bytes;
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

                {
                    cache.write().unwrap().put(&topic, &key, &data_string);
                }


                for _ in 0..expected_value_len {
                    data.pop_front();
                }

                current_signal = Signal::None
            }
            Signal::Get(_) => {

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
                        trace!("Key: {} Value: {}",  key, value);
                        let msg = add_get_data(value.as_bytes());
                        safe_fill(should_terminate, output_data, write_master_buffer_size, &*msg, msg.len() );
                    }
                    _ => {}
                }


                data.drain(0..key_position.1);
                current_signal = Signal::None
            },
            Signal::Ping => {
                let msg = &[PING];
                trace!("Received Ping");
                safe_fill(should_terminate, output_data, write_master_buffer_size, &*msg, 1);
                current_signal = Signal::None
            },
            _ => {}
        }

    }

}
