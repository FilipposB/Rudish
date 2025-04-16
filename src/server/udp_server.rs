// use crate::server::{treat, Server};
// use crate::cache::Cache;
//
// use std::{
//     collections::VecDeque,
//     net::{SocketAddr, UdpSocket},
//     sync::{
//         atomic::{AtomicBool, Ordering},
//         Arc, Mutex,
//     },
//     thread,
//     time::Duration,
// };
//
// pub struct UDPServerSettings {
//     port: u16,
//     threads: usize,
// }
//
// impl UDPServerSettings {
//     pub fn new(port: u16, threads: usize) -> Self {
//         Self { port, threads }
//     }
// }
//
// pub struct UDPServer {
//     socket: Arc<UdpSocket>,
//     threads: usize,
//     queue: Arc<Mutex<VecDeque<(Vec<u8>, SocketAddr)>>>,
//     should_stop: Arc<AtomicBool>,
//     cache: Arc<Mutex<Cache>>,
// }
//
// impl Server<UDPServerSettings> for UDPServer {
//     fn run(&self) -> std::io::Result<()> {
//         let mut handles = Vec::new();
//
//         for i in 0..self.threads {
//             let queue = Arc::clone(&self.queue);
//             let socket = Arc::clone(&self.socket);
//             let should_stop = Arc::clone(&self.should_stop);
//             let cache = Arc::clone(&self.cache);
//
//             let handle = thread::spawn(move || {
//                 while !should_stop.load(Ordering::SeqCst) {
//                     let maybe_job = {
//                         let mut q = queue.lock().unwrap();
//                         q.pop_front()
//                     };
//
//                     if let Some((data, src)) = maybe_job {
//                         println!("[Worker {}] Received from {}: {} Bytes {:?}", i, src, data.len(), data);
//                         if let Some(response) = treat(i as u8, data, cache.clone(), ) {
//                             if let Err(e) = socket.send_to(&response, src) {
//                                 eprintln!("send_to error: {}", e);
//                             }
//                         }
//                     } else {
//                         thread::sleep(Duration::from_millis(1));
//                     }
//                 }
//
//                 println!("[Worker {}] Shutting down gracefully...", i);
//             });
//
//             handles.push(handle);
//         }
//
//         // Receive loop on main thread
//         let mut buf = [0u8; 1024];
//         while !self.should_stop.load(Ordering::SeqCst) {
//             match self.socket.recv_from(&mut buf) {
//                 Ok((amt, src)) => {
//                     let data = buf[..amt].to_vec();
//                     let mut q = self.queue.lock().unwrap();
//                     q.push_back((data, src));
//                 }
//                 Err(e) => {
//                     eprintln!("recv_from error: {}", e);
//                     thread::sleep(Duration::from_millis(10));
//                 }
//             }
//         }
//
//         for handle in handles {
//             let _ = handle.join();
//         }
//
//         Ok(())
//     }
//
//     fn close(&self) -> std::io::Result<()> {
//         self.should_stop.store(true, Ordering::SeqCst);
//         Ok(())
//     }
//
//     fn new(config: UDPServerSettings, should_stop: Arc<AtomicBool>) -> UDPServer {
//         let addr = format!("127.0.0.1:{}", config.port);
//         let socket = UdpSocket::bind(&addr).expect("Failed to bind UDP socket");
//         socket.set_nonblocking(false).expect("Failed to set blocking mode");
//
//         let queue = Arc::new(Mutex::new(VecDeque::new()));
//
//         let cache = {
//             let mut cache = Cache::new();
//             cache.put("new", "new", "1");
//             cache.put("new", "new", "2");
//             cache.put("new", "new", "3");
//
//             Arc::new(Mutex::new(cache))
//         };
//
//         UDPServer {
//             socket: Arc::new(socket),
//             threads: config.threads,
//             queue,
//             should_stop,
//             cache,
//         }
//     }
// }
