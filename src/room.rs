/*!
room.rs

The `Room` struct and its associated methods.

updated 2020-11-29

A `Room` roughly parallels an IRC "channel". They can be formed on the
fly from any valid, unique name, and are meant to automatically wink out of
existence when the last person leaves.
*/

const DEBUG: bool = true;

use std::collections::HashMap;

use super::proto2::{Env, Endpoint};
use super::user::{User, ascollapse};

#[derive(Debug)]
pub struct Room {
    idn: u64,
    name: String,
    idstr: String,
    users: Vec<u64>,
    op: u64,
    inbox: Vec<Env>,
}

impl Room {
    pub fn new(id: u64, new_name: String, creator_id: u64) -> Room {
        Room {
            idn: id,
            idstr: ascollapse(&new_name),
            name: new_name,
            users: vec![],
            op: creator_id,
            inbox: Vec::new(),
        }
    }
    
    pub fn get_id(&self) -> u64 { self.idn }
    pub fn get_name(&self) -> &str { &(self.name) }
    pub fn get_idstr(&self) -> &str { &(self.idstr) }
    
    pub fn deliver(&self, env: &Env, uid_hash: &mut HashMap<u64, User>) {
        match env.dest {
            Endpoint::User(uid) => {
                if let Some(u) = uid_hash.get_mut(&uid) { u.deliver(env); }
            },
            _ => {
                for uid in &(self.users) {
                    if let Some(u) = uid_hash.get_mut(uid) { u.deliver(env); }
                }
            },
        }
    }
    
    pub fn enqueue(&mut self, env: Env) { self.inbox.push(env); }
    pub fn deliver_inbox(&mut self, uid_hash: &mut HashMap<u64, User>) {
        for env in self.inbox.drain(..) {
            match env.dest {
                Endpoint::User(uid) => {
                    if let Some(u) = uid_hash.get_mut(&uid) { u.deliver(&env); }
                },
                _ => {
                    for uid in &(self.users) {
                        if let Some(u) = uid_hash.get_mut(&uid) { u.deliver(&env); }
                    }
                },
            }
        }
    }
    
    pub fn join(&mut self, uid: u64) { self.users.push(uid); }
    pub fn leave(&mut self, uid: u64) { self.users.retain(|n| *n != uid); }
    
    pub fn set_op(&mut self, uid: u64) { self.op = uid; }
    pub fn get_op(&self) -> u64 { self.op }
    
    pub fn get_users(&self) -> &[u64] { &(self.users) }
}

impl Drop for Room {
    fn drop(&mut self) {
        if DEBUG { println!("Room {} ({}) dropping.", self.idn, &(self.name)); }
    }
}
