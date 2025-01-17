/*!
The `User` struct--representing a connected client--and related methods.

updated: 2021-02-01

*/

use std::fmt::Display;
use std::time::{Duration, Instant};
use lazy_static::lazy_static;
use super::sock::{Sock, SockError};
//use super::proto2::{Endpoint, Env, Msg};
use super::proto3::{End, Env, Sndr, Rcvr};
use super::unidata::Multichar;

static TICK: Duration = Duration::from_millis(100);

lazy_static!{
    static ref UNIDATA: std::collections::HashMap<u32, Multichar<'static>> =
                            super::unidata::generate_hash();
}

/** The `UserError` signals an error condition in a `User`, generally some
problem with the unerlying socket. Normal, non-blocking `User` sends and
reads won't _return_ errors, but will pile them up in an internal vector,
so that the `User` can be checked for errors at a convenient point in the
event loop and shut down.
*/
#[derive(Clone, Debug)]
pub struct UserError {
    msg: String,
}

impl UserError {
    fn new(message: &str) -> UserError {
        UserError { msg: String::from(message) }
    }
    
    fn from_socket(err: &SockError) -> UserError {
        UserError{
            msg: format!("Underlying socket error: {}", err),
        }
    }
    
    fn from_sockets(err_list: &[SockError]) -> UserError {
        let mut message = format!("{} Underlying socket error(s):", err_list.len());
        for err in err_list.iter() {
            let s = format!("\n  * {}", err);
            message.push_str(&s);
        }
        UserError { msg: message }
    }
}

impl Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "UserError: {}", &(self.msg))
    }
}

impl std::error::Error for UserError {}

/**
The `User` represents a connected client, wrapping the underlying socket and
storing some state.
*/
pub struct User {
    thesock: Sock,
    name: String,
    idn: u64,
    idstr: String,
    bytes_sucked: usize,
    quota_bytes: usize,
    last_data_time: Instant,
    errs: Vec<SockError>,
    blocks: Vec<u64>,
}

impl User {
    pub fn new(new_sock: Sock, new_idn: u64) -> User {
        let new_name = format!("user{}", &new_idn);
        User {
            thesock: new_sock,
            idn: new_idn,
            idstr: ascollapse(&new_name),
            name: new_name,
            bytes_sucked: 0,
            quota_bytes: 0,
            last_data_time: Instant::now(),
            errs: Vec::<SockError>::new(),
            blocks: Vec::<u64>::new(),
        }
    }
    
    pub fn get_name(&self)  -> &str { &(self.name) }
    pub fn get_id(&self)    -> u64  { self.idn }
    pub fn get_idstr(&self) -> &str { &(self.idstr) }
    pub fn get_addr(&mut self) -> Option<String> {
        match self.thesock.get_addr() {
            Ok(a)  => Some(a),
            Err(e) => {
                self.errs.push(e);
                None
            },
        }
    }
    
    pub fn set_name(&mut self, new_name: &str) {
        self.name = String::from(new_name);
        self.idstr = ascollapse(new_name);
    }
    
    /** To implement throttling, the `User` increments and internal byte
    counter whenever certain types of `Msg`s are decoded from the underlying
    socket; this count can be lowered over time.
    
    This method returns the value of that internal counter.
    */
    pub fn get_byte_quota(&self) -> usize { self.quota_bytes }
    
    /** To implement throttling, the `User` increments and internal byte
    counter whenever certain types of `Msg`s are decoded from the underlying
    socket; this method can be used to lower that internal counter.
    */
    pub fn drain_byte_quota(&mut self, amount: usize) {
        if amount > self.quota_bytes {
            self.quota_bytes = 0;
        } else {
            self.quota_bytes -= amount;
        }
    }
    
    /** Returns the time when the last `Msg` was successfuly read from
    the underlying socket.
    */
    pub fn get_last_data_time(&self) -> Instant { self.last_data_time }
    
    // Returns true if any errors have accumulated.
    pub fn has_errors(&self) -> bool { self.errs.len() > 0 }
    
    /** Returns an error wrapping/representing any underlying errors that
    have accumulated. Returns an error even if none have accumulated, so
    it's a good idea to check with `.has_errors()` before calling this.
    */
    pub fn get_errors(&self) -> UserError {
        UserError::from_sockets(&(self.errs))
    }
    
    /** Attempt to send a logout message and close the underlying socket.
    Appropriate for both clean logouts and forced logouts due to errors.
    */
    pub fn logout(&mut self, logout_message: &str) {
        let msg = Sndr::Logout(logout_message);
        self.deliver_msg(&msg);
        let _ = self.thesock.blow();
        let _ = self.thesock.shutdown();
    }
    
    /** Add the ID of a user to the list of users this user has blocked.
    Returns true if the ID was added and false if that ID was already blocked.
    */
    pub fn block_id(&mut self, id: u64) -> bool {
        let res = &(self.blocks).binary_search(&id);
        match res {
            Err(n) => {
                self.blocks.insert(*n, id);
                true
            },
            Ok(_)  => false,
        }
    }
    
    /** Removes the ID of a user from the list of users this user has blocked.
    Returns true if the supplied ID was indeed blocked and false if that user
    wasn't being blocked.
    */
    pub fn unblock_id(&mut self, id: u64) -> bool {
        let res = &(self.blocks).binary_search(&id);
        match res {
            Err(_) => false,
            Ok(n)  => {
                let _ = self.blocks.remove(*n);
                true
            },
        }
    }
    
