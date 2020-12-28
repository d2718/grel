//! jsock.rs
//!
//! A non-blocking socket for sending and receiving JSON messages.
//!
//! This socket wrapper was conceived and intended to be used with the
//! `grel` chat program. As such, for performance reasons, this socket is
//! designed to _send_ data that has already been JSON encoded, but _return_
//! data as `serde_json::Value` types.
//!
//! 2020-11-25

use std::io::{Read, Write};
use std::net::{TcpStream, Shutdown};
use serde_json::{Value, error::Category};
use std::error::Error;
use std::fmt;

const DEBUG: bool = false;

const DEFAULT_BUFF: usize = 1024;
const MAXIMUM_BUFF: usize = 8 * 1024;

const NEWLINE: u8 = '\n' as u8;

static ERRS: &'static [&'static str] = &[
    "Unable to set_nodelay on underlying socket",       // 0
    "Unable to set_nonblocking on underlying socket",   // 1
    "Error shutting down underlying socket",            // 2
    "Error reading from the underlying socket",         // 3
    "Non-syntactic data from underlying socket",        // 4
    "Error writing to the underlying socket",           // 5
    "Error flushing the underlying socket",             // 6
];

#[derive(Debug)]
pub struct JSockError {
    msg: String,
    is_fatal: bool,
}

impl JSockError {
    pub fn new(msg: &str, is_fatal: bool) -> JSockError {
        JSockError { msg: String::from(msg), is_fatal: is_fatal, }
    }
    
    pub fn string(msg: String, is_fatal: bool) -> JSockError {
        JSockError { msg: msg, is_fatal: is_fatal, }
    }
    
    fn from_err(errno: usize, e: &dyn Error, is_fatal: bool) -> JSockError {
        JSockError::string(format!("{}: {}", ERRS[errno], e), is_fatal)
    }
    
    pub fn fatal(&self) -> bool { self.is_fatal }
}

impl fmt::Display for JSockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let f_str: &str = match self.is_fatal {
            true  => "FATAL ",
            false => "",
        };
        write!(f, "{}JSockError: {}", &f_str, &(self.msg))
    }
}

impl Error for JSockError {}

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

pub struct JSock {
    sock: TcpStream,
    buff: Vec::<u8>,
    current: Vec::<u8>,
    out: Vec::<u8>,
    min_buff_capacity: usize,
    max_buff_capacity: usize,
}

impl JSock {
    pub fn new(stream: TcpStream) -> Result<JSock, JSockError> {
        if let Err(e) = stream.set_nodelay(true) {
            let re = JSockError::string(format!("{}: {}", ERRS[0], e), true);
            return Err(re);
        }
        if let Err(e) = stream.set_nonblocking(true) {
            let re = JSockError::string(format!("{}: {}", ERRS[1], e), true);
            return Err(re);
        }
        let mut new_buff: Vec<u8> = Vec::with_capacity(DEFAULT_BUFF);
        new_buff.resize(DEFAULT_BUFF, 0u8);
        let s = JSock {
            sock: stream,
            buff: new_buff,
            current: Vec::<u8>::new(),
            out: Vec::<u8>::new(),
            min_buff_capacity: DEFAULT_BUFF,
            max_buff_capacity: MAXIMUM_BUFF,
        };
        return Ok(s);
    }
    
    pub fn set_buff_limits(&mut self, min: usize, max: usize) {
        if min > max { panic!("Minimum buffer size cannot be more than maximum buffer size."); }
        self.min_buff_capacity = min;
        self.max_buff_capacity = max;
    }
    
    pub fn get_buff_limits(&self) -> (usize, usize) {
        (self.min_buff_capacity, self.max_buff_capacity)
    }
    
    pub fn shutdown(&mut self) -> Result<(), JSockError> {
        match self.sock.shutdown(Shutdown::Both) {
            Err(e) => {
                let re = JSockError::from_err(2, &e, false);
                Err(re)
            },
            Ok(()) => Ok(()),
        }
    }
    
