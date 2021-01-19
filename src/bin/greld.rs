/*!
greld.rs

The `grel` daemon (server) process.

updated 2021-01-19
*/

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};
use log::{debug, warn, trace};
use simplelog::WriteLogger;

use grel::proto2::*;
use grel::user::*;
use grel::room::Room;
use grel::sock::Sock;
use grel::config::ServerConfig;

// const DEBUG: bool = true;

static BLOCK_TIMEOUT: Duration = Duration::from_millis(5000);
static BYTES_PER_TICK: usize = 6;

/* The Context is instantiated in process_room() and passed to each of
the functions that handles receiving messages from clients.
*/
struct Context<'a>{
    rid: u64,
    uid: u64,
    umap: &'a mut HashMap<u64, User>,
    ustr: &'a mut HashMap<String, u64>,
    rmap: &'a mut HashMap<u64, Room>,
    rstr: &'a mut HashMap<String, u64>,
}

impl Context<'_> {
    fn gumap(&self, uid: u64) -> Result<&User, String> {
        match self.umap.get(&uid) {
            None => Err(format!("{:?}.gumap(&{}) returns None", &self, &uid)),
            Some(u) => Ok(u),
        }
    }
    
    fn grmap(&self, rid: u64) -> Result<&Room, String> {
        match self.rmap.get(&rid) {
            None => Err(format!("{:?}.grmap(&{}) return None", &self, &rid)),
            Some(r) => Ok(r),
        }
    }
    
    fn gumap_mut(&mut self, uid: u64) -> Result<&mut User, String> {
        if let Some(u) = self.umap.get_mut(&uid) {
            return Ok(u);
        }
        return Err(format!("Context {{ rid: {}, uid: {} }}.gumap_mut(&{}) returns None",
                            self.rid, self.uid, &uid));
    }
    
    fn grmap_mut(&mut self, rid: u64) -> Result<&mut Room, String> {
        if let Some(r) = self.rmap.get_mut(&rid) {
            return Ok(r);
        }
        return Err(format!("Context {{ rid: {}, uid: {} }}.grmap_mut(&{}) returns None",
                            self.rid, self.uid, &rid));
    }
    
    fn gustr(&self, u_idstr: &str) -> Option<u64> { 
        if let Some(n) = self.ustr.get(u_idstr) { Some(*n) } else { None }
    }
    fn grstr(&self, r_idstr: &str) -> Option<u64> { 
        if let Some(n) = self.rstr.get(r_idstr) { Some(*n) } else { None }
    }
}

impl std::fmt::Debug for Context<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("rid", &self.rid)
            .field("uid", &self.uid)
            .finish()
    }
}

fn match_string<T>(s: &str, hash: &HashMap<String, T>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for k in hash.keys() {
        if k.starts_with(s) { v.push(String::from(k)); }
    }
    return v;
}

fn append_comma_delimited_list<T: AsRef<str>>(base: &mut String, v: &[T]) {
    let mut v_iter = v.iter();
    if let Some(x) = v_iter.next() { base.push_str(x.as_ref()); }
    while let Some(x) = v_iter.next() {
        base.push_str(", ");
        base.push_str(x.as_ref());
    }
}

fn initial_negotiation(u: &mut User) -> Result<(), String> {
    match u.blocking_get(BLOCK_TIMEOUT) {
        Err(e) => {
            let err_str = format!("Error reading initial \"Name\" message: {}", e);
            u.logout(&err_str);
            return Err(err_str);
        },
        Ok(m) => match m {
            Msg::Name(new_name) => {
                u.set_name(&new_name);
                return Ok(());
            },
            x => {
                u.logout("Protocol error: Initial message should be of type \"Name\".");
                return Err(format!("Bad initial message: {:?}", &x));
            },
        },
    }
}

fn listen(addr: String, tx: mpsc::Sender<User>) {
    /* Lowest possible uid is 100 */
    let mut new_user_id: u64 = 100;
    let listener = TcpListener::bind(&addr).unwrap();
    for res in listener.incoming() {
        match res {
            Err(e) => { debug!("listen(): Error accepting connection: {}", &e); },
            Ok(stream) => {
                debug!("listen(): Accepted connection from {:?}", stream.peer_addr().unwrap());
                let new_sock: Sock;
                match Sock::new(stream) {
                    Err(e) => {
                        debug!("listen(): Error setting up new Sock: {}", &e);
                        continue;
                    },
                    Ok(x) => { new_sock = x; },
                }
                let mut u = User::new(new_sock, new_user_id);
                match initial_negotiation(&mut u) {
                    Err(e) => { debug!("listen(): Error negotiating initial protocol: {}", &e); },
                    Ok(()) => {
                        debug!("listen(): Sending new client \"{}\" through channel.", u.get_name());
                        if let Err(e) = tx.send(u) {
                            debug!("listen(): Error sending client through channel: {}", &e);
                        } else {
                            new_user_id = new_user_id + 1;
                        }
                    },
                }
            },
        }
    }
}