    /** Add the contents of an `Env` to the outgoing buffer to be sent on
    subesequent calls to `.nudge()` (unless the message originates from a
    blocked user).
    */
    pub fn deliver(&mut self, env: &Env) {
        match env.source {
            End::User(id) => match &(self.blocks).binary_search(&id) {
                Ok(_)  => { /* User is blocked; do not deliver. */ },
                Err(_) => { self.thesock.enqueue(env.bytes()); },
            },
            _ => { self.thesock.enqueue(env.bytes()); },
        }
    }
    
    /** Encode a `Msg` directly into the outgoing buffer, regardless of
    origin.
    */
    pub fn deliver_msg(&mut self, msg: &Sndr) {
        self.thesock.enqueue(&(msg.bytes()));
    }
    
    /** Attempt to write bytes from the outgoing buffer to the underlying
    socket. Any errors will be added to an internal `Vec` and not returned.
    */
    pub fn nudge(&mut self) {
        if self.thesock.send_buff_size() > 0 {
            if let Err(e) = self.thesock.blow() {
                self.errs.push(e);
            }
        }
    }
    
    /** Encode a message directly to the outgoing buffer, and then continually
    attempt to write bytes to the underlying socket until either the buffer
    is empty or `limit` has passed.
    
    Unlike the nonblocking sends, this _will_ return an error if encountered,
    _or_ if `limit` passes without the buffer emptying.
    */
    pub fn blocking_send(&mut self, msg: &Sndr, limit: Duration) -> Result<(), UserError> {
        self.deliver_msg(msg);
        let start_t = Instant::now();
        loop {
            match self.thesock.blow() {
                Err(e) => {
                    let err = UserError::from_socket(&e);
                    self.errs.push(e);
                    return Err(err);
                },
                Ok(n) => {
                    if n == 0 { return Ok(()); }
                },
            }
            if start_t.elapsed() > limit { 
                return Err(UserError::new("Timed out on blocking send."));
            } else {
                std::thread::sleep(TICK);
            }
        }
    }
    
    /* Attempt to read data and decode a `Msg` from the underlying socket.
    Any errors will be added to an internal `Vec` and not returned.
    */
    pub fn try_get(&mut self) -> Option<Rcvr> {
        match self.thesock.suck() {
            Err(e) => {
                self.errs.push(e);
                return None;
            },
            Ok(n) => { self.bytes_sucked = self.bytes_sucked + n; },
        }
        
        let n_buff = self.thesock.recv_buff_size();
        if n_buff > 0 {
            match self.thesock.try_get() {
                Err(e) => {
                    self.errs.push(e);
                    return None;
                },
                Ok(msg_opt) => {
                    self.last_data_time = Instant::now();
                    // If it's a noisy message, increment byte quota.
                    if let Some(ref m) = msg_opt {
                        if m.counts() {
                            self.quota_bytes += n_buff - self.thesock.recv_buff_size();
                        }
                    }
                    return msg_opt;
                },
            }
        } else {
            return None;
        }
    }
    
    /** Continually attempt to read and decode a `Msg` from the underlying
    socket until either successful, or `limit` has passed.
    
    Unlike the nonblocking reads, this _will_ return an error if encountered,
    or if `limit` goes by without successfully decoding a `Msg`.
    */
    pub fn blocking_get(&mut self, limit: Duration) -> Result<Rcvr, UserError> {
        match self.thesock.try_get() {
            Err(e) => {
                let err = UserError::from_socket(&e);
                self.errs.push(e);
                return Err(err);
            },
            Ok(msg_opt) => {
                if let Some(m) = msg_opt {
                    return Ok(m);
                }
            },
        }
        
        let start_t = Instant::now();
        loop {
            match self.thesock.suck() {
                Err(e) => {
                    let err = UserError::from_socket(&e);
                    self.errs.push(e);
                    return Err(err);
                },
                Ok(n) => {
                    if n > 0 {
                        match self.thesock.try_get() {
                            Err(e) => {
                                let err = UserError::from_socket(&e);
                                self.errs.push(e);
                                return Err(err);
                            },
                            Ok(opt) => match opt {
                                Some(m) => { return Ok(m); }
                                None => {},
                            },
                        }
                    }
                },
            }
            if start_t.elapsed() > limit {
                return Err(UserError::new("Timed out on a blocking get."));
            } else {
                std::thread::sleep(TICK);
            }
        }
    }
}

#[cfg(debug)]
impl Drop for User {
    fn drop(&mut self) {
        if DEBUG {
            println!("User {} ({}) dropping.", self.idn, &(self.name));
        }
    }
}

/** Collapse a string of characters (e.g., a `User` or `Room` name) into a
"collapsed" representation that's convenient to type and parse.

This involves removing whitespace, capitalization, and diacritics.
*/
pub fn ascollapse(s: &str) -> String {
    let mut r = String::new();
    for c in s.to_lowercase().chars().filter(|x| !x.is_whitespace()) {
        match UNIDATA.get(&(c as u32)) {
            None      => { r.push(c); },
            Some(mch) => {
                let base_c = unsafe { std::char::from_u32_unchecked(mch.base) };
                r.push(base_c);
            }
        }
    }
    return r;
}
