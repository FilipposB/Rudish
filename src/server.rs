use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool};
use std::sync::{Arc};

mod rudish_session_handler;
pub mod tcp_server;
mod tcp_session_handler;

pub trait Server<T> {
    fn run(&self) -> std::io::Result<()>;
    fn close(&self) -> std::io::Result<()>;
    fn new(config: T, terminate: Arc<AtomicBool>) -> Self;
}

pub(crate) const BUFFER_SIZE: usize = 1024*5;