fn first_free_id<T: Sized>(map: &HashMap<u64, T>) -> u64 {
    let mut n: u64 = 0;
    while let Some(_) = map.get(&n) { n = n + 1; }
    return n;
}

/*

The next several functions are called during `process_room(...)` in response
to the various types of `proto2::Msg` pulled out of a given user's `sock`.

*/

/// In response to Msg::Text{ _, lines }

fn do_text(ctxt: &mut Context, lines: Vec<String>)
-> Result<Vec<Env>, String> {
    let u = ctxt.gumap(ctxt.uid)?;
    
    let msg = Msg::Text {
        who: u.get_name().to_string() ,
        lines: lines,
    };
    let env = Env::new(
        Endpoint::User(ctxt.uid),
        Endpoint::Room(ctxt.rid),
        &msg);
        
    return Ok(vec![env]);
}

/// In response to Msg::Priv { who, text }

fn do_priv(ctxt: &mut Context, who: String, text: String)
-> Result<Vec<Env>, String> {
    let u = ctxt.gumap(ctxt.uid)?;
    
    let to_tok = ascollapse(&who);
    if to_tok.len() == 0 {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("The recipient name must have at least one non-whitespace character."));
        return Ok(vec![env]);
    }
    
    let tgt_uid = match ctxt.gustr(&to_tok) {
        None => {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Err(format!("There is no user whose name matches \"{}\".", &to_tok)));
            return Ok(vec![env]);
        },
        Some(n) => n,
    };
    let tgt_u = ctxt.gumap(tgt_uid)?;
    
    let echo_env = Env::new(
        Endpoint::Server,
        Endpoint::User(ctxt.uid),
        &Msg::Misc {
            what: "priv_echo".to_string(),
            data: vec![tgt_u.get_name().to_string(), text.clone()],
            alt: format!("$ You @ {}: {}", tgt_u.get_name(), &text),
        });
    let to_env = Env::new(
        Endpoint::User(ctxt.uid),
        Endpoint::User(tgt_uid),
        &Msg::Priv {
            who: u.get_name().to_string(),
            text: text,
        });
    
    return Ok(vec![echo_env, to_env]);
}

/// In response to Msg::Name(new_candidate)

fn do_name(ctxt: &mut Context, cfg: &ServerConfig, new_candidate: String)
-> Result<Vec<Env>, String> {
    let new_str = ascollapse(&new_candidate);
    if new_str.len() == 0 {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("Your name must have more whitespace characters."));
        return Ok(vec![env]);
    } else if new_candidate.len() > cfg.max_user_name_length {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::Err(format!("Your name cannot be longer than {} characters.",
                              &cfg.max_user_name_length)));
        return Ok(vec![env]);
    }
    
    if let Some(ouid) = ctxt.ustr.get(&new_str) {
        let ou = ctxt.gumap(*ouid)?;
        if *ouid != ctxt.uid {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Err(format!("There is already a user named \"{}\".",
                                  ou.get_name())));
            return Ok(vec![env]);
        }
    }
    
    /* The last part of this function is a little wonky. An extra scope
    with some uninitialized upvals are introduced to work around the
    mutable borrow of `mu` from `ctxt.gumap_mut()`.
    */
    
    let old_idstr: String;
    let new_idstr: String;
    let env: Env;
    {
        let mu = ctxt.gumap_mut(ctxt.uid)?;
        let old_name = mu.get_name().to_string();
        old_idstr = mu.get_idstr().to_string();
        
        mu.set_name(&new_candidate);
        new_idstr = mu.get_idstr().to_string();
        
        env = Env::new(
            Endpoint::Server,
            Endpoint::Room(ctxt.rid),
            &Msg::Misc {
                what: "name".to_string(),
                alt: format!("{} is now known as {}.", &old_name, &new_candidate),
                data: vec![old_name, new_candidate.clone()],
            });
    }
    let _ = ctxt.ustr.remove(&old_idstr);

    ctxt.ustr.insert(new_idstr, ctxt.uid);
    return Ok(vec![env]);
}

/// In response to Msg::Join(room_name)

