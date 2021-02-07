/*!
A non-blocking socket wrapper for sending and receiving JSON-encoded
`proto3::Msg` objects.

updated: 2021-02-07

As `grel` is IRC-style chat software, many messages will have a single
_source_ and be sent to multiple _destinations_. As such, the operation
of `sock::Sock` is asymmetrical: Reading _from_ a `Sock` yields
`proto3::Rcvr` structs, but the data written _to_ a `Sock` should
already be JSON-encoded slices of bytes. This is to avoid the unnecessary
overhead of encoding a `proto3::Sndr` once for each recipient. Instead,
it gets encoded when sticking it into a `proto3::Env`, and those bytes
get pushed into the `Sock`.

*/
use std::io::{Read, Write};
use std::net::{TcpStream, Shutdown};
use std::error::Error;
use serde_json::error::Category;
use super::proto3::Rcvr;

const DEFAULT_BUFFER_SIZE: usize = 1024;

const NEWLINE: u8 = '\n' as u8;

static ERRS: &'static [&'static str] = &[
    "Unable to set_nodelay on underlying socket",               // 0
    "Unable to set_nonblocking on underlying socket",           // 1
    "Error shutting down underlying socket",                    // 2
    "Error reading from the underlying socket",                 // 3
    "Syntax error in data from underlying socket",              // 4
    "Error writing to the underlying socket",                   // 5
    "Error flushing the underlying socket",                     // 6
    "Error retrieving the remote address",                      // 7
];

/**
The `SockError` wraps or signals errors on the `Sock`'s underlying `TcpStream`.
If a `Sock` returns one of these, it's probably in the market for being
`shutdown()`.
*/
#[derive(Debug)]
pub struct SockError { msg: String }

impl SockError {
    /// Instantiate a new `SockError` from an `&str` message.
    pub fn new(message: &str) -> SockError {
        SockError { msg: String::from(message), }
    }
    
    /// Instantiate a new `SockError` from a `String` message.
    pub fn string(message: String) -> SockError {
        SockError { msg: message, }
    }
    
    /// Wrap an underlying error (probably a `std::io::Result` from the
    /// underlying `TcpStream` with a message from `ERRS`, above.
    fn from_err(errno: usize, e: &dyn Error) -> SockError {
        SockError::string(format!("{}: {}", ERRS[errno], e))
    }
}

impl std::fmt::Display for SockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SockError: {}", &(self.msg))
    }
}

impl Error for SockError {}

/* This is a hacky way of turning certain helpful-to-a-human but
not-so-helpful-to-a-machine error messages returned by `serde_json`'s
decoding functions.

If a chunk of data has extra characters after the end of a syntactially-
correct JSON object, `serde_json` will return an `Error` with the one-based
line and column offsets of the first problematic characters. This function
grovels through the stream of bytes, counting characters and keeping track
of newlines, to determine the actual _byte offset_ of said characters. That
way, the complete JSON object can be sliced off and decoded, leaving the
remaining data in the buffer.
*/
fn get_actual_offset(dat: &[u8], e: &serde_json::Error)
-> Result<usize, &'static str> {
    let line = e.line() - 1;
    let col  = e.column() - 1;
    let mut line_n: usize = 0;
    let mut offs: Option<usize> = None;
    let mut n: usize = 0;
    loop {
        if line_n < line {
            match dat.get(n) {
                None => break,
                Some(b) => {
                    if *b == NEWLINE { line_n = line_n + 1; }
                },
            }
            n = n + 1;
        } else {
            offs = Some(n + col);
            break;
        }
    }
    match offs {
        None => Err("Overran buffer seeking error location."),
        Some(v) => Ok(v),
    }
}

/**
The `sock::Sock` wraps a `std::net::TcpStream` and exchanges
`proto3::{Sndr, Rcvr}` objects over it.

It's default mode is entirely non-blocking, and suitable for single-threaded
operation. Chunks of encoded JSON can be queued into its send buffer, and
attempts to decode incoming chunks from its receive buffer can be made at
any time. Nonblocking attempts can be made to send/receive bytes down/up the
underlying stream to/from those respective buffers can be made at any time.

If any of the operations returns a `SockError`, it probably means the
connection should be shut down.
*/
pub struct Sock {
    sock: TcpStream,
    read_buff: Vec::<u8>,
    current: Vec::<u8>,
    send_buff: Vec::<u8>,
}

impl Sock {
    
