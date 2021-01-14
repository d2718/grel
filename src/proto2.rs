/*!
proto2.rs

A newer, simpler, more easily-extensible `grel` protocol. As of 2020-12-29,
this supersedes the `grel::protocol` lib.

2020-101-14
*/

use serde::{Serialize, Deserialize};

/** The `Op` enum represents one of the `Room` operator subcommands. It is
used in the `Msg::Op(...)` variant of the `Msg` enum.
*/
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    
    /** Open the current Room, allowing in the general public. */
    Open,
    
    /** Close the current Room to anyone who hasn't been specifically
    `Invite`d. */
    Close,
    
    /** Ban the user with the supplied user name from the Room (even if
    it's `Open`), removing him if he's currently in it. */
    Kick(String),
    
    /** Allow the user to enter the current room, even if it's `Close`d.
    Also sends an invitation message to the user. */
    Invite(String),
    
    /** Transfer operatorship to another user. (The user must be in the
    current room to receive the mantle of operatorship.) */
    Give(String),
}

/** The `Msg` enum is the structure that gets serialized to JSON and passed
along the TCP connections between the server and the various clients.

The first four variants, `Text`, `Ping`, `Priv` and `Logout` are
bi-directional, being used to send similar information both from client
to server and server to client.

The next six, `Name`, `Join`, `Query`, `Block`, `Unblock`, and `Op` are
for sending commands or requests from the client to the server.

The final three, `Info`, `Err`, and `Misc` are used to send information
from the server back to the client.
*/
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
    
    /** A private message delivered only to the recipient.
    
    When sent from the client to the server, `who` should be an identifier
    for the _recipient_; when sent server to client, `who` is the name of
    the source.
    */
    Priv {
        who: String,
        text: String,
    },
    
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
    
    /** A request from the client to block messages (including private
    messages) from the user with the matching name. */
    Block(String),
    
    /** A request to unblock the given user. */
    Unblock(String),
    
    /** One of the Room operator subcommands. See the `proto2::Op` enum. */
    Op(Op),
    
    // Server-to-client messages.
    
    /** A non-error, miscellaneously-informative message sent form the server
    to the client.
    */
    Info(String),
    
    /** A message from the server to the client indicating the client has
    done something wrong, like sent an invalid message.
    */
    Err(String),
    
    /**
    The `Misc` variant represents information that the client may want to
    display in a structured manner (and not just as an unadorned line of
    text). For any given "type" of `Misc` message, the client is free to
    either implement its own form of displaying the information, or to
    just use the contents of the provided `.alt` field.
    
    Current Misc variants (with example field values):
    
    ``` ignore
    // in response to a Query { what: "roster". ... }
    Misc {
        what: "roster".to_string(),
        data: vec!["user1".to_string(), "user2".to_string()], # ...
        alt: "[ comma-delimited list of Users in room ]".to_string(),
    };
    
    // when a user joins a channel
    Misc {
        what: "join".to_string(),
        data: vec!["grel user".to_string(),
                   "room name".to_string()],
        alt: "grel user joins [room name]".to_string(),
    };
    
    // when a user logs out or leaves a channel
    Misc {
        what: "leave".to_string(),
        data: vec!["grel user".to_string(),
                   "moved to another room".to_string()],
        alt: "grel user moved to another room".to_string(),
    };
    
    // when a user changes his or her name
    Misc {
        what: "name".to_string(),
        data: vec!["old name".to_string(),
                   "new name".to_string()],
        alt: "\"old name\" is now known as \"new name\".".to_string(),
    };
    
    // in response to a Query { what: "addr", ... }
    Misc {
        what: "addr".to_string(),
        data: vec!["127.0.0.1:33333".to_string()]
        alt: "Your public address is 127.0.0.1:33333".to_string(),
    };
    
    // in response to a Query { what: "who", ... }
    Misc {
        what: "who".to_string(),
        data: vec!["user1".to_string(), "user2".to_string(), ... ],
        alt: "Matching names: \"user1\", \"user2\", ...".to_string(),
    };
    
    // echoes a private message back to the sender
    Misc {
        what: "priv_echo".to_string(),
        data: vec!["recipient".to_string(), "text of message".to_string()],
        alt: "$ You @ Recipient: text of message".to_string()
    };
    ```
    */
    
    Misc {
        what: String,
        data: Vec<String>,
        alt: String,
    },
}

/** Some of these are convenience functions for instantiating certain
variants.
*/
impl Msg {
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
        
        println!("Msg::Priv variant");
        let m = Msg::Priv {
            who: String::from("naggum"),
            text: String::from("XML is bascially the Hitler of protocols."),
        };
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
        
        println!("Msg::Block variant");
        let m = Msg::Block(String::from("Dickweed User"));
        test_serde(&m);
        
        println!("Msg::Unblock variant");
        let m = Msg::Unblock(String::from("Misunderstood User"));
        test_serde(&m);
        
        println!("A couple of Msg::Op variants");
        let m = Msg::Op(Op::Close);
        test_serde(&m);
        let m = Msg::Op(Op::Kick("FpS DoUgG".to_string()));
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
