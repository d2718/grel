/*!
protocol.rs

The original, deprecated `grel` communication protocol and its associated
types. The current version of `grel` and `greld` use the `grel::proto2`
module.

updated 2020-12-29
*/

use serde::{Serialize, Deserialize};

/**
A `protocol::Msg` represents a single atom of communication between the `grel`
server and a client. These get encoded to and decoded from JSON to be sent.
*/
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// #[serde(tag = "type")]
pub enum Msg {
    
    /// The typical chunk of text that would be exchanged while chatting.
    Text {
        #[serde(default)]
        who: String,
        lines: Vec<String>
    },
    
    /** A name-change message, either a request from a client to change that
    connected user's display name, or a message from the server informing
    other users of a name change.
    */
    Name {
        #[serde(default)]
        who: String,
        new: String
    },
    
    /** A Room join message, either from the server informing Room members
    of someone joining, or from a client, expressing a desire to create/join
    the supplied room.
    */
    Join {
        #[serde(default)]
        who: String,
        #[serde(default)]
        what: String,
    },
    
    /// A message from the server informing Room members someone is leaving.
    Leave {
        #[serde(default)]
        who: String,
        message: String
    },
    
    /** Request for or acknowledgement of proof of connection. If the server
    hasn't received any data from a client for a while, it will send one of
    these. The client can then respond with one to indicate it's still
    connected.
    */
    Ping,
    
    /** A message from the client indicating it would like to disconnect
    cleanly from the server; in response, the server will send one of these
    and then close the connection.
    */
    Logout(String),
    
    /** A list of individual values, usually in response to a request for
    some type of information.
    */
    List {
        what: String,
        items: Vec<String>,
    },
    
    
    /** A request from the client to the server for some type of information,
    like a list of users matching a pattern.
    */
    
    Query {
        what: String,
        arg: String,
    },
    
    /** A non-error, miscellaneously-informative message sent from the
    server to the client.
    */
    Info(String),
    
    /** A message from the server to the client indicating it has done
    something wrong, like sent an invalid message.
    */
    Err(String),
}

impl Msg {
    /// Convenience function for instantiating a `Msg::Logout`
    pub fn logout(msg: &str) -> Msg { Msg::Logout(String::from(msg)) }
    
    /// Convenience function for instantiating a `Msg::Info`
    pub fn info(msg: &str) -> Msg { Msg::Info(String::from(msg)) }
    
    /// Convenience function for instantiating a 'Msg::Err`
    pub fn err(msg: &str) -> Msg { Msg::Err(String::from(msg)) }
    
    /// Return the JSON-encoded version of a `Msg`.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).unwrap()
    }
}

/**
 * The `Endpoint` enum specifies sources and destinations in an `Env`.
 * Users and Rooms are stored in respective `HashMap`s with unique `u64`
 * IDs as keys.
 */ 
#[derive(Copy, Clone, Debug)]
pub enum Endpoint {
    User(u64),
    Room(u64),
    Server,
    All,
}

/**
 * An `Env` wraps the bytes of a JSON-encoded message, along with
 * unambiguous source and destination information. This metadata is necessary
 * because the encoded JSON is opaque to the server without decoding.
 */
#[derive(Clone, Debug)]
pub struct Env {
    pub source: Endpoint,
    pub dest: Endpoint,
    data: Vec<u8>,
}

impl Env {
    /** Wrap a Msg. */
    pub fn new(from: Endpoint, to: Endpoint, msg: &Msg) -> Env {
        Env {
            source: from,
            dest: to,
            data: msg.bytes(),
        }
    }
    
    /** Get a reference to the encoded bytes. */
    pub fn bytes(&self) -> &[u8] {
        &(self.data)
    }
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
    fn direct_serde_test() {
        println!("Msg::Text variant");
        let mut test_linez: Vec<String> = Vec::new();
        test_linez.push(String::from("This is a first line of text."));
        test_linez.push(String::from("Following is a second line of text."));
        let m = Msg::Text {
            who: String::from("Some User"),
            lines: test_linez,
        };
        
        test_serde(&m);
        
        println!("Msg::Name variant");
        let m = Msg::Name {
            who: String::from("Some User"),
            new: String::from("s0m3 u54r"),
        };
        test_serde(&m);
        
        println!("Msg::Join variant");
        let m = Msg::Join {
            who: String::from("Luser87"),
            what: String::from("Room About Nothing"),
        };
        test_serde(&m);
        
        println!("Msg::Leave variant");
        let m = Msg::Leave {
            who: String::from("Dude Leaving"),
            message: String::from("Bye, all!"),
        };
        test_serde(&m);
        
        println!("Msg::Ping variant");
        let m = Msg::Ping;
        test_serde(&m);
        
        println!("Msg::Logout variant");
        let m = Msg::Logout(String::from("You have been logged out because of an error."));
        test_serde(&m);
        
        println!("Msg::List variant");
        let mut test_names: Vec<String> = Vec::new();
        test_names.push(String::from("William1934"));
        test_names.push(String::from("D00fu5 f4c3"));
        test_names.push(String::from("xXx _ c0o1i0z _ xXx"));
        let m = Msg::List {
            what: String::from("users"),
            items: test_names,
        };
        test_serde(&m);
        
        println!("Msg::Query variant");
        let m = Msg::Query {
            what: String::from("finger"),
            arg: String::from("user1234"),
        };
        test_serde(&m);
        
        println!("Msg::Info variant");
        let m = Msg::Info(String::from("That is a silly request, and the server will not honor it."));
        test_serde(&m);
        
        println!("Msg::Err variant");
        let m = Msg::Err("Improper protocol: First message must be of type \"Name\".".to_string());
        test_serde(&m);
    }
}