    /** Wrap a `std::net::TcpStream` in a new `Sock` */
    pub fn new(stream: TcpStream) -> Result<Sock, SockError> {
        if let Err(e) = stream.set_nodelay(true) {
            return Err(SockError::from_err(0, &e));
        }
        if let Err(e) = stream.set_nonblocking(true) {
            return Err(SockError::from_err(1, &e));
        }
        let mut new_buff: Vec<u8> = Vec::with_capacity(DEFAULT_BUFFER_SIZE);
        new_buff.resize(DEFAULT_BUFFER_SIZE, 0u8);
        let s = Sock {
            sock: stream,
            read_buff: new_buff,
            current: Vec::<u8>::new(),
            send_buff: Vec::<u8>::new(),
        };
        return Ok(s);
    }
    
    /** Shuts down the underlying stream (or tries to). */
    pub fn shutdown(&mut self) -> Result<(), SockError> {
        match self.sock.shutdown(Shutdown::Both) {
            Err(e) => Err(SockError::from_err(2, &e)),
            Ok(()) => Ok(()),
        }
    }
    
    /** By default, each nonblocking `.suck()` call will attempt to read
    DEFAULT_BUFFER_SIZE (1024) bytes. You can change that with this function.
    
    Setting this to 0 would be pointless and stupid.
    */
    pub fn set_read_buffer_size(&mut self, new_size: usize) {
        self.read_buff.resize(new_size, 0u8);
    }
    
    /** Returns how many bytes this attempts to read per `.suck()`. */
    pub fn get_read_buffer_size(&self) -> usize {
        self.read_buff.len()
    }
    
    /** Attempts to read data from the underlying stream, copying it into
    its internal buffer for later attempted decoding. If this returns the
    `Err(SockError)` variant, it should probably be `.shutdown()`. Otherwise,
    returns the number of bytes read.
    
    A return value of `Ok(0)` either means there wasn't any data to read,
    or something nonfatal interrupted the attempt to read.
    */
    pub fn suck(&mut self) -> Result<usize, SockError> {
        match self.sock.read(&mut self.read_buff) {
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock  => Ok(0),
                    std::io::ErrorKind::Interrupted => Ok(0),
                    _ => Err(SockError::from_err(3, &e)),
                }
            },
            Ok(n) => {
                if n > 0 {
                    self.current.extend_from_slice(&self.read_buff[..n]);
                }
                Ok(n)
            },
        }
    }
    
    /** Attempts to decode a `proto3::Rcvr` from its internal buffer of
    data read from the underlying stream. A returned error value means
    the stream is receiving syntactically bad data and should probably be
    shut down. A returned `Ok(None)` means there isn't enough data in the
    buffer to form a full `Rcvr`.
    */
    pub fn try_get(&mut self) -> Result<Option<Rcvr>, SockError> {
        let offs;
        let maybe_msg = serde_json::from_slice::<Rcvr>(&self.current);
        match maybe_msg {
            Ok(m) => {
                self.current.clear();
                return Ok(Some(m));
            },
            Err(e) => match e.classify() {
                Category::Eof => { return Ok(None); },
                Category::Syntax => {
                    offs = get_actual_offset(&self.current, &e).unwrap();
                },
                _ => { return Err(SockError::from_err(4, &e)); },
            },
        }
        
        let maybe_msg = serde_json::from_slice::<Rcvr>(&self.current[..offs]);
        match maybe_msg {
            Ok(m) => {
                let temp = (&self.current[offs..]).to_vec();
                self.current = temp;
                return Ok(Some(m));
            },
            Err(e) => { return Err(SockError::from_err(4, &e)); },
        }
    }
    
    /** Blockingly busy-waits, attempting every `tick` time interval to
    `.suck()` data from the underlying stream until it has enough data to
    decode and return a `proto3::Rcvr`. A returned error is probably
    grounds for a `.shutdown()`.
    */
    pub fn blocking_get(&mut self, tick: std::time::Duration)
    -> Result<Rcvr, SockError> {
        if let Some(m) = self.try_get()? { return Ok(m); }
        
        loop {
            if self.suck()? == 0 {
                std::thread::sleep(tick);
            } else {
                if let Some(m) = self.try_get()? { return Ok(m); }
            }
        }
    }
    
    /** Copies `data` to the outgoing send buffer, to be sent on subesequent
    calls to `.blow()`. Needless to say, `data` should be a JSON-encoded
    `proto3::Sndr`.
    */
    pub fn enqueue(&mut self, data: &[u8]) {
        self.send_buff.extend_from_slice(data);
    }
    
    /** Attempts to write data that's been `.enqueue()`d onto the internal
    send buffer to the underlying stream. Returns the number of bytes _left
    in the send buffer_, as opposed to the number of bytes sent. This way,
    `Ok(0)` always means the send buffer is empty. As with other functions
    that can return an error, this is probably fatal and the `Sock` should
    be `.shutdown()`.
    */
    pub fn blow(&mut self) -> Result<usize, SockError> {

        let res = self.sock.write(&self.send_buff);
        
        match res {
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    Ok(self.send_buff.len())
                } else {
                    Err(SockError::from_err(5, &e))
                }
            },
            Ok(n) => {
                if n == self.send_buff.len() {
                    if let Err(e) = self.sock.flush() {
                        Err(SockError::from_err(6, &e))
                    } else {
                        self.send_buff.clear();
                        Ok(0)
                    }
                } else {
                    let temp = (&self.send_buff[n..]).to_vec();
                    self.send_buff = temp;
                    Ok(self.send_buff.len())
                }
            },
        }
    }
    
    /** Queues up the supplied `data` at the end of  the send buffer, then
    blockingly attemps to `.blow()` every `tick` until the send buffer is empty.
    */
    pub fn blocking_send(&mut self, data: &[u8], tick: std::time::Duration)
    -> Result<(), SockError> {
        self.enqueue(data);
        loop {
            if 0 == self.blow()? { return Ok(()); }
            std::thread::sleep(tick);
        }
    }
    
    /** Returns how many bytes are still queued up to be `.blow()`n. */
    pub fn send_buff_size(&self) -> usize { self.send_buff.len() }
    /** Returns how many bytes are sitting in the receive buffer waiting
    to get decoded. */
    pub fn recv_buff_size(&self) -> usize { self.current.len() }
    
    /// Returns the address of the remote endpoint of the underlying stream.
    pub fn get_addr(&self) -> Result<String, SockError> {
        match self.sock.peer_addr() {
            Ok(a)  => Ok(a.to_string()),
            Err(e) => Err(SockError::from_err(7, &e)),
        }
    }
    
}

