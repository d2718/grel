/*!
grel.rs

The `grel` terminal client.

updated 2020-12-30
*/

use lazy_static::lazy_static;
use log::{error, debug, trace};
use std::io::stdout;
use std::net::TcpStream;
use std::time::{Instant};

use termion::input::TermRead;
use termion::event::{Event, Key};
use termion::raw::IntoRawMode;

use grel::proto2::Msg;
use grel::sock::Sock;
use grel::config::ClientConfig;
use grel::line::{Line, Style};
use grel::screen::Screen;

lazy_static!{
    static ref PING: Vec<u8> = Msg::Ping.bytes();
    static ref ROSTER_REQUEST: Vec<u8> =
        Msg::Query{
            what: String::from("roster"),
            arg: String::new(),
        }.bytes();
}

const SPACE:    char = ' ';
const RETURN:   char = '\n';

/** Represents the vaguely vi-like mode the client is in. */
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Command,
    Input,
}

/** Global variable struct. */
struct Globals {
    uname: String,
    rname: String,
    mode: Mode,
    messages: Vec<String>,
    local_addr: String,
    server_addr: String,
    socket: Sock,
    cmd: char,
}

/** Read command line options and configuration file. */
fn configure() -> ClientConfig {
    let opts = clap::App::new("grel")
        .max_term_width(80)
        .version("0.1")
        .author("Dan Hill <daniel.s.hill@gmail.com>")
        .about("grel chat terminal client")
        .arg(clap::Arg::with_name("config")
            .short("c").long("config")
            .value_name("FILE")
            .help("use an alternate configuration file")
            .takes_value(true),
        )
        .arg(clap::Arg::with_name("name")
            .short("n").long("name")
            .value_name("NAME")
            .help("set user name")
            .takes_value(true),
        )
        .arg(clap::Arg::with_name("address")
            .short("a").long("address")
            .value_name("IP:PORT")
            .help("attempt to connect to server at IP:PORT")
            .takes_value(true),
        ).arg(clap::Arg::with_name("write")
            .short("g").long("generate-default")
            .help("generate a new default configuration file")
            .takes_value(false),
        )
        .get_matches();
    
    if opts.is_present("write") {
        match ClientConfig::generate() {
            Ok(dir) => {
                println!("Default configuration file written to {}", &dir);
                std::process::exit(0);
            },
            Err(e) => {
                println!("{}", e);
                std::process::exit(2);
            },
        }
    }
    
    let mut cfg = match ClientConfig::configure(opts.value_of("config")) {
        Ok(x) => x,
        Err(e) => {
            println!("Configuration error: {}", e);
            std::process::exit(1);
        },
    };
    
    if let Some(n) = opts.value_of("name") { cfg.name = String::from(n); }
    if let Some(a) = opts.value_of("address") { cfg.address = String::from(a); }
    
    return cfg;
}

/** Attempt to connect to the `greld` server specified either on the
command line or in the config file.
*/
fn connect(cfg: &ClientConfig) -> Result<Sock, String> {
    let mut thesock: Sock = match TcpStream::connect(&cfg.address) {
        Err(e) => { return Err(format!("Error connecting to {}: {}", cfg.address, e)); },
        Ok(s) => match Sock::new(s) {
            Err(e) => { return Err(format!("Error setting up socket: {}", e)); },
            Ok(sck) => sck,
        },
    };
    let b = Msg::Name(cfg.name.clone()).bytes();
    let res = thesock.blocking_send(&b, cfg.tick);
    match res {
        Err(e) => match thesock.shutdown() {
            Err(ee) => { return Err(format!("Error in initial protocol: {}; error during shutdown: {}", e, ee)); },
            Ok(()) =>  { return Err(format!("Error in initial protocol: {}", e)); },
        },
        Ok(()) => {},
    }
    
    return Ok(thesock);
}

/** This function splits a "command line" (one starting with the command
character) into the command token and the rest of the line.

It is written this way to avoid pulling in the whole regex crate.
*/
//~ fn parse_command_line(ipt: &[char]) -> (String, String) {
    //~ let mut cmd = String::new();
    //~ let mut arg = String::new();
    
    //~ let mut ipt_iter = ipt.iter();
    //~ let _ = ipt_iter.next();        // discard command char
    
    //~ while let Some(c) = ipt_iter.next() {
        //~ if c.is_whitespace() {
            //~ break;
        //~ } else {
            //~ for x in c.to_lowercase() { cmd.push(x); }
        //~ }
    //~ }
    
    //~ while let Some(c) = ipt_iter.next() {
        //~ if c.is_whitespace() {
            //~ // skip it
        //~ } else {
            //~ arg.push(*c);
            //~ break;
        //~ }
    //~ }
    
    //~ while let Some(c) = ipt_iter.next() { arg.push(*c); }
    
    //~ return (cmd, arg);
