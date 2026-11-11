//! Receives the engine's `udp_stream` output (raw 32-bit float mono PCM) on
//! localhost and buffers it so the browser can poll for new bytes.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_BUFFERED_BYTES: usize = 16000 * 4 * 10; // ~10 s of mono float32 at 16 kHz

#[derive(Default)]
struct Buffer {
    data: Vec<u8>,
    base_offset: u64, // stream offset of data[0]
}

impl Buffer {
    fn push(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        if self.data.len() > MAX_BUFFERED_BYTES {
            let trim = self.data.len() - MAX_BUFFERED_BYTES;
            self.data.drain(..trim);
            self.base_offset += trim as u64;
        }
    }

    fn read_since(&self, since: u64) -> (Vec<u8>, u64) {
        let start = since.max(self.base_offset) - self.base_offset;
        let start = (start as usize).min(self.data.len());
        (self.data[start..].to_vec(), self.base_offset + self.data.len() as u64)
    }
}

pub struct LiveChannel {
    buffer: Arc<Mutex<Buffer>>,
    pub error: Option<String>,
}

impl LiveChannel {
    fn start(port: u16) -> Self {
        let buffer = Arc::new(Mutex::new(Buffer::default()));
        let error = match UdpSocket::bind(("127.0.0.1", port)) {
            Ok(sock) => {
                let _ = sock.set_read_timeout(Some(Duration::from_secs(1)));
                let buf = buffer.clone();
                std::thread::spawn(move || {
                    let mut packet = vec![0u8; 65536];
                    loop {
                        match sock.recv(&mut packet) {
                            Ok(n) => buf.lock().unwrap().push(&packet[..n]),
                            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
                            // Windows reports ICMP "port unreachable" on UDP sockets as a recv error.
                            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
                            Err(_) => break,
                        }
                    }
                });
                None
            }
            Err(e) => Some(e.to_string()),
        };
        LiveChannel { buffer, error }
    }

    pub fn read_since(&self, since: u64) -> (Vec<u8>, u64) {
        self.buffer.lock().unwrap().read_since(since)
    }
}

#[derive(Clone, Default)]
pub struct LiveAudioManager {
    channels: Arc<Mutex<HashMap<u16, Arc<LiveChannel>>>>,
}

impl LiveAudioManager {
    /// Returns the listener for `port`, binding it the first time it is asked for.
    pub fn get(&self, port: u16) -> Arc<LiveChannel> {
        self.channels.lock().unwrap().entry(port).or_insert_with(|| Arc::new(LiveChannel::start(port))).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_tracks_offsets_and_trims() {
        let mut b = Buffer::default();
        b.push(&[1, 2, 3]);
        assert_eq!(b.read_since(0), (vec![1, 2, 3], 3));
        assert_eq!(b.read_since(2), (vec![3], 3));
        assert_eq!(b.read_since(99), (vec![], 3));
        b.push(&vec![0u8; MAX_BUFFERED_BYTES]);
        let (chunk, next) = b.read_since(0);
        assert_eq!(chunk.len(), MAX_BUFFERED_BYTES);
        assert_eq!(next, 3 + MAX_BUFFERED_BYTES as u64);
    }

    #[test]
    fn receives_udp_packets() {
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let mgr = LiveAudioManager::default();
        let ch = mgr.get(port);
        assert!(ch.error.is_none());
        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        sender.send_to(&[9, 8, 7, 6], ("127.0.0.1", port)).unwrap();
        for _ in 0..50 {
            if ch.read_since(0).1 == 4 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(ch.read_since(0), (vec![9, 8, 7, 6], 4));
        assert!(Arc::ptr_eq(&ch, &mgr.get(port)));
    }
}