fn do_join(ctxt: &mut Context, cfg: &ServerConfig, room_name: String)
-> Result<Vec<Env>, String> {
    let collapsed = ascollapse(&room_name);
    if collapsed.len() == 0 {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("A room name must have more non-whitespace characters."));
        return Ok(vec![env]);
    } else if room_name.len() > cfg.max_room_name_length {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::Err(format!("Room names cannot be longer than {} characters.",
                              &cfg.max_room_name_length)));
        return Ok(vec![env]);
    }
    
    let tgt_rid = match ctxt.grstr(&collapsed) {
        Some(n) => n,
        None => {
            let new_id = first_free_id(&ctxt.rmap);
            let new_room = Room::new(new_id, room_name.clone(), ctxt.uid);
            ctxt.rstr.insert(collapsed, new_id);
            ctxt.rmap.insert(new_id, new_room);
            let mu = ctxt.gumap_mut(ctxt.uid)?;
            let create_msg = Msg::Info(format!("You create room \"{}\".", &room_name));
            mu.deliver_msg(&create_msg);
            new_id
        },
    };
    
    let uname: String;
    let uid = ctxt.uid;
    let rid = ctxt.rid;
    {
        let u = ctxt.gumap(ctxt.uid)?;
        uname = u.get_name().to_string();
    }
    
    {
        let targ_r = ctxt.grmap_mut(tgt_rid)?;
        if tgt_rid == rid {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(uid),
                &Msg::Info(format!("You are already in \"{}\".", targ_r.get_name())));
            return Ok(vec![env]);
        } else if targ_r.is_banned(&uid) {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(uid),
                &Msg::Info(format!("You are banned from \"{}\".", targ_r.get_name())));
            return Ok(vec![env]);
        } else if targ_r.closed && !targ_r.is_invited(&uid) {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(uid),
                &Msg::Info(format!("\"{}\" is closed.", targ_r.get_name())));
            return Ok(vec![env]);
        }
        targ_r.join(uid);
        let join_env = Env::new(
            Endpoint::Server,
            Endpoint::Room(tgt_rid),
            &Msg::Misc {
                what: "join".to_string(),
                data: vec![uname.clone(), targ_r.get_name().to_string()],
                alt: format!("{} joins {}.", &uname, targ_r.get_name()),
            });
        targ_r.enqueue(join_env);
    }
    
    let cur_r = ctxt.grmap_mut(ctxt.rid)?;
    
    let leave_env = Env::new(
        Endpoint::Server,
        Endpoint::Room(tgt_rid),
        &Msg::Misc {
            what: "leave".to_string(),
            alt: format!("{} moved to another room.", &uname),
            data: vec![uname, "[ moved to another room ]".to_string()],
        });
    cur_r.leave(uid);
    return Ok(vec![leave_env]);
}

/// In response to Msg::Block(user_name)

fn do_block(ctxt: &mut Context, user_name: String)
-> Result<Vec<Env>, String> {
    let collapsed = ascollapse(&user_name);
    if collapsed.len() == 0 {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("That cannot be anyone's user name."));
        return Ok(vec![env]);
    }
    let ouid = match ctxt.ustr.get(&collapsed) {
        None => {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
            return Ok(vec![env]);
        },
        Some(n) => *n,
    };
    if ouid == ctxt.uid {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("You shouldn't block yourself."));
        return Ok(vec![env]);
    }
    
    let blocked_name = match ctxt.umap.get(&ouid) {
        None => { return Err(format!("do_block(r {}, u {}): no target User {}", ctxt.rid, ctxt.uid, ouid)); },
        Some(u) => u.get_name().to_string(),
    };
    
    let mu = ctxt.gumap_mut(ctxt.uid)?;
    let msg = match mu.block_id(ouid) {
        true => Msg::Info(format!("You are now blocking {}.", &blocked_name)),
        false => Msg::Err(format!("You are already blocking {}.", &blocked_name)),
    };
    mu.deliver_msg(&msg);
    
    return Ok(vec![]);
}

/// In response to Msg::Unblock(user_name)

fn do_unblock(ctxt: &mut Context, user_name: String)
-> Result<Vec<Env>, String> {
    let collapsed = ascollapse(&user_name);
    if collapsed.len() == 0 {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("That cannot be anyone's user name."));
        return Ok(vec![env]);
    }
    let ouid = match ctxt.ustr.get(&collapsed) {
        None => {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
            return Ok(vec![env]);
        },
        Some(n) => *n,
    };
    if ouid == ctxt.uid {
        let env = Env::new(
            Endpoint::Server,
            Endpoint::User(ctxt.uid),
            &Msg::err("You couldn't block yourself; you can't unblock yourself."));
        return Ok(vec![env]);
    }
    
    let blocked_name = match ctxt.umap.get(&ouid) {
        None => { return Err(format!("do_unblock(r {}, u {}): no target User {}", ctxt.rid, ctxt.uid, ouid)); },
        Some(u) => u.get_name().to_string(),
    };
    
    let mu = ctxt.gumap_mut(ctxt.uid)?;
    let msg = match mu.unblock_id(ouid) {
        true => Msg::Info(format!("You unblock {}.", &blocked_name)),
        false => Msg::Err(format!("You were not blocking {}.", &blocked_name)),
    };
    mu.deliver_msg(&msg);
    
    return Ok(vec![]);
}

/// In response to Msg::Logout(salutation)