    pub fn suck(&mut self) -> Result<usize, JSockError> {
        match self.sock.read(&mut self.buff) {
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::WouldBlock => Ok(0),
                    std::io::ErrorKind::Interrupted => Ok(0),
                    _ => {
                        let err = JSockError::from_err(3, &e, true);
                        Err(err)
                    }
                }
            },
            Ok(n) => {
                if n > 0 {
                    self.current.extend_from_slice(&self.buff[..n]);
                }
                if self.buff.len() > self.max_buff_capacity {
                    self.buff.resize(self.min_buff_capacity, 0u8);
                }
                if DEBUG { println!("suck(): unerlying socket read {} bytes", n); }
                Ok(n)
            },
        }
    }
    
    pub fn try_get(&mut self) -> Result<Option<Value>, JSockError> {
        let offs;
        match serde_json::from_slice(&self.current) {
            Ok(v) => {
                self.current.clear();
                return Ok(Some(v));
            },
            Err(e) => match e.classify() {
                Category::Eof => { return Ok(None); },
                Category::Syntax => {
                    offs = get_actual_offset(&self.current, &e).unwrap();
                },
                _ => {
                    let err = JSockError::from_err(4, &e, true);
                    return Err(err);
                },
            },
        }
        
        match serde_json::from_slice(&self.current[..offs]) {
            Ok(v) => {
                let temp = (&self.current[offs..]).to_vec();
                self.current = temp;
                return Ok(Some(v));
            },
            Err(e) => {
                let err = JSockError::from_err(4, &e, true);
                return Err(err);
            },
        }
    }
    
    pub fn blocking_get(&mut self, tick: std::time::Duration)
    -> Result<Value, JSockError> {
        if let Some(v) = self.try_get()? {
            return Ok(v);
        }
        
        loop {
            if self.suck()? == 0 {
                std::thread::sleep(tick);
            } else {
                if let Some(v) = self.try_get()? {
                    return Ok(v);
                }
            }
        }
    }
    
    pub fn push(&mut self, data: &[u8]) {
        self.out.extend_from_slice(data);
    }
    
    // Attempts to write outgoing bytes to the underlying socket.
    // Returns JSockError on fatal error, otherwise returns number of
    // bytes remaining in outbound buffer.
    pub fn blow(&mut self) -> Result<usize, JSockError> {
        let res = self.sock.write(&self.out);
        
        match res {
            Err(e) => {
                if e.kind() == std::io::ErrorKind::Interrupted {
                    Ok(self.out.len())
                } else {
                    Err(JSockError::from_err(5, &e, true))
                }
            },
            Ok(n) => {
                if DEBUG { println!("block(): wrote {} bytes", n); }
                if n == self.out.len() {
                    if let Err(e) = self.sock.flush() {
                        Err(JSockError::from_err(6, &e, true))
                    } else {
                        if DEBUG { println!("block(): underlying socket flushed"); }
                        self.out.clear();
                        Ok(0)
                    }
                } else {
                    let temp = (&self.out[n..]).to_vec();
                    self.out = temp;
                    Ok(self.out.len())
                }
            },
        }
    }
    
    pub fn blocking_push(&mut self, data: &[u8], tick: std::time::Duration)
    -> Result<(), JSockError> {
        self.push(data);
        loop {
            match self.blow() {
                Err(e) => { return Err(e); },
                Ok(n)  => { if n == 0 { return Ok(()); } },
            }
            std::thread::sleep(tick);
        }
    }
    
    pub fn outbuff_size(&self) -> usize {
        self.out.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use std::net::TcpListener;
    
    static SLEEP_T: Duration = Duration::from_millis(100);
    const ADDR: &str = "127.0.0.1:51516";
    
    static msg_txt0: &str = r#"{ "frogs": { "color": "blue", "wings": true } }"#;
    static msg_txt1: &str = r#"
{
    "status": "success",
    "payload": {
        "message": "This has been successful.",
        "code": 102
    }
}"#;
    static msg_txt2: &str = r#"{ "message": "yes" } { "flumph": "LG" }"#;
    
    #[test]
    fn test_two_way() {
        
        let val0: Value = serde_json::from_slice::<Value>(msg_txt0.as_bytes()).unwrap();
        let val1: Value = serde_json::from_slice::<Value>(msg_txt1.as_bytes()).unwrap();        

        let lthread = thread::spawn(|| {
            let lnr = TcpListener::bind(ADDR).unwrap();
            println!("Listener is listening.");
            let (sock, addr) = lnr.accept().unwrap();
            println!("Listener accepted connection from {}.", addr);
            let mut js = JSock::new(sock).unwrap();
            println!("Listener end JSock created.");
            
            js.blocking_push(msg_txt0.as_bytes(), SLEEP_T).unwrap();
            println!("Listener has written message.");
            let v = js.blocking_get(SLEEP_T).unwrap();
            println!("Listener rec'd response:\n{}", v.to_string());
            js.blocking_push(msg_txt2.as_bytes(), SLEEP_T).unwrap();
            println!("Listener sent double message");
            js.shutdown();
        });
        
        let cthread = thread::spawn(|| {
            let sock = TcpStream::connect(ADDR).unwrap();
            println!("Connected connected.");
            let mut js = JSock::new(sock).unwrap();
            println!("Connector end JSock created.");
            
            let v = js.blocking_get(SLEEP_T).unwrap();
            println!("Connector rec'd message:\n{}", v.to_string());
            js.blocking_push(msg_txt1.as_bytes(), SLEEP_T).unwrap();
            println!("Connector has written message.");
            let v = js.blocking_get(SLEEP_T).unwrap();
            println!("Connector rec'd message:\n{}", v.to_string());
            let v = js.blocking_get(SLEEP_T).unwrap();
            println!("Connector rec'd message:\n{}", v.to_string());
            js.shutdown().unwrap();
        });
        
        lthread.join().unwrap();
        cthread.join().unwrap();
        println!("Threads joined.");
    }
}
