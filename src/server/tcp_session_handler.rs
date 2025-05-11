use crate::cache::Cache;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use rudish_lib::{drain_from_master_buffer, safe_fill};
use tracing::{debug, error, info, trace};
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
                stream.nodelay().unwrap();

                
                let terminate_rudish_handler = Arc::new(AtomicBool::new(false));


                let mut buffer = [0u8; BUFFER_SIZE];


                let write_master_buffer  = Arc::new(RwLock::new([0u8; BUFFER_SIZE]));
                let write_master_buffer_size = Arc::new(AtomicUsize::new(0));

                let write_handler;

                {
                    let should_terminate = terminate_rudish_handler.clone();
                    let master_buffer = write_master_buffer.clone();
                    let master_buffer_size = write_master_buffer_size.clone();
                    let mut stream = stream.try_clone().unwrap();

                    write_handler =   thread::Builder::new()
                        .name(format!("Handler-{}", self.handler_id)).spawn(move || {
                        while !should_terminate.load(Ordering::Relaxed) {
                            let mut buffer = [0u8; BUFFER_SIZE];

                            let bytes =
                                match drain_from_master_buffer(&master_buffer, &master_buffer_size, &mut buffer){
                                    Some(bytes) => bytes,
                                    None => {
                                        thread::yield_now();
                                        continue;
                                    },
                                };


                            if bytes > 0{
                                stream.write(&buffer[..bytes]).unwrap();
                                trace!("Sent {} bytes", bytes);
                            }
                        }
                    }).unwrap();
                }

                let mut read_master_buffer = Arc::new(RwLock::new([0u8; BUFFER_SIZE]));
                let read_master_buffer_size = Arc::new(AtomicUsize::new(0));

                let rudish_handler;

                {
                    let terminate_rudish_handler = terminate_rudish_handler.clone();
                    let cache = self.cache.clone();
                    let read_master_buffer = read_master_buffer.clone();
                    let reader_master_buffer_size = read_master_buffer_size.clone();

                    let write_master_buffer = write_master_buffer.clone();
                    let write_master_buffer_size = write_master_buffer_size.clone();



                    rudish_handler =   thread::Builder::new()
                        .name(format!("Handler-{}", self.handler_id)).spawn(move || {RudishSessionHandler::new(cache, read_master_buffer, write_master_buffer).run(terminate_rudish_handler, reader_master_buffer_size, write_master_buffer_size)}).unwrap();
                }

                let mut last_interaction = Instant::now();
                let mut last_message = Instant::now();

                let message_frequency = Duration::from_millis(500);

                let mut locked_stream = stream.try_clone().unwrap();
                locked_stream.set_read_timeout(Some(self.timeout)).unwrap();


                while last_interaction.elapsed() < self.timeout {
                    match locked_stream.read(&mut buffer) {
                        Ok(amt) if amt > 0 => {
                            safe_fill(&self.should_terminate, &mut read_master_buffer, &read_master_buffer_size, &buffer, amt);
                            
                            trace!("Received {} bytes", amt);
                            last_interaction =  Instant::now();
                        }
                        Ok(_) => {
                            if last_message.elapsed() > message_frequency {
                                debug!("Idle connection...");
                                last_message = Instant::now();
                            }
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(e) => {
                            error!("Read error: {}",  e);
                            break;
                        }
                    }
                }

                terminate_rudish_handler.store(true, Ordering::Relaxed);
                rudish_handler.join().unwrap();
                write_handler.join().unwrap();
                info!("Connection closed.");
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }

        info!("Shutting down");
    }



}