fn do_logout(ctxt: &mut Context, salutation: String)
-> Result<Vec<Env>, String> {
    let mr = match ctxt.rmap.get_mut(&ctxt.rid) {
        None => { return Err(format!("do_logout(r {}, u {}): no Room {}", ctxt.rid, ctxt.uid, ctxt.rid)); },
        Some(r) => r,
    };
    mr.leave(ctxt.uid);
    
    let mut mu = match ctxt.umap.remove(&ctxt.uid) {
        None => { return Err(format!("do_logout(r {}, u {}): no User {}", ctxt.rid, ctxt.uid, ctxt.uid)); },
        Some(u) => u,
    };
    let _ = ctxt.ustr.remove(mu.get_idstr());
    mu.logout("You have logged out.");
    
    let env = Env::new(
        Endpoint::Server,
        Endpoint::Room(ctxt.rid),
        &Msg::Misc {
            what: "leave".to_string(),
            alt: format!("{} leaves: {}", mu.get_name(), &salutation),
            data: vec![mu.get_name().to_string(), salutation],
        });
    mr.enqueue(env);
    
    Ok(vec![])
}

/// In response to Msg::Query { what, arg }

fn do_query(ctxt: &mut Context, what: String, arg: String)
-> Result<Vec<Env>, String> {
    match what.as_str() {
        "addr" => {
            let mu = ctxt.gumap_mut(ctxt.uid)?;
            let (addr_str, alt_str): (String, String) = match mu.get_addr() {
                None => ("???".to_string(),
                    "Your public address cannot be determined.".to_string()),
                Some(s) => {
                    let astr = format!("Your public address is {}.", &s);
                    (s, astr)
                },
            };
            let msg = Msg::Misc {
                what: "addr".to_string(),
                data: vec![addr_str],
                alt: alt_str,
            };
            mu.deliver_msg(&msg);
            return Ok(vec![]);
        },
        
        "roster" => {
            let r = ctxt.grmap(ctxt.rid)?; 
            let op_id = r.get_op();
            let mut names_list: Vec<String> = Vec::with_capacity(r.get_users().len());
            
            for uid in r.get_users().iter().rev() {
                if *uid != op_id {
                    match ctxt.umap.get(uid) {
                        None => { warn!("do_query(r {}, u{} {:?}): no User {}",
                                       ctxt.rid, ctxt.uid, &what, uid);
                        },
                        Some(u) => { names_list.push(u.get_name().to_string()); },
                    }
                }
            }
            
            let mut altstr: String;
            /* The lobby will never have an operator. It's operator uid is
            set to 0 (the lowest possible uid is 100). */
            if op_id == 0 {
                altstr = format!("{} roster: ", r.get_name());
                append_comma_delimited_list(&mut altstr, &names_list);
            } else {
                let op_name = match ctxt.umap.get(&op_id) {
                    None => "[ ??? ]".to_string(),
                    Some(u) => u.get_name().to_string(),
                };
                altstr = format!("{} roster: {} (operator) ", r.get_name(), &op_name);
                append_comma_delimited_list(&mut altstr, &names_list);
                names_list.push(op_name);
            }

            let names_list = names_list.into_iter().rev().collect();
            
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Misc {
                    what: "roster".to_string(),
                    data: names_list,
                    alt: altstr,
                });
            return Ok(vec![env]);
        },
        
        "who" => {
            let collapsed = ascollapse(&arg);
            let matches = match_string(&collapsed, ctxt.ustr);
            let env: Env;
            if matches.len() == 0 {
                env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
            } else {
                let mut altstr = String::from("Matching names: ");
                append_comma_delimited_list(&mut altstr, &matches);
                env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Misc {
                        what: "who".to_string(),
                        data: matches,
                        alt: altstr,
                    });
            }
            return Ok(vec![env]);
        },
        
        "rooms" => {
            let collapsed = ascollapse(&arg);
            let matches = match_string(&collapsed, ctxt.rstr);
            let env: Env;
            if matches.len() == 0 {
                env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Info(format!("No Rooms matching the pattern \"{}\".", &collapsed)));
            } else {
                let mut altstr = String::from("Matching Rooms: ");
                append_comma_delimited_list(&mut altstr, &matches);
                env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Misc {
                        what: "rooms".to_string(),
                        data: matches,
                        alt: altstr,
                    });
            }
            return Ok(vec![env]);
        },
        
        ukn @ _ => {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::Err(format!("Unknown \"Query\" type: \"{}\".", ukn)));
            return Ok(vec![env]);
        },
    }
}

/// In response to Msg::Op(op)

