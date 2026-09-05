//! Local HTTP fixtures with explicit shutdown, shared by transport and workflow tests.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

pub(super) struct TestServer {
    pub(super) base: String,
    pub(super) bodies: Arc<Mutex<Vec<String>>>,
    pub(super) methods: Arc<Mutex<Vec<String>>>,
    thread: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    address: SocketAddr,
    release: mpsc::Sender<()>,
    active: Arc<Mutex<Option<TcpStream>>>,
    accepted: mpsc::Receiver<()>,
}

impl TestServer {
    pub(super) fn wait_for_connection(&self) {
        self.accepted
            .recv_timeout(Duration::from_secs(5))
            .expect("server accepted connection");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _release = self.release.send(());
        if let Some(stream) = self.active.lock().expect("active connection").as_ref() {
            let _shutdown = stream.shutdown(Shutdown::Both);
        }
        let _wake = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
        if let Some(handle) = self.thread.take() {
            let result = handle.join();
            if !thread::panicking() {
                result.expect("server thread");
            }
        }
    }
}

pub(super) fn server(responses: Vec<(&str, &str)>) -> TestServer {
    let responses: Vec<(String, String)> = responses
        .into_iter()
        .map(|(s, b)| (s.to_owned(), b.to_owned()))
        .collect();
    dynamic_server(responses.len(), move |index, _| responses[index].clone())
}

pub(super) fn dynamic_server(
    count: usize,
    mut respond: impl FnMut(usize, &str) -> (String, String) + Send + 'static,
) -> TestServer {
    spawn_server(count, move |index, body| Some(respond(index, body)))
}

// Wait for explicit shutdown instead of sleeping for a guessed client timeout.
// A bounded fallback closes the socket if the client's timeout regresses.
pub(super) fn stalled_server() -> TestServer {
    spawn_server(1, |_, _| None)
}

fn spawn_server(
    count: usize,
    mut respond: impl FnMut(usize, &str) -> Option<(String, String)> + Send + 'static,
) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = listener.local_addr().expect("address");
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&bodies);
    let methods = Arc::new(Mutex::new(Vec::new()));
    let captured_methods = Arc::clone(&methods);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = Arc::clone(&stop);
    let active = Arc::new(Mutex::new(None));
    let active_connection = Arc::clone(&active);
    let (release, released) = mpsc::channel();
    let (connection_ready, accepted) = mpsc::channel();
    let handle = thread::spawn(move || {
        for index in 0..count {
            let (mut stream, _) = listener.accept().expect("accept");
            *active_connection.lock().expect("active connection") =
                Some(stream.try_clone().expect("shutdown handle"));
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            let _ready = connection_ready.send(());
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            stream
                .set_write_timeout(Some(Duration::from_secs(2)))
                .expect("write timeout");
            let request = read_request(&mut stream);
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            let (method, request_body) = request.expect("read request");
            captured
                .lock()
                .expect("body lock")
                .push(request_body.clone());
            captured_methods.lock().expect("methods lock").push(method);
            if let Some((status, body)) = respond(index, &request_body) {
                let result = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                if !stopped.load(Ordering::SeqCst) {
                    result.expect("respond");
                }
            } else {
                let _release = released.recv_timeout(Duration::from_secs(5));
            }
            active_connection.lock().expect("active connection").take();
        }
    });
    TestServer {
        base: format!("http://{address}"),
        bodies,
        methods,
        thread: Some(handle),
        stop,
        address,
        release,
        active,
        accepted,
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<(String, String)> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    let header_end = loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    // Capture only the API method, never the token-bearing request path.
    let method = headers
        .lines()
        .next()
        .expect("request line")
        .split_whitespace()
        .nth(1)
        .expect("path")
        .rsplit('/')
        .next()
        .expect("method")
        .to_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
                .map(str::parse::<usize>)
        })
        .transpose()
        .expect("content length")
        .unwrap_or(0);
    while bytes.len() - header_end < length {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok((
        method,
        String::from_utf8(bytes[header_end..header_end + length].to_vec()).expect("utf8 body"),
    ))
}