//~ }

/** Divide &str s into alternating chunks of whitespace and non-whitespace. */
fn tokenize_the_whitespace_too<'a>(s: &'a str) -> Vec<&'a str> {
    let mut v: Vec<&str> = Vec::new();
    
    let mut change: usize = 0;
    let mut s_iter = s.chars();
    let mut in_ws = match s_iter.next() {
        None => { return v; },
        Some(c) => c.is_whitespace(),
    };
    
    let s_iter = s.char_indices();
    for (i, c) in s_iter {
        if in_ws {
            if !c.is_whitespace() {
                v.push(&s[change..i]);
                change = i;
                in_ws = false;
            }
        } else {
            if c.is_whitespace() {
                v.push(&s[change..i]);
                change = i;
                in_ws = true;
            }
        }
    }
    v.push(&s[change..(s.len())]);
    
    return v;
}

/** In input mode, when the user hits return, this processes processes the
content of the input line and decides what to do.
*/
fn respond_to_user_input(ipt: Vec<char>, scrn: &mut Screen, gv: &mut Globals) {
    if let Some(c) = ipt.first() {
        if *c == gv.cmd {
            
            /* Collect the ipt vector as a string, discarding the cmd_char and
            translating newlines to spaces. */
            let cmd_line: String = ipt[1..].into_iter()
                .map(|c| if *c == RETURN { SPACE } else { *c }).collect();
                
            /* Tokenize the resulting string. */
            let cmd_toks = tokenize_the_whitespace_too(&cmd_line);
            
            /* Pre-calculate an upper-bound on the "arg" portion of the
            command, so multiple allocations need not be made during assembly. */
            let tot_len = cmd_toks.iter().fold(0usize, |sum, v| sum + v.len());
            let mut arg = String::with_capacity(tot_len);
            
            let cmd = cmd_toks[0].to_lowercase();  
            
            match cmd.as_str() {
                
                "quit" => {
                    if cmd_toks.len() > 2 { for s in &cmd_toks[2..] { arg.push_str(s); } }
                    let b = Msg::Logout(arg).bytes();
                    gv.socket.enqueue(&b);
                },
                
                "priv" => {
                    if cmd_toks.len() < 3 {
                        let mut sl = Line::new();
                        sl.pushf("# You must specify a recipient for a private message.",
                                 scrn.bfg(), scrn.bbg(), Style::None);
                        scrn.push_line(sl);
                    } else {
                        let targ = cmd_toks[2].to_string();
                        if cmd_toks.len() > 4 { for s in &cmd_toks[4..] { arg.push_str(s); } }
                        let b = Msg::Priv {
                            who: targ,
                            text: arg,
                        }.bytes();
                        gv.socket.enqueue(&b);
                    }
                },
                
                "name" => {
                    if cmd_toks.len() > 2 { for s in &cmd_toks[2..] { arg.push_str(s); } }
                    let b = Msg::Name(arg).bytes();
                    gv.socket.enqueue(&b);
                },
                
                "join" => {
                    if cmd_toks.len() > 2 { for s in &cmd_toks[2..] { arg.push_str(s); } }
                    let b = Msg::Join(arg).bytes();
                    gv.socket.enqueue(&b);
                },
                
                x @ _ => {
                    let mut sl = Line::new();
                    sl.pushf("# Unknown command ", scrn.bfg(), scrn.bbg(), Style::None);
                    sl.pushf(x, scrn.bfg(), scrn.bbg(), Style::Bold);
                    scrn.push_line(sl);
                },
            }
            return;
        }
    }
    
    let mut lines: Vec<String> = Vec::new();
    let mut cur_line = String::new();
    for c in ipt.into_iter() {
        if c == '\n' {
            lines.push(cur_line);
            cur_line = String::new();
        } else {
            cur_line.push(c);
        }
    }
    lines.push(cur_line);
    let b = Msg::Text {
        who: String::new(),
        lines: lines,
    }.bytes();
    gv.socket.enqueue(&b);
}