fn do_op(ctxt: &mut Context, op: Op)
-> Result<Vec<Env>, String> {
    {
        let r = ctxt.grmap(ctxt.rid)?;
        if r.get_op() != ctxt.uid {
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &Msg::err("You are not the operator of this Room."));
            return Ok(vec![env]);
        }
    }
    
    let (uid, rid) = (ctxt.uid, ctxt.rid);
    let op_name = {
        let u = ctxt.gumap(ctxt.uid)?;
        u.get_name().to_string()
    };
    
    match op {
        Op::Open => {
            let cur_r = ctxt.grmap_mut(rid)?;
            if cur_r.closed {
                cur_r.closed = false;
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::Room(rid),
                    &Msg::Info(format!("{} has opened {}.", &op_name, cur_r.get_name())));
                return Ok(vec![env]);
            } else {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(uid),
                    &Msg::Info(format!("{} is already open.", cur_r.get_name())));
                return Ok(vec![env]);
            }
        },
        
        Op::Close => {
            let cur_r = ctxt.grmap_mut(rid)?;
            if cur_r.closed {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(uid),
                    &Msg::Info(format!("{} is already closed.", cur_r.get_name())));
                return Ok(vec![env]);
            } else {
                cur_r.closed = true;
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::Room(rid),
                    &Msg::Info(format!("{} has closed {}.", &op_name, cur_r.get_name())));
                return Ok(vec![env]);
            }
        },
        
        Op::Give(ref new_name) => {
            let collapsed = ascollapse(&new_name);
            if collapsed.len() == 0 {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::err("That cannot be anyone's user name."));
                return Ok(vec![env]);
            }
            
            let ouid = match ctxt.ustr.get(&collapsed) {
                None => {
                    let env = Env::new(
                        Endpoint::Server,
                        Endpoint::User(ctxt.uid),
                        &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
                    return Ok(vec![env]);
                },
                Some(n) => *n,
            };
            
            if ouid == ctxt.uid {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::info("You are already the operator of this room."));
                return Ok(vec![env]);
            }
            
            let ou_name = {
                let u = ctxt.gumap(ouid)?;
                u.get_name().to_string()
            };
            
            let cur_r = ctxt.grmap_mut(rid)?;
            if !cur_r.get_users().contains(&ouid) {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Info(format!("{} must be in the room to transfer ownership.", &ou_name)));
                return Ok(vec![env]);
            }
            cur_r.set_op(ouid);
            let env = Env::new(
                Endpoint::Server,
                Endpoint::Room(ctxt.rid),
                &Msg::Info(format!("The room operator is now {}.", &ou_name)));
            return Ok(vec![env]);
        },

        Op::Invite(ref uname) => {
            let collapsed = ascollapse(&uname);
            if collapsed.len() == 0 {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::info("That cannot be anyone's user name."));
                return Ok(vec![env]);
            }
            
            let ouid = match ctxt.ustr.get(&collapsed) {
                None => {
                    let env = Env::new(
                        Endpoint::Server,
                        Endpoint::User(ctxt.uid),
                        &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
                    return Ok(vec![env]);
                },
                Some(n) => *n,
            };

            let cur_r = match ctxt.rmap.get_mut(&ctxt.rid) {
                None => { return Err(format!("do_op(r {}, u {}, {:?}): no Room {}",
                                              ctxt.rid, ctxt.uid, &op, ctxt.rid));
                },
                Some(r) => r,
            };
            
            if ouid == ctxt.uid {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Info(format!("You are already allowed in {}.", cur_r.get_name())));
                return Ok(vec![env]);
            };
            
            let ou = match ctxt.umap.get_mut(&ouid) {
                None => { return Err(format!("do_op(r {}, u {}, {:?}): no target User {}",
                                              ctxt.rid, ctxt.uid, &op, ouid));
                },
                Some(u) => u,
            };
            
            if cur_r.is_invited(&ouid) {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::Info(format!("{} has already been invited to {}.",
                                        ou.get_name(), cur_r.get_name())));
                return Ok(vec![env]);
            };
            cur_r.invite(ouid);
            
            let inviter_msg: Msg;
            let invitee_msg: Msg;
            if cur_r.get_users().contains(&ouid) {
                inviter_msg = Msg::Info(format!("{} may now return to {} even when closed.",
                                                ou.get_name(), cur_r.get_name()));
                invitee_msg = Msg::Info(format!("You have been invited to return to {} even if it closes.",
                                                cur_r.get_name()));
            } else {
                inviter_msg = Msg::Info(format!("You invite {} to join {}.", ou.get_name(), cur_r.get_name()));
                invitee_msg = Msg::Info(format!("You have been invited to join {}.", cur_r.get_name()));
            }
            ou.deliver_msg(&invitee_msg);
            let env = Env::new(
                Endpoint::Server,
                Endpoint::User(ctxt.uid),
                &inviter_msg);
            return Ok(vec![env]);
        },
        
        Op::Kick(ref uname) => {
            let collapsed = ascollapse(&uname);
            if collapsed.len() == 0 {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::info("That cannot be anyone's user name."));
                return Ok(vec![env]);
            }
            
            let ouid = match ctxt.ustr.get(&collapsed) {
                None => {
                    let env = Env::new(
                        Endpoint::Server,
                        Endpoint::User(ctxt.uid),
                        &Msg::Info(format!("No users matching the pattern \"{}\".", &collapsed)));
                    return Ok(vec![env]);
                },
                Some(n) => *n,
            };
            
            if ouid == ctxt.uid {
                let env = Env::new(
                    Endpoint::Server,
                    Endpoint::User(ctxt.uid),
                    &Msg::info("Bestowing the operator mantle on another and then leaving would be a more orderly transfer of power."
                    ));
                return Ok(vec![env]);
            }
            
            let ku = match ctxt.umap.get_mut(&ouid) {
                None => { return Err(format!("do_op(r {}, u {}, {:?}): no target User {}",
                                             ctxt.rid, ctxt.uid, &op, ouid));
                },
                Some(u) => u,
            };
            
            let in_room: bool;
            let mut cur_room_name = String::new();
            
            {
                let cur_r = match ctxt.rmap.get_mut(&ctxt.rid) {
                    None => { return Err(format!("do_op(r {}, u {}, {:?}): no Room {}",
                                                 ctxt.rid, ctxt.uid, &op, ctxt.rid));
                    },
                    Some(r) => r,
                };
            
                if cur_r.is_banned(&ouid) {
                    let env = Env::new(
                        Endpoint::Server,
                        Endpoint::User(ctxt.uid),
                        &Msg::Info(format!("{} is already banned from {}.",
                                           ku.get_name(), cur_r.get_name())));
                    return Ok(vec![env]);
                };
            
                cur_r.ban(ouid);
                in_room = cur_r.get_users().contains(&ouid);
                
                if !in_room {
                    /* This case is easy because we only have to message the
                    banner about his activity.
                    */
                    if !cur_r.get_users().contains(&ouid) {
                        let env = Env::new(
                            Endpoint::Server,
                            Endpoint::User(ctxt.uid),
                            &Msg::Info(format!("You have banned {} from {}.", ku.get_name(), cur_r.get_name())));
                        return Ok(vec![env]);
                        /* If the kickee is _not_ in the room, this function
                        returns now. All the rest of the Op::Kick match arm,
                        even after the closing of the conditionals and
                        reference-dropping scopes, involves the case where
                        the kickee _is_ in the current room.
                        */
                    }
                } else {
                    /* This case is tougher because it involves
                      * messaging the kicked user
                      * moving the kicked user
                      * messaging the room
                      * messaging the Lobby that he's joined it
                    
                    It requires careful dancing around &mut lifetimes.
                    */
                    
                    let to_kicked = Msg::Info(format!("You have been kicked from {}.", cur_r.get_name()));
                    ku.deliver_msg(&to_kicked);                    
                    cur_r.leave(ouid);
            
                    cur_room_name = cur_r.get_name().to_string();
                }
                
                /* Mutable reference to ctxt.rmap contents (&mut cur_r) drops
                here, allowing us to get a mutable reference to the lobby to
                add the kicked user back in there.
                 */
            }
            
            // If the lobby doesn't exist, the server can go ahead and crash.
            let lobby = ctxt.rmap.get_mut(&0).unwrap();
            lobby.join(ouid);
            let to_lobby = Env::new(
                Endpoint::Server,
                Endpoint::Room(ctxt.rid),
                &Msg::Misc {
                    what: "join".to_string(),
                    data: vec![ku.get_name().to_string(), lobby.get_name().to_string()],
                    alt: format!("{} joins {}.", ku.get_name(), lobby.get_name()),
                });
            lobby.enqueue(to_lobby);
            
            let env = Env::new(
                Endpoint::Server,
                Endpoint::Room(ctxt.rid),
                &Msg::Misc {
                    what: "kick_other".to_string(),
                    alt: format!("{} has been kicked from {}.", ku.get_name(), &cur_room_name),
                    data: vec![ku.get_name().to_string(), cur_room_name],
                });
            
            return Ok(vec![env]);
        },
    }
}

