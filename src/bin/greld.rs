/*!
greld.rs

The `grel` daemon (server) process.

updated 2020-12-24
*/

use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Instant, Duration};
use log::{debug, warn};
use simplelog::WriteLogger;

use grel::protocol::*;
use grel::user::*;
use grel::room::Room;
use grel::sock::Sock;
use grel::config::ServerConfig;

// const DEBUG: bool = true;

static BLOCK_TIMEOUT: Duration = Duration::from_millis(5000);
static BYTES_LIMIT:    usize = 1024;
static BYTES_PER_TICK: usize = 6;

enum Action {
    Move { who: u64, from: u64, to: u64,},
    Rename { who: u64, new: String },
    Logout { who: u64, to_who: String, to_room: String },
    Address { who: u64 },
}

fn match_string<T>(s: &str, hash: &HashMap<String, T>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for k in hash.keys() {
        if k.starts_with(s) { v.push(String::from(k)); }
    }
    return v;
}

fn initial_negotiation(u: &mut User) -> Result<(), String> {
    match u.blocking_get(BLOCK_TIMEOUT) {
        Err(e) => {
            let err_str = format!("Error reading initial \"Name\" message: {}", e);
            u.logout(&err_str);
            return Err(err_str);
        },
        Ok(m) => match m {
            Msg::Name { who: _, new: new_name } => {
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

fn process_room(
    rid: u64,
    current_time: Instant,
    user_map: &mut HashMap<u64, User>,
    ustr_map: &mut HashMap<String, u64>,
    room_map: &mut HashMap<u64, Room>,
    rstr_map: &mut HashMap<String, u64>,
    cfg: &ServerConfig
) -> Result<(), String> {
    let mut uid_list: Vec<u64> = Vec::new();
    {
        match room_map.get(&rid) {
            None  => { return Err(format!("Room {} doesn't exist.", &rid)); },
            Some(r) => { uid_list.extend_from_slice(r.get_users()); },
        }
    }
    
    let mut envz: Vec<Env> = Vec::new();
    let mut acts: Vec<Action> = Vec::new();
    
    for uid in &uid_list {
        let u: &User;
        let m: Msg;
        {
            let mu = match user_map.get_mut(uid) {
                None => {
                    debug!("process_room({}): user {} doesn't exist", &rid, uid);
                    continue;
                },
                Some(x) => x,
            };
            mu.drain_byte_quota(BYTES_PER_TICK);
            match mu.try_get() {
                None => {
                    let last = mu.get_last_data_time();
                    match current_time.checked_duration_since(last) {
                        Some(x) if x > cfg.blackout_time_to_kick => {
                            let act = Action::Logout{
                                who: *uid,
                                to_who: String::from("Too long since the server received data from the client."),
                                to_room: String::from("[ disconnected by server ]"),
                            };
                            acts.push(act);
                        },
                        Some(x) if x > cfg.blackout_time_to_ping => {
                            mu.deliver_msg(&Msg::Ping);
                        },
                        _ => {},
                    }
                    continue;
                },
                Some(msg) => { m = msg; }
            }
            u = mu;
        }
        
        match m {
            Msg::Text { who: _, lines: l } => {
                let newm = Msg::Text { who: u.get_name().to_string(), lines: l };
                let env = Env::new(Endpoint::User(*uid), Endpoint::Room(rid), &newm);
                envz.push(env);
            },
            Msg::Name { who: _, new: new_candidate } => {
                let act = Action::Rename{ who: *uid, new: new_candidate };
                acts.push(act);
            },
            Msg::Join { who: _, what: room_name }=> {
                let collapsed = ascollapse(&room_name);
                debug!("process_room({}): Msg::Join: {} ({})", &rid, &room_name, &collapsed);
                if collapsed.len() == 0 {
                    let env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                        &Msg::err("That cannot be a room name."));
                    envz.push(env);
                    continue;
                } else if room_name.len() > cfg.max_room_name_length {
                    let env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                        &Msg::Err(format!("Room names cannot be longer than {} bytes.", cfg.max_room_name_length)));
                    envz.push(env);
                    continue;
                }
                let found: bool;
                match rstr_map.get(&collapsed) {
                    Some(n) => {
                        acts.push(Action::Move{ who: *uid, from: rid, to: *n });
                        found = true;
                    },
                    None => { found = false; },
                }
                if !found {
                    let new_id = first_free_id(room_map);
                    let new_room = Room::new(new_id, room_name.clone(), *uid);
                    rstr_map.insert(collapsed, new_id);
                    room_map.insert(new_id, new_room);
                    let env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                                        &Msg::Info(format!("You create room \"{}\".", &room_name)));
                    debug!("process_room({}): User {} ({}) creates room {} ({})",
                            &rid, u.get_id(), u.get_name(), &new_id, &room_name);
                    envz.push(env);
                    let act = Action::Move{ who: *uid, from: rid, to: new_id };
                    acts.push(act);
                }

            },
            Msg::Query { what: k, arg: v }=> {
                match k.as_str() {
                    "addr" => {
                        let act = Action::Address{ who: *uid };
                        acts.push(act);
                    },
                    "roster" => {
                        let mut names_list: Vec<String> = Vec::with_capacity(uid_list.len());
                        for uid2 in &uid_list {
                            match user_map.get(uid2) {
                                None => { warn!("process_room({}): Msg::Query( roster ): no user {}",
                                                 &rid, uid2); },
                                Some(u) => { names_list.push(String::from(u.get_name())); },
                            }
                        }
                        let msg = Msg::List {
                            what: String::from("roster"),
                            items: names_list,
                        };
                        let env = Env::new(Endpoint::Server, Endpoint::User(*uid), &msg);
                        envz.push(env);
                    },
                    "who" => {
                        let match_name = ascollapse(&v);
                        let matches = match_string(&match_name, ustr_map);
                        let env: Env;
                        if matches.len() == 0 {
                            env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                                &Msg::Info(format!("No users matching the pattern \"{}\".", &match_name)));
                        } else {
                            env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                                &Msg::List{ what: "who".to_string(), items: matches, });
                        }
                        envz.push(env);
                    },
                    _ => {
                        let env = Env::new(Endpoint::Server, Endpoint::User(*uid),
                            &Msg::Err(format!("Unknonw \"Query\" type: \"{}\".", k)));
                        envz.push(env);
                    },
                }
            },
            Msg::Logout(salutation) => {
                let act = Action::Logout{
                    who: *uid, 
                    to_who: String::from("You have logged out."),
                    to_room: salutation
                };
                acts.push(act);
            },
            _ => { /* not implemented */ },
        }
    }
    
    for act in &acts {
        match act {
            
            Action::Move{ who: w, from: _f, to: t } => {
                let mu = match user_map.get(&w) {
                    Some(u) => u,
                    None => {
                        warn!("process_room({}): Action::Move: user {} doesn't exist.", &rid, &w);
                        continue;
                    },
                };
                {
                    let targ_r = match room_map.get_mut(&t) {
                        Some(r) => r,
                        None => {
                            warn!("process_room({}): Action::Move: room {} doesn't exist.", &rid, &t);
                            continue;
                        },
                    };
                    targ_r.join(*w);
                    let join_msg = Msg::Join{
                        who: String::from(mu.get_name()),
                        what: String::from(targ_r.get_name()),
                    };
                    let join_env = Env::new(Endpoint::User(*w), Endpoint::Room(*t), &join_msg);
                    targ_r.enqueue(join_env);
                    debug!("process_room({}): Action::Move: target room {} user list: {:?}",
                           &rid, targ_r.get_id(), targ_r.get_users());
                }
                {
                    let cur_r = room_map.get_mut(&rid).unwrap();
                    let leave_msg = Msg::Leave{
                        who: String::from(mu.get_name()),
                        message: String::from("Moved to another room."),
                    };
                    let leave_env = Env::new(Endpoint::User(*w), Endpoint::Room(rid), &leave_msg);
                    envz.push(leave_env);
                    cur_r.leave(*w);
                    debug!("process_room({}): Action::Move: old room {} user list: {:?}",
                           &rid, cur_r.get_id(), cur_r.get_users());
                }
            },
            
            Action::Rename{ who: w, new: new_name } => {
                let new_id = ascollapse(&new_name);
                if new_id.len() == 0 {
                    let msg = Msg::err("Your name must have more non-whitespace characters.");
                    let env = Env::new(Endpoint::Server, Endpoint::User(*w), &msg);
                    envz.push(env);
                    continue;
                } else if new_name.len() > cfg.max_user_name_length {
                    let msg = Msg::Err(format!("Your name cannot be longer than {} bytes.", cfg.max_user_name_length));
                    let env = Env::new(Endpoint::Server, Endpoint::User(*w), &msg);
                    envz.push(env);
                    continue;
                }
                if let Some(ouid) = ustr_map.get(&new_id) {
                    let ou = match user_map.get(ouid) {
                        None => { continue; },
                        Some(u) => u,
                    };
                    let msg = Msg::Err(format!("There is already a user named \"{}\".", ou.get_name()));
                    let env = Env::new(Endpoint::Server, Endpoint::User(*w), &msg);
                    envz.push(env);
                    continue;
                }
                if let Some(mu) = user_map.get_mut(&w) {
                    let msg = Msg::Name{
                        who: mu.get_name().to_string(),
                        new: new_name.clone(),
                    };
                    let _ = ustr_map.remove(mu.get_idstr());
                    let env = Env::new(Endpoint::Server, Endpoint::Room(rid), &msg);
                    envz.push(env);
                    mu.set_name(&new_name);
                    ustr_map.insert(mu.get_idstr().to_string(), *w);
                } else {
                    warn!("process_room({}): Action::Rename: User {} doesn't exist.", &rid, w);
                }
            },
            
            Action::Logout{ who: w, to_who: twho, to_room: salutation } => {
                if let Some(mut mu) = user_map.remove(w) {
                    let _ = ustr_map.remove(mu.get_idstr());
                    mu.logout(&twho);
                    let msg = Msg::Leave{
                        who: mu.get_name().to_string(),
                        message: salutation.to_string(),
                    };
                    let env = Env::new(Endpoint::User(*w), Endpoint::Room(rid), &msg);
                    envz.push(env);
                } else {
                    warn!("process_room({}): Action::Logout: User {} doesn't exist.", &rid, &w);
                }
                let mr = room_map.get_mut(&rid).unwrap();
                mr.leave(*w);
            },
            
            Action::Address{ who: w } => {
                if let Some(mu) = user_map.get_mut(&w) {
                    let msg = Msg::List {
                        what: String::from("addr"),
                        items: match mu.get_addr() {
                            None => vec![String::from("???")],
                            Some(s) => vec![s],
                        }
                    };
                    let env = Env::new(Endpoint::Server, Endpoint::User(*w), &msg);
                    envz.push(env);
                } else {
                    warn!("process_room({}): Action::Address: User {} doesn't exist.", &rid, &w);
                }
            },
        }
    }
    
    {
        let r = room_map.get_mut(&rid).unwrap();
        r.deliver_inbox(user_map);
        for env in &envz {
            r.deliver(env, user_map);
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

fn main() {
    let cfg: ServerConfig = ServerConfig::configure();
    println!("Configuration: {:?}", &cfg);
    WriteLogger::init(cfg.log_level, simplelog::Config::default(),
                      std::fs::File::create(&cfg.log_file).unwrap()).unwrap();
    let listen_addr = cfg.address.clone();
    
    let mut user_map: HashMap<u64, User> = HashMap::new();
    let mut ustr_map: HashMap<String, u64> = HashMap::new();
    let mut room_map: HashMap<u64, Room> = HashMap::new();
    let mut rstr_map: HashMap<String, u64> = HashMap::new();
    
    let mut lobby: Room = Room::new(0, String::from(""), 0);
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
                    let msg = Msg::Name {
                        who: String::from(u.get_name()),
                        new: String::from(&new_name),
                    };
                    u.set_name(&new_name);
                    u.deliver_msg(&msg);
                }

                let msg = Msg::Join{ who: u.get_name().to_string(),
                                     what: String::new(),
                                    };
                let env = Env::new(Endpoint::Server, Endpoint::Room(0), &msg);
                let mut lobby = room_map.get_mut(&0).unwrap();
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
