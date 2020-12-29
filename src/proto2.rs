/*!
proto2.rs

A newer, simpler, more easily-extensible `grel` protocol, to which it will
eventually switch. This will supersede the `grel::protocol` lib.

2020-12-28
*/

use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Msg {
    
    // Bi-directional messages
    
    /// Typical chunk of text to be exchanged while chatting.
    Text {
        #[serde(default)]
        who: String,
        lines: Vec<String>,
    },
    
    /** Request for or acknowledgement of proof of connection.
    
    If the server hasn't received any data from the client in a while, it will
    send one of these. The client can then respond with one to indicate it's
    still connected.
    */
    Ping,
    
    /** A message from the client indicating it would like to disconnect
    cleanly from the server; in response, the server will send back one of
    these with a message and close the connection.
    */
    Logout(String),
    
    // Client-to-server messages.
    
    /// Name-change request from client to server.
    Name(String),
    
    /// Client request to create/join a room.
    Join(String),
    
    /** A request from the client to the server for some type of information,
    like a list of users matching a pattern.
    */
    Query {
        what: String,
        arg: String,
    },
    
    // Server-to-client messages.
    
    /** A non-error, miscellaneously-informative message sent form the server
    to the client.
    */
    Info(String),
    
    /** A message from the server to the client indicating the client has
    done something wrong, like sent an invalid message.
    */
    Err(String),
    
    Misc {
        what: String,
        data: Vec<String>,
        alt: String,
    },
}

impl Msg {
    /// Convenience functions for instantiating certain types.
    pub fn logout(msg: &str) -> Msg { Msg::Logout(String::from(msg)) }
    pub fn info(msg: &str)   -> Msg { Msg::Info(String::from(msg)) }
    pub fn err(msg: &str)    -> Msg { Msg::Err(String::from(msg)) }
    
    /// Return a JSON-encoded version of a `Msg`.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(&self).unwrap()
    }
}

/** The `Endpoint` enum specifies sources and destinations in an `Env`.
`User`s and `Room`s are stored in respective `HashMap`s with unique `u64`
IDs as keys.
*/
#[derive(Copy, Clone, Debug)]
pub enum Endpoint {
    User(u64),
    Room(u64),
    Server,
    All,
}

/** An `Env` (-elope) wraps the bytes of a JSON-encoded `Msg`, along with
unambiguous source and destination information. This metadata is necessary
because the encoded JSON is opaque to the server without decoding it.
*/
#[derive(Clone, Debug)]
pub struct Env {
    pub source: Endpoint,
    pub dest: Endpoint,
    data: Vec<u8>,
}

impl Env {
    /** Wrap a `Msg`. */
    pub fn new(from: Endpoint, to: Endpoint, msg: &Msg) -> Env {
        Env {
            source: from,
            dest: to,
            data: msg.bytes(),
        }
    }
    
    /** Get a reference to the encoded bytes. */
    pub fn bytes(&self) -> &[u8] { &self.data }
    
    /** Consume the `Env`, returning the owned vector of bytes. */
    pub fn into_bytes(self) -> Vec<u8> { self.data }
}


#[cfg(test)]
mod test {
    use super::*;
    
    fn test_serde(m: &Msg) {
        let stringd = serde_json::to_string_pretty(m).unwrap();
        println!("{}\n", &stringd);
        let newm: Msg = serde_json::from_str(&stringd).unwrap();
        assert_eq!(*m, newm);
    }
    
    #[test]
    fn visual_serde() {
        println!("Msg::Text variant");
        let m = Msg::Text {
            who: String::from("gre luser"),
            lines: vec!["This is a first line of text.".to_string(),
                        "Following the first is a second line of text.".to_string()],
        };
        test_serde(&m);
        
        println!("Msg::Ping variant");
        let m = Msg::Ping;
        test_serde(&m);
        
        println!("Msg::Logout variant");
        let m = Msg::logout("You have been logged out because you touch yourself at night.");
        test_serde(&m);
        
        println!("Msg::Name variant");
        let m = Msg::Name(String::from("New Lewser"));
        test_serde(&m);
        
        println!("Msg::Join variant");
        let m = Msg::Join(String::from("Gay Space Communism"));
        test_serde(&m);
        
        println!("Msg::Query variant");
        let m = Msg::Query {
            what: String::from("who"),
            arg: String::from("fink"),
        };
        test_serde(&m);
        
        println!("Msg::Info variant");
        let m = Msg::info("Santa isn't real.");
        test_serde(&m);
        
        println!("Msg::Err variant");
        let m = Msg::err("Unrecognized Query \"meaning of life\".");
        test_serde(&m);
        
        println!("Msg::Misc variant");
        let m = Msg::Misc {
            what: String::from("roster"),
            data: vec!["you".to_string(), "me".to_string(),
                        "a dog named foo".to_string()],
            alt: String::from("you, me, and a dog named foo"),
        };
        test_serde(&m);
    }
}