/** Respond to key presses when in "command" mode.

Returns the mode the client should be in after processing this event.
*/
fn process_command(evt: Event, scrn: &mut Screen, gv: &mut Globals) -> Mode {
    trace!("process_command(...): rec'd: {:?}", &evt);
    match evt {
        Event::Mouse(_) => {
            debug!("Mouse events aren't supported.");
            return Mode::Command;
        },
        Event::Key(k) => match k {
            Key::Char(SPACE) | Key::Char(RETURN) => {
                return Mode::Input;
            },
            Key::Up   => { scrn.scroll_lines(1); },
            Key::Down => { scrn.scroll_lines(-1); },
            Key::PageUp => {
                let jump = (scrn.get_main_height() as i16) - 1;
                scrn.scroll_lines(jump);
            },
            Key::PageDown => {
                let jump = 1 - (scrn.get_main_height() as i16);
                scrn.scroll_lines(jump);
            },
            Key::Char('q') => {
                let m = Msg::logout("quit...");
                gv.socket.enqueue(&m.bytes());
            },
            e @ _ => { debug!("process_command(...): {:?} ignored", e); },
        },
        e @ _ => { debug!("process_command(...): {:?} ignored", e); },
    }
    
    return Mode::Command;
}

/** Respond to key presses when in "input" mode. Mostly this involves
adding characters to the input line or moving the insertion point on
the input line.

Returns the mode the client should be in after processing this event.
*/
fn process_input(evt: Event, scrn: &mut Screen, gv: &mut Globals) -> Mode {
    match evt {
        Event::Key(k) => match k {
            Key::Char(RETURN) => {
                let cv = scrn.pop_input();
                respond_to_user_input(cv, scrn, gv);
            },
            Key::Backspace => {
                if scrn.get_input_length() == 0 {
                    return Mode::Command;
                } else {
                    scrn.input_backspace();
                }
            },
            Key::Delete =>    { scrn.input_delete(); },
            Key::Left =>      { scrn.input_skip_chars(-1); },
            Key::Right =>     { scrn.input_skip_chars(1); },
            Key::Esc | Key::Alt('\u{1b}')=> { return Mode::Command; },
            Key::Char(c) =>   { scrn.input_char(c); },
            e @ _ => { debug!("process_insert(...): {:?} ignored", e); },
        },
        Event::Mouse(_) => {
            debug!("Mouse events aren't supported.");
        },
        e @ _ => { debug!("process_insert(...): {:?} ignored", e); },
    }
    
    return Mode::Input;
}

/** When the Sock coughs up a Msg, this function decides what to do with it.

Returns true if the program should quit.
*/
fn process_msg(m: Msg,
               scrn: &mut Screen,
               gv: &mut Globals)