/*
Each time through `greld`'s main loop, this is called on each of the `Room`s.
It iterates through each User in the room, reacting to any `Msg`s it
receives from them.

It also performs some various housekeeping, like displaying any messages
`.enqueue()`d by calls to this function on _other_ `Room`s, and bestowing
the op mantle on another user if the op leaves.
*/

fn process_room(
    rid: u64,
    current_time: Instant,
    user_map: &mut HashMap<u64, User>,
    ustr_map: &mut HashMap<String, u64>,
    room_map: &mut HashMap<u64, Room>,
    rstr_map: &mut HashMap<String, u64>,
    cfg: &ServerConfig
) -> Result<(), String> {
    let mut uid_list: Vec<u64>;
    {
        match room_map.get(&rid) {
            None  => { return Err(format!("Room {} doesn't exist.", &rid)); },
            Some(r) => {
                uid_list = vec![0; r.get_users().len()];
                uid_list.copy_from_slice(r.get_users());
            },
        }
    }
    
    let mut ctxt = Context {
        rid: rid,
        uid: 0,
        umap: user_map,
        ustr: ustr_map,
        rmap: room_map,
        rstr: rstr_map,
    };
    
    let mut envz: Vec<Env> = Vec::new();
    let mut logouts: Vec<u64> = Vec::new();
    
    for uid in &uid_list {
        let m: Msg;
        {
            let mu = match ctxt.umap.get_mut(uid) {
                None => {
                    debug!("process_room({}): user {} doesn't exist", &rid, uid);
                    continue;
                },
                Some(x) => x,
            };
            
            let over_quota = mu.get_byte_quota() > cfg.byte_limit;
            mu.drain_byte_quota(cfg.byte_tick);
            if over_quota && mu.get_byte_quota() <= cfg.byte_limit {
                let msg = Msg::err("You may send messages again.");
                mu.deliver_msg(&msg);
            }
            
            match mu.try_get() {
                None => {
                    let last = mu.get_last_data_time();
                    match current_time.checked_duration_since(last) {
                        Some(x) if x > cfg.blackout_time_to_kick => {
                            logouts.push(*uid);
                        },
                        Some(x) if x > cfg.blackout_time_to_ping => {
                            mu.deliver_msg(&Msg::Ping);
                        },
                        _ => {},
                    }
                    continue;
                },
                Some(msg) => {
                    if !over_quota {
                        m = msg;
                        if mu.get_byte_quota() > cfg.byte_limit {
                            let msg = Msg::err("You have exceeded your data quota and your messages will be ignored for a short time.");
                            mu.deliver_msg(&msg);
                        }
                    } else {
                        continue;
                    }
                }
            }
        }
        
        ctxt.uid = *uid;
        
        let pres = match m {

            Msg::Text { who: _, lines: l } => do_text(&mut ctxt, l),
            Msg::Priv { who, text }        => do_priv(&mut ctxt, who, text),
            Msg::Name(new_candidate)       => do_name(&mut ctxt, cfg, new_candidate),
            Msg::Join(room_name)           => do_join(&mut ctxt, cfg, room_name),
            Msg::Block(user_name)          => do_block(&mut ctxt, user_name),
            Msg::Unblock(user_name)        => do_unblock(&mut ctxt, user_name),
            Msg::Logout(salutation)        => do_logout(&mut ctxt, salutation),
            Msg::Query{ what, arg }        => do_query(&mut ctxt, what, arg),
            Msg::Op(op)                    => do_op(&mut ctxt, op),
            _ => { /* Other patterns require no response. */ Ok(vec![]) },
        };
        
        match pres {
            Err(e) => {
                #[cfg(debug_assertions)]
                trace!("{}", &e);
                #[cfg(not(debug_assertions))]
                warn!("{}", &e);
            },
            Ok(mut v) => { for env in v.drain(..) { envz.push(env); } },
        }
    }
    
    for uid in logouts.drain(..) {
        if let Some(mu) = ctxt.umap.get_mut(&uid) {
            let msg = Msg::logout("Too long since the server received data from the client.");
            mu.deliver_msg(&msg);
            let env = Env::new(
                Endpoint::Server,
                Endpoint::Room(ctxt.rid),
                &Msg::Misc {
                    what: "leave".to_string(),
                    data: vec![mu.get_name().to_string(),
                               "[ disconnected by server ]".to_string()],
                    alt: format!("{} has been disconnected from the server.", mu.get_name()),
                });
            envz.push(env);
        } else {
            warn!("process_room({} ...): logouts.drain(): no User {}", ctxt.rid, uid);
        }
    }
    
    // Change room operator if current op is no longer in room.
    // (But obviously not for the lobby.)
    
    if rid != 0 {
        let mr = ctxt.rmap.get_mut(&rid).unwrap();
        let op_id = mr.get_op();
        let op_still_here = mr.get_users().contains(&op_id);
        if !op_still_here {
            if let Some(pnid) = mr.get_users().get(0) {
                if let Some(u) = ctxt.umap.get(pnid) {
                    let nid = *pnid;
                    mr.set_op(nid);
                    let env = Env::new(Endpoint::Server, Endpoint::Room(rid),
                        &Msg::Info(format!("{} is now the Room operator.", u.get_name())));
                    envz.push(env);
                }
            }
        }
    }
    
    {
        let r = ctxt.rmap.get_mut(&rid).unwrap();
        r.deliver_inbox(ctxt.umap);
        for env in &envz {
            r.deliver(env, ctxt.umap);
        }
        uid_list.clear();
        uid_list.extend_from_slice(r.get_users());
        for uid in uid_list.iter_mut() {
            if let Some(mu) = user_map.get_mut(uid) {
                mu.nudge();
            }
        }
    }
    
    return Ok(());
}

