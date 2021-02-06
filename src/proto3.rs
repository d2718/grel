/*!

This third attempt at a protocol uses different structs for _sending_ and
_receiving_ data. Sending structs contain _references_ to data, while
receiving structs _own_ their data. This is to try to reduce unnecessary
allocation when creating structs to send.

2020-02-01
*/

use serde::{Serialize, Deserialize};

#[derive(Clone, Copy, Debug, Serialize)]
pub enum SndOp<'a> {
    Open,
    Close,
    Kick(&'a str),
    Invite(&'a str),
    Give(&'a str),
}

#[derive(Clone, Copy, Debug, Serialize)]
pub enum Sndr<'a> {
    
    Text {
        who: &'a str,
        lines: &'a [&'a str],
    },
    
    Ping,
    Priv { who: &'a str, text: &'a str, },
    Logout(&'a str),
    
    Name(&'a str),
    Join(&'a str),
    Query { what: &'a str, arg: &'a str, },
    Block(&'a str),
    Unblock(&'a str),
    Op(SndOp<'a>),
    
    Info(&'a str),
    Err(&'a str),
    Misc{ what: &'a str, data: &'a [&'a str], alt: &'a str, },
}

impl<'a> Sndr<'_> {
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(&self).unwrap()
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum RcvOp {
    Open,
    Close,
    Kick(String),
    Invite(String),
    Give(String),
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Rcvr {
    
    Text {
        #[serde(default)]
        who: String,
        lines: Vec<String>,
    },
    
    Ping,
    Priv { who: String, text: String, },
    Logout(String),
    
    Name(String),
    Join(String),
    Query { what: String, arg: String, },
    Block(String),
    Unblock(String),
    Op(RcvOp),
    
    Info(String),
    Err(String),
    Misc { what: String,data: Vec<String>, alt: String,  },
}

impl Rcvr {
    pub fn counts(&self) -> bool {
        match self {
            Rcvr::Text { who: _, lines: _ } => true,
            Rcvr::Priv { who: _,  text: _ } => true,
            Rcvr::Name(_) => true,
            Rcvr::Join(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum End {
    User(u64),
    Room(u64),
    Server,
    All,
}

#[derive(Clone, Debug)]
pub struct Env {
    pub source: End,
    pub dest: End,
    data: Vec<u8>,
}

impl<'a> Env {
    pub fn new(from: End, to: End, msg: &'a Sndr) -> Env {
        Env {
            source: from,
            dest: to,
            data: msg.bytes(),
        }
    }
    
    pub fn bytes(&self) -> &[u8] { &self.data }
    pub fn into_bytes(self) -> Vec<u8> { self.data }
}

#[cfg(test)]
mod test {
    use super::*;
    
    fn loose_test<'a>(m: &'a Sndr) {
        let env = Env::new(End::All, End::All, m);
        let md = std::mem::discriminant(m);
        println!("{}\n", std::str::from_utf8(env.bytes()).unwrap());
        let newm: Rcvr = serde_json::from_slice(env.bytes()).unwrap();
        let nd = std::mem::discriminant(&newm);
        println!("{:?}, {:?}\n", md, nd);
    }
    
    #[test]
    fn printy_serde() {
        println!("\n*::Text variant");
        let m = Sndr::Text {
            who: "Some Dude",
            lines: &["This is the first line.",
                     "And this is the second line; it comes after."],
        };
        loose_test(&m);
        
        println!("\n*::Ping variant");
        let m = Sndr::Ping;
        loose_test(&m);
        
        println!("\n*::Priv variant");
        let m = Sndr::Priv {
            who: "naggum",
            text: "XML is basically the Hitler of protocols.",
        };
        loose_test(&m);
        
        println!("\n*::Logout variant");
        let m = Sndr::Logout("You have been logged out because everyone hates you.");
        loose_test(&m);
        
        println!("\n A couple of Op variants");
        let m = Sndr::Op(SndOp::Close);
        loose_test(&m);
        let m = Sndr::Op(SndOp::Kick("FpS DoUg"));
        loose_test(&m);
    }
}