-> Result<bool, String> {
    debug!("process_msg(...): rec'd: {:?}", &m);
    match m {
        Msg::Ping => { gv.socket.enqueue(&PING); },
        
        Msg::Text { who, lines } => {
            for lin in &lines {
                let mut sl = Line::new();
                sl.pushf(&who, scrn.hfg(), scrn.hbg(), Style::None);
                sl.push(": ");
                sl.push(lin);
                scrn.push_line(sl);
            }
        },
        
        Msg::Priv { who, text } => {
            let mut sl = Line::new();
            sl.push("$ ");
            sl.pushf(&who, scrn.bfg(), scrn.bbg(), Style::None);
            sl.push(": ");
            sl.push(&text);
            scrn.push_line(sl);
        },
        
        Msg::Logout(s) => {
            gv.messages.push(s);
            return Ok(true);
        },
        
        Msg::Info(s) => {
            let mut sl = Line::new();
            sl.pushf("* ", None, None, Style::None);
            sl.pushf(&s, None, None, Style::None);
            scrn.push_line(sl);
        },

        Msg::Err(s) => {
            let mut sl = Line::new();
            sl.pushf("# ", scrn.bfg(), scrn.bbg(), Style::None);
            sl.pushf(&s, scrn.bfg(), scrn.bbg(), Style::None);
            scrn.push_line(sl);
        },
        
        Msg::Misc { ref what, ref data, ref alt } => match what.as_str() {
            "join" => {
                let name = match data.get(0) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let mut sl = Line::new();
                sl.push("* ");
                if name.as_str() == gv.uname.as_str() {
                    sl.pushf("You", None, None, Style::Bold);
                    sl.push(" join ");
                    if let Some(room) = data.get(1) {
                        gv.rname = room.clone();
                        let mut room_line = Line::new();
                        room_line.pushf(&gv.rname, scrn.hfg(), scrn.hbg(), Style::None);
                        scrn.set_stat_ur(room_line);
                    }
                } else {
                    sl.pushf(name, scrn.hfg(), scrn.hbg(), Style::None);
                    sl.push(" joins ");
                }
                if let Some(room) = data.get(1) {
                    sl.pushf(room, scrn.hfg(), scrn.hbg(), Style::None);
                } else {
                    sl.push("the server");
                }
                sl.push(".");
                gv.socket.enqueue(&ROSTER_REQUEST);
                scrn.push_line(sl);
            },
            
            "leave" => {
                let name = match data.get(0) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let message = match data.get(1) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let mut sl = Line::new();
                sl.push("* ");
                sl.pushf(name, scrn.hfg(), scrn.hbg(), Style::None);
                sl.push(" leaves: ");
                sl.push(message);
                gv.socket.enqueue(&ROSTER_REQUEST);
                scrn.push_line(sl);
            },
            
            "priv_echo" => {
                let name = match data.get(0) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let text = match data.get(1) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let mut sl = Line::new();
                sl.push("$ ");
                sl.pushf("You", scrn.bfg(), scrn.bbg(), Style::Bold);
                sl.pushf(" @ ", scrn.bfg(), scrn.bbg(), Style::None);
                sl.pushf(&name, scrn.hfg(), scrn.hbg(), Style::None);
                sl.push(": ");
                sl.push(&text);
                scrn.push_line(sl);
            },
            
            "name" => {
                let old = match data.get(0) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                let new = match data.get(1) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(x) => x,
                };
                
                let mut sl = Line::new();
                sl.push("* ");
                if old.as_str() == gv.uname.as_str() {
                    sl.pushf("You", None, None, Style::Bold);
                    sl.push(" are now known as ");
                    gv.uname = new.clone();
                    write_mode_line(scrn, gv);
                } else {
                    sl.pushf(old, scrn.hfg(), scrn.hbg(), Style::None);
                    sl.push(" is now known as ");
                }
                sl.pushf(new, scrn.hfg(), scrn.hbg(), Style::None);
                sl.push(".");
                scrn.push_line(sl);
                gv.socket.enqueue(&ROSTER_REQUEST);
            },
            
            "roster" => {
                if data.len() < 1 { return Err(format!("Incomplete data: {:?}", &m)); }
                scrn.set_roster(data);
            },
            
            "addr" => {
                match data.get(0) {
                    None => { return Err(format!("Incomplete data: {:?}", &m)); },
                    Some(addr) => {
                        gv.local_addr = addr.clone();
                        write_mode_line(scrn, gv);
                    },
                }
            },
            
            _ => {
                let mut sl = Line::new();
                sl.push("* ");
                sl.push(alt);
                scrn.push_line(sl)
            },
        },

        msg @ _ => {
            let msgs = format!("{:?}", msg);
            let s: String = msgs.chars().map(|c| {
                match c {
                    RETURN => SPACE,
                    x @ _ => x,
                }
            }).collect();
            let mut sl = Line::new();
            sl.push("# Unsupported Msg: ");
            sl.push(&s);
            scrn.push_line(sl);
        },
    }
    return Ok(false);
}

/** When the mode line (in the lower-left-hand corner) should change,
this updates it.
*/
fn write_mode_line(scrn: &mut Screen, gv: &Globals) {
    let none = grel::line::Style::None;
    let mut mode_line = Line::new();
    let mch: &str = match gv.mode {
        Mode::Command => "Com",
        Mode::Input => "Ipt",
    };
    mode_line.pushf(mch, scrn.hfg(), scrn.hbg(), none);
    mode_line.pushf(" | ", scrn.bfg(), scrn.bbg(), none);
    mode_line.pushf(&(gv.uname), scrn.hfg(), scrn.hbg(), none);
    mode_line.push(" @ ");
    mode_line.pushf(&(gv.local_addr), scrn.hfg(), scrn.hbg(), none);
    scrn.set_stat_ll(mode_line);
}