/** When a user joins with a name that `ascollapse()`s to a user who is
already joined, this generates them a generic (but unique) name.
*/
fn gen_name(init_count: u64, map: &HashMap<String, u64>) -> String {
    let mut n = init_count;
    loop {
        let new_name = format!("user{}", n);
        if map.get(&new_name) == None {
            return new_name;
        }
        n = n + 1;
    }
}

/** Write pidfile. This may get formalized or configurizable eventually;
right now it just makes it easier to stop the server.
*/
fn write_pid() -> std::io::Result<()> {
    use std::fs::File;
    use std::io::Write;
    let pidstr = format!("{}", std::process::id());
    let mut pidf = File::create("d.pid")?;
    pidf.write_all(pidstr.as_bytes())?;
    return pidf.sync_all()
}

fn main() {
    if let Err(e) = write_pid() {
        println!("Error writing pidfile: {}", e);
    };
    
    let cfg: ServerConfig = ServerConfig::configure();
    println!("Configuration: {:?}", &cfg);
    WriteLogger::init(cfg.log_level, simplelog::Config::default(),
                      std::fs::File::create(&cfg.log_file).unwrap()).unwrap();
    let listen_addr = cfg.address.clone();
    
    let mut user_map: HashMap<u64, User> = HashMap::new();
    let mut ustr_map: HashMap<String, u64> = HashMap::new();
    let mut room_map: HashMap<u64, Room> = HashMap::new();
    let mut rstr_map: HashMap<String, u64> = HashMap::new();
    
    /* We set the lobby's uid to be 0, because no user will have a
       uid less than 100.
    */
    let mut lobby: Room = Room::new(0, cfg.lobby_name.clone(), 0);
    lobby.leave(0);
    room_map.insert(0, lobby);
    
    let (usender, urecvr) = mpsc::channel::<User>();
    thread::spawn(move || { listen(listen_addr, usender); });
    
    let mut now: Instant;
    
    loop {
        now = Instant::now();
        let mut roomz: Vec<u64> = room_map.keys().map(|k| *k).collect();
        for rid in roomz.drain(..) {
            let rnum = room_map.len();
            match process_room(rid, now, &mut user_map, &mut ustr_map,
                               &mut room_map, &mut rstr_map, &cfg) {
                Ok(()) => {},
                Err(e) => { warn!("process_room({}, ...) returned error: {}", rid, &e); },
            }
            if rnum != room_map.len() {
                for (k, v) in rstr_map.iter() { debug!("{} => {}", k, v); }
                for (k, v) in room_map.iter() { debug!("{} => {}", k, v.get_idstr()); }
            }
            
            if rid != 0 {
                let mut remove: bool = false;
                if let Some(r) = room_map.get(&rid) {
                    if r.get_users().len() == 0 {
                        remove = true;
                        let _ = rstr_map.remove(r.get_idstr());
                    }
                }
                if remove {
                    let _ = room_map.remove(&rid);
                }
            }
            
        }
        
        match urecvr.try_recv() {
            Ok(mut u) => {
                debug!("Accepting user {}: {}", u.get_id(), u.get_name());
                u.deliver_msg(&Msg::info(&cfg.welcome));
                
                let mut rename: Option<String> = None;
                if u.get_idstr().len() == 0 {
                    rename = Some(String::from("Your name does not have enough whitespace characters."));
                } else if u.get_name().len() > cfg.max_user_name_length {
                    rename = Some(format!("Your name cannot be longer than {} bytes.", cfg.max_user_name_length));
                } else {
                    let maybe_same_name = ustr_map.get(u.get_idstr());
                    if let Some(user_n) = maybe_same_name {
                        rename = Some(format!("Name \"{}\" exists.", user_map.get(user_n).unwrap().get_name()));
                    }
                }
                
                if let Some(err_msg) = rename {
                    let new_name = gen_name(u.get_id(), &ustr_map);
                    let msg = Msg::Err(err_msg);
                    u.deliver_msg(&msg);
                    let msg = Msg::Misc {
                        what: "name".to_string(),
                        data: vec![String::from(u.get_name()),
                                   String::from(&new_name)],
                        alt: format!("You are now known as \"{}\".", &new_name),
                    };
                    u.set_name(&new_name);
                    u.deliver_msg(&msg);
                }

                let msg = Msg::Misc{
                    what: "join".to_string(),
                    data: vec![u.get_name().to_string(), cfg.lobby_name.clone()],
                    alt: format!("{} joins {}.", u.get_name(), cfg.lobby_name.clone()),
                };
                let env = Env::new(Endpoint::Server, Endpoint::Room(0), &msg);
                let lobby = room_map.get_mut(&0).unwrap();
                lobby.join(u.get_id());
                lobby.enqueue(env);
                ustr_map.insert(u.get_idstr().to_string(), u.get_id());
                user_map.insert(u.get_id(), u);
            },
            Err(_) => {},
        }
        
        let loop_time = Instant::now().duration_since(now);
        if loop_time < cfg.min_tick {
            thread::sleep(cfg.min_tick - loop_time);
        }
    }
}
