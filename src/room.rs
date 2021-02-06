/*!
room.rs

The `Room` struct and its associated methods.

updated 2020-02-02

A `Room` roughly parallels an IRC "channel". They can be formed on the
fly from any valid, unique name, and are meant to automatically wink out of
existence when the last person leaves.
*/

use std::collections::HashMap;

use super::proto3::{Env, End};
use super::user::{User, ascollapse};

#[derive(Debug)]
pub struct Room {
    idn: u64,
    name: String,
    idstr: String,
    users: Vec<u64>,
    op: u64,
    pub closed: bool,
    bans: Vec<u64>,
    invites: Vec<u64>,
    inbox: Vec<Env>,
}

impl Room {
    pub fn new(id: u64, new_name: String, creator_id: u64) -> Room {
        Room {
            idn: id,
            idstr: ascollapse(&new_name),
            name: new_name,
            users: Vec::new(),
            op: creator_id,
            closed: false,
            bans: Vec::new(),
            invites: Vec::new(),
            inbox: Vec::new(),
        }
    }
    
    pub fn get_id(&self) -> u64 { self.idn }
    pub fn get_name(&self) -> &str { &(self.name) }
    pub fn get_idstr(&self) -> &str { &(self.idstr) }
    
    pub fn deliver(&self, env: &Env, uid_hash: &mut HashMap<u64, User>) {
        match env.dest {
            End::User(uid) => {
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
                End::User(uid) => {
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
    
    pub fn ban(&mut self, uid: u64) {
        self.invites.retain(|n| *n != uid);
        self.bans.push(uid);
    }
    
    pub fn invite(&mut self, uid: u64) {
        self.bans.retain(|n| *n != uid);
        self.invites.push(uid);
    }
    
    pub fn set_op(&mut self, uid: u64) { self.op = uid; }
    pub fn get_op(&self) -> u64 { self.op }
    
    pub fn get_users(&self) -> &[u64] { &(self.users) }
    
    pub fn is_banned(&self, uid: &u64)  -> bool { self.bans.contains(uid) }
    pub fn is_invited(&self, uid: &u64) -> bool { self.invites.contains(uid) }
}

#[cfg(debug)]
impl Drop for Room {
    fn drop(&mut self) {
        if DEBUG { println!("Room {} ({}) dropping.", self.idn, &(self.name)); }
    }
}