fn main() {
    let cfg: ClientConfig = configure();
    
    simplelog::WriteLogger::init(simplelog::LevelFilter::Trace,
                                 simplelog::Config::default(),
                                 std::fs::File::create("grel.log").unwrap())
        .unwrap();
        
    debug!("{:?}", &cfg);
    println!("Attempting to connect to {}...", &cfg.address);
    let mut sck: Sock = match connect(&cfg) {
        Err(e) => {
            println!("{}", e);
            std::process::exit(2);
        },
        Ok(x) => x,
    };
    sck.set_read_buffer_size(cfg.read_size);
    println!("...success. Negotiating initial protocol...");
    
    {
        let b = Msg::Query{
            what: String::from("addr"),
            arg: String::new(),
        }.bytes();
        sck.enqueue(&b);
    }
    println!("...success. Initializing terminal.");
    
    let mut gv: Globals = Globals {
        uname: cfg.name.clone(),
        rname: String::from("Lobby"),
        mode: Mode::Input,
        local_addr: String::new(),
        messages: Vec::new(),
        server_addr: sck.get_addr().unwrap(),
        socket: sck,
        cmd: cfg.cmd_char,
    };
    
    {
        let mut term = stdout().into_raw_mode().unwrap();
        let mut scrn: Screen = Screen::new(&mut term, cfg.roster_width);
        let mut evt_iter = termion::async_stdin().events();
        let mut addr_line = Line::new();
        addr_line.pushf(&gv.server_addr, scrn.hfg(), scrn.hbg(), Style::None);
        scrn.set_stat_ul(addr_line);
        let mut room_line = Line::new();
        room_line.pushf(&gv.rname, scrn.hfg(), scrn.hbg(), Style::None);
        scrn.set_stat_ur(room_line);
        write_mode_line(&mut scrn, &gv);
        
        /* The 'main_loop repeats until the program should end, generally
        after disconnection.
        */
        'main_loop: loop {
            let loop_start = Instant::now();
            
            /* Read any input that has piled up since the last iteration
            of `main_loop. */
            while let Some(r) = evt_iter.next() {
                match r {
                    Err(e) => {
                        gv.messages.push(format!("{}", e));
                        break 'main_loop;
                    },
                    Ok(e) => {
                        trace!("read loop: .next() -> {:?}", &e);
                        let cur_mode = gv.mode;
                        let new_mode = match cur_mode {
                            Mode::Command => process_command(e, &mut scrn, &mut gv),
                            Mode::Input => process_input(e, &mut scrn, &mut gv),
                        };
                        if new_mode != cur_mode {
                            gv.mode = new_mode;
                            write_mode_line(&mut scrn, &gv);
                        }
                            
                        trace!("main loop: mode: {:?}", &gv.mode);
                        scrn.refresh(&mut term);
                    },
                }
            }
            
            /* Attempt to push any data in the `Sock`'s outgoing buffer to
            the server. */
            let outgoing_bytes = gv.socket.send_buff_size();
            match gv.socket.blow() {
                Err(e) => {
                    gv.messages.push(format!("{}", e));
                    break 'main_loop;
                },
                Ok(n) => {
                    let sent = outgoing_bytes - n;
                    if sent > 0 { debug!("Sock::blow() wrote {} bytes.", sent); }
                },
            }
            
            /* Try to suck from the byte stream incoming from the server.
            
            If there's anything there, attempt to decode `Msg`s from the
            `Sock` and process them until there's nothing left. */
            let suck_res = gv.socket.suck();
            match suck_res {
                Err(e) => {
                    gv.messages.push(format!("{}", e));
                    break 'main_loop;
                },
                Ok(0) => { /* no bytes, we're done */ },
                Ok(n) => {
                    debug!("Sock::suck() huffed {} bytes.", n);
                    'msg_loop: loop {
                        let get_res = gv.socket.try_get();
                        match get_res {
                            Err(e) => {
                                gv.messages.push(format!("{}", e));
                                break 'main_loop;
                            },
                            Ok(None) => { break 'msg_loop; },
                            Ok(Some(msg)) => {
                                match process_msg(msg, &mut scrn, &mut gv) {
                                    Ok(true) => { break 'main_loop; },
                                    Ok(false) => {},
                                    Err(e) => {
                                        error!("process_msg(...) returned error: {}", e);
                                    },
                                };
                            },
                        }
                    }
                },
            }
            
            /* If the scrollback buffer has grown too large, prune it down. */
            if scrn.get_scrollback_length() > cfg.max_scrollback {
                scrn.prune_scrollback(cfg.min_scrollback);
            }
            
            /* Check for terminal resize every iteration; if the size hasn't
            changed, `Screen::auto_resize()` doesn't do anything else. */
            scrn.auto_resize();
            
            /* If there are any changes to the state of the screen (I think
            everything but the receipt/sending of a `Msg::Ping` does this),
            redraw the areas that changed. */
            scrn.refresh(&mut term);
            
            /* If less than the configured tick time has elapsed, sleep for
            the rest of the tick. This will probably happen unless there's a
            gigantic amount of incoming data. */
            let loop_time = Instant::now().duration_since(loop_start);
            if loop_time < cfg.tick {
                std::thread::sleep(cfg.tick - loop_time);
            }
        }
    }
    
    for m in &gv.messages {
        println!("{}", &m);
    }
    
}
