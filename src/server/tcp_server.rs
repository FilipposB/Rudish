use crate::server::{treat, Server};
use crate::cache::Cache;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use std::sync::RwLock;

pub struct TCPServerSettings {
    pub port: u16,
    pub threads: usize,
}

impl TCPServerSettings {
    pub fn new(port: u16, threads: usize) -> Self {
        Self { port, threads }
    }
}

pub struct TCPServer {
    listener: Arc<TcpListener>,
    threads: usize,
    queue: Arc<Mutex<Vec<TcpStream>>>,
    should_stop: Arc<AtomicBool>,
    cache: Arc<RwLock<Cache>>,
}

impl Server<TCPServerSettings> for TCPServer {
    fn run(&self) -> std::io::Result<()> {
        let mut handles = Vec::new();

        for i in 0..self.threads {
            let queue = Arc::clone(&self.queue);
            let should_stop = Arc::clone(&self.should_stop);
            let cache = Arc::clone(&self.cache);

            let handle = thread::spawn(move || {
                while !should_stop.load(Ordering::SeqCst) {
                    let mut stream_opt = {
                        let mut queue_guard = queue.lock().unwrap();
                        queue_guard.pop()
                    };

                    if let Some(mut stream) = stream_opt {
                        let i = 199;
                        let mut buffer = [0u8; 1];
                        let mut missed_laps = 0;

                        while missed_laps < 10 {
                            match stream.read(&mut buffer) {
                                Ok(amt) if amt > 0 => {
                                    let data = buffer[..amt].to_vec();
                                    println!("[Worker {}] Received {} bytes", i, amt);

                                    if let Some(response) = treat(i as u8, data, Arc::clone(&cache), &stream) {
                                        let _ = stream.write_all(&response);
                                    }

                                    missed_laps = 0;
                                }
                                Ok(_) => {
                                    println!("[Worker {}] Idle connection...", i);
                                    thread::sleep(Duration::from_secs(1));
                                    missed_laps += 1;
                                }
                                Err(e) => {
                                    eprintln!("[Worker {}] Read error: {}", i, e);
                                    break;
                                }
                            }
                        }

                        println!("[Worker {}] Connection closed.", i);
                    } else {
                        thread::sleep(Duration::from_millis(1));
                    }
                }

                println!("[Worker {}] Shutting down...", i);
            });

            handles.push(handle);
        }

        let listener = Arc::clone(&self.listener);
        let queue = Arc::clone(&self.queue);
        let should_stop = Arc::clone(&self.should_stop);

        thread::spawn(move || {
            while !should_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, addr)) => {
                        println!("[Main Thread] Accepted connection from {}", addr);
                        let mut queue_guard = queue.lock().unwrap();
                        queue_guard.push(stream);
                    }
                    Err(e) => {
                        eprintln!("Accept error: {}", e);
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }

            println!("[Main Thread] Listener stopping...");
        });

        for handle in handles {
            let _ = handle.join();
        }

        Ok(())
    }

    fn close(&self) -> std::io::Result<()> {
        self.should_stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn new(config: TCPServerSettings, should_stop: Arc<AtomicBool>) -> TCPServer {
        let addr = format!("127.0.0.1:{}", config.port);
        let listener = TcpListener::bind(&addr).expect("Failed to bind TCP socket");
        let queue = Arc::new(Mutex::new(Vec::new()));
        let cache = Arc::new(RwLock::new(Cache::new()));

        TCPServer {
            listener: Arc::new(listener),
            threads: config.threads,
            queue,
            should_stop,
            cache,
        }
    }
}
