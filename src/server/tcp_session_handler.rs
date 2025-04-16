use crate::cache::Cache;
use std::cmp::min;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use crate::server::rudish_session_handler::RudishSessionHandler;
use crate::server::{BUFFER_SIZE};


pub struct TcpSessionHandler {
    handler_id: usize,
    should_terminate: Arc<AtomicBool>,
    cache: Arc<RwLock<Cache>>,
    queue: Arc<Mutex<Vec<TcpStream>>>,
    timeout: Duration,
}

impl TcpSessionHandler {
    pub fn new(
        handler_id: usize,
        should_terminate: Arc<AtomicBool>,
        cache: Arc<RwLock<Cache>>,
        queue: Arc<Mutex<Vec<TcpStream>>>,
        timeout: Duration
    ) -> Self {
        TcpSessionHandler {
            handler_id,
            should_terminate,
            cache,
            queue,
            timeout
        }
    }

    pub fn run(self) {

        while !self.should_terminate.load(Ordering::SeqCst) {
            // Accept a TcpStream
            let mut stream_opt = {
                let mut queue_guard = self.queue.lock().unwrap();
                queue_guard.pop()
            };

            // Process TcpStream
            if let Some( stream) = stream_opt {
                let stream = Arc::new(stream);
                
                let terminate_rudish_handler = Arc::new(AtomicBool::new(false));


                let mut buffer = [0u8; BUFFER_SIZE];

                let mut master_buffer = Arc::new(RwLock::new([0u8; BUFFER_SIZE]));
                let master_buffer_size = Arc::new(AtomicUsize::new(0));

                let handler;

                {
                    let handler_id = self.handler_id;
                    let terminate_rudish_handler = terminate_rudish_handler.clone();
                    let cache = self.cache.clone();
                    let master_buffer = master_buffer.clone();
                    let master_buffer_size = master_buffer_size.clone();
                    let stream = stream.try_clone().unwrap();
            
                    handler = thread::spawn(move || {RudishSessionHandler::new(handler_id, cache, master_buffer).run(terminate_rudish_handler, master_buffer_size, stream)});
                }

                let mut last_interaction = Instant::now();
                let mut last_message = Instant::now();

                let message_frequency = Duration::from_millis(500);
                
                let mut stream =  stream.clone();

                while last_interaction.elapsed() < self.timeout {
                    let mut locked_stream = stream.try_clone().unwrap();

                    match locked_stream.read(&mut buffer) {
                        Ok(amt) if amt > 0 => {
                            Self::safe_fill(&self, &mut master_buffer, &master_buffer_size, buffer, amt);
                            println!("[Handler {}] Received {} bytes", self.handler_id, amt);
                            last_interaction = Instant::now();
                        }
                        Ok(_) => {
                            if last_message.elapsed() > message_frequency {
                                println!("[Handler {}] Idle connection...", self.handler_id);
                                last_message = Instant::now();
                            }
                            drop(locked_stream);
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            eprintln!("[Handler {}] Read error: {}", self.handler_id, e);
                            break;
                        }
                    }
                }

                terminate_rudish_handler.store(true, Ordering::Relaxed);
                handler.join().unwrap();
                println!("[Handler {}] Connection closed.", self.handler_id);
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }

        println!("[Handler {}] Shutting down...", self.handler_id);
    }

    fn safe_fill(&self, master_buffer: &mut Arc<RwLock<[u8; BUFFER_SIZE]>>, master_buffer_size: &Arc<AtomicUsize>, buffer: [u8; BUFFER_SIZE], buffer_size: usize){
        let mut offset: usize = 0;

        while !self.should_terminate.load(Ordering::Relaxed) {
            let number_of_bytes_to_copy;
            let write_offset;

            {
                let mut locked_master_buffer = master_buffer.write().unwrap();
                let current_master_size = master_buffer_size.load(Ordering::Relaxed);
                let available_space = BUFFER_SIZE - current_master_size;

                number_of_bytes_to_copy = min(buffer_size - offset, available_space);

                if number_of_bytes_to_copy == 0 {
                    drop(locked_master_buffer);
                    thread::sleep(Duration::from_micros(10));
                    continue;
                }

                write_offset = current_master_size;

                locked_master_buffer[write_offset..write_offset + number_of_bytes_to_copy]
                    .copy_from_slice(&buffer[offset..offset + number_of_bytes_to_copy]);

                // Update the master_buffer_size
                master_buffer_size.fetch_add(number_of_bytes_to_copy, Ordering::Relaxed);
            }

            offset += number_of_bytes_to_copy;

            if offset == buffer_size {
                break;
            }
        }
    }
}