/* These tests need to be rewritten, because since the introduction of
`proto3`, a sent message will never be the same as its decoded received
version. */

#[cfg(will_never_be_used)]
mod test {
    use super::*;
    use std::time::Duration;
    use std::net::TcpListener;
    use std::thread;
    
    static SLEEP_T: Duration = Duration::from_millis(100);
    const ADDR: &str = "127.0.0.1:51517";
    
    #[test]
    fn two_way_test() {
        let ma0 = Msg::Ping;
        let ma1 = ma0.clone();
        
        let mb0 = Msg::Text{
            who: String::from("Some Guy"),
            lines: vec![String::from("One line of text."),
                        String::from("A second line of text.")],
        };
        let mb1 = mb0.clone();
        
        let lthread = thread::spawn(move || {
            let lnr = TcpListener::bind(ADDR).unwrap();
            println!("TcpListener is listening.");
            let (tcp_sock, addr) = lnr.accept().unwrap();
            println!("TcpListener accept connection from {}.", addr);
            let mut sck = Sock::new(tcp_sock).unwrap();
            println!("Listener Sock created.");
            
            sck.blocking_send(&(ma0.bytes()), SLEEP_T).unwrap();
            println!("Listener has written message.");
            let rtrn_msg = sck.blocking_get(SLEEP_T).unwrap();
            println!("Listener has rec'd response:\n{:?}", &rtrn_msg);
            assert_eq!(rtrn_msg, mb1);
            sck.shutdown().unwrap();
        });
        
        let cthread = thread::spawn(move || {
            let tcp_sock = TcpStream::connect(ADDR).unwrap();
            println!("Connector connected.");
            let mut sck = Sock::new(tcp_sock).unwrap();
            println!("Connector Sock created.");
            
            let frst_msg = sck.blocking_get(SLEEP_T).unwrap();
            println!("Connector rec'd message:\n{:?}", frst_msg);
            assert_eq!(frst_msg, ma1);
            
            sck.blocking_send(&(mb0.bytes()), SLEEP_T).unwrap();
            println!("Connector has written message.");
            sck.shutdown().unwrap();
        });
        
        lthread.join().unwrap();
        cthread.join().unwrap();
        println!("Threads joined.");
    }
}
