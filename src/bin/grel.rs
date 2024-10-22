/*!
grel.rs

The `grel` terminal client.

updated 2021-02-01
*/

use lazy_static::lazy_static;
use log::{error, debug, trace};
use std::io::stdout;
use std::net::TcpStream;
use std::time::{Instant};

//~ use termion::input::TermRead;
//~ use termion::event::{Event, Key};
//~ use termion::raw::IntoRawMode;

use crossterm::{event, event::Event, event::KeyCode };

//~ use grel::proto2::{Msg, Op};
use grel::proto3::{Sndr, Rcvr, SndOp};
use grel::sock::Sock;
use grel::config::ClientConfig;
use grel::line::Line;
use grel::screen::Screen;

const JIFFY: std::time::Duration = std::time::Duration::from_millis(0);

lazy_static!{
    static ref PING: Vec<u8> = Sndr::Ping.bytes();
    static ref ROSTER_REQUEST: Vec<u8> =
        Sndr::Query{ what: "roster", arg: "", }.bytes();
}

const SPACE:    char = ' ';
const RETURN:   char = '\n';
const OP_ERROR: &str = "# The recognized OP subcommands are OPEN, CLOSE, KICK, INVITE, and GIVE.";

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
    run: bool,
}

impl Globals {
    pub fn enqueue(&mut self, m: &Sndr) {
        let b = m.bytes();
        self.socket.enqueue(&b);
    }
    
    pub fn enqueue_bytes(&mut self, b: &[u8]) {
        self.socket.enqueue(b);
    }
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
    let b = Sndr::Name(&cfg.name).bytes();
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

/** Split a vector of alternating whitespance-and-non tokens (as returned
by `tokenize_the_whitespace_too(...)` (above) into a vector of `n_cmds`
"command" words and an "arg" `String` made of the non-command tokens
concatenated.
*/
fn split_command_toks<'a>(toks: &'a [&str], n_cmds: usize)
-> Result<(Vec<&'a str>, String), ()> {
    if n_cmds == 0 { return Err(()); }
    if toks.len() < (2* n_cmds) - 1 { return Err(()); }
    
    let mut cmds: Vec<&'a str> = Vec::new();
    let mut arg: String = String::new();
    
    let mut n: usize = 0;
    for _ in 0..n_cmds {
        cmds.push(toks[n]);
        n = n + 2;
    }
    while n < toks.len() {
        arg.push_str(toks[n]);
        n = n + 1;
    }
    
    return Ok((cmds, arg));
}

/** In input mode, when the user hits return, this processes processes the
content of the input line and decides what to do.
*/
fn respond_to_user_input(ipt: Vec<char>, scrn: &mut Screen, gv: &mut Globals) {
    if let Some(c) = ipt.first() {
        if *c == gv.cmd {
            
            /* If the only thing in the input line is a single semicolon,
            the rest of this tokenizing stuff will panic, so bail here. */
            if ipt.len() == 1 { return; }
            
            /* Collect the ipt vector as a string, discarding the cmd_char and
            translating newlines to spaces. */
            let cmd_line: String = ipt[1..].into_iter()
                .map(|c| if *c == RETURN { SPACE } else { *c }).collect();
                
            /* Tokenize the resulting string. */
            let cmd_toks = tokenize_the_whitespace_too(&cmd_line);
            let cmd = cmd_toks[0].to_lowercase();  
            
            match cmd.as_str() {
                "quit" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => { gv.enqueue(&Sndr::Logout(&arg)); },
                        Err(_) => { return; },
                    }
                },
                
                "priv" => {
                    match split_command_toks(&cmd_toks, 2) {
                        Ok((cmds, arg)) => {
                            gv.enqueue(&Sndr::Priv {
                                who: cmds[1],
                                text: &arg,
                            });
                        },
                        Err(_) => {
                            let mut sl = Line::new();
                            sl.pushf("# You must specify a recipient for a private message.", &scrn.styles().dim);
                            scrn.push_line(sl);
                        },
                    }
                },
                
                "name" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => { gv.enqueue(&Sndr::Name(&arg)); },
                        Err(_) => { return; },
                    }
                },
                
                "join" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => { gv.enqueue(&Sndr::Join(&arg)); },
                        Err(_) => { return; },
                    }
                },
                
                "who" | "rooms" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => {
                            gv.enqueue(&Sndr::Query{
                                what: &cmd,
                                arg: &arg,
                            });
                        },
                        Err(_) => { return; }
                    }
                },
                
                "block" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => { gv.enqueue(&Sndr::Block(&arg)); },
                        Err(_) => { return; },
                    }
                },
                
                "unblock" => {
                    match split_command_toks(&cmd_toks, 1) {
                        Ok((_, arg)) => { gv.enqueue(&Sndr::Unblock(&arg)); },
                        Err(_) => { return; },
                    }
                },
                
                "op" => {
                    match split_command_toks(&cmd_toks, 2) {
                        Err(_) => {
                            let mut sl = Line::new();
                            sl.pushf(OP_ERROR, &scrn.styles().dim);
                            scrn.push_line(sl);
                        },
                        Ok((cmds, arg)) => {
                            let msg: Option<Sndr> = match cmds[1].to_lowercase().as_str() {
                                "open"   => Some(Sndr::Op(SndOp::Open)),
                                "close"  => Some(Sndr::Op(SndOp::Close)),
                                "ban" | "kick" => Some(Sndr::Op(SndOp::Kick(&arg))),
                                "invite" => Some(Sndr::Op(SndOp::Invite(&arg))),
                                "give"   => Some(Sndr::Op(SndOp::Give(&arg))),
                                _ => {
                                    let mut sl = Line::new();
                                    sl.pushf(OP_ERROR, &scrn.styles().dim);
                                    scrn.push_line(sl);
                                    None
                                }
                            };
                            if let Some(m) = msg { gv.enqueue(&m); }
                        },
                    }
                },
                
                x @ _ => {
                    let mut sl = Line::new();
                    sl.pushf("# Unknown command ", &scrn.styles().dim);
                    sl.pushf(x, &scrn.styles().dim_bold);
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
    let lineref: Vec<&str> = lines.iter().map(|x| x.as_str()).collect();
    gv.enqueue(&Sndr::Text {
        who: "",
        lines: &lineref,
    });
}

/** Respond to keypress events in _command_ mode. */
fn command_key(evt: event::KeyEvent, scrn: &mut Screen, gv: &mut Globals) {
    match evt.code {
        KeyCode::Char(SPACE) | KeyCode::Enter => {
            gv.mode = Mode::Input;
        },
        /* The values passed to the two scrolling functions are opposite
        because the roster scroll is counted from the top, but the main
        window scroll is counted from the bottom.
        */
        KeyCode::Up   => { 
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.scroll_roster(-1);
            } else {
                scrn.scroll_lines(1);
            }
        },
        KeyCode::Down => {
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.scroll_roster(1);
            } else {
                scrn.scroll_lines(-1);
            }
        },
        KeyCode::PageUp => {
            let jump = (scrn.get_main_height() as i16) - 1;
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.scroll_roster(-jump);
            } else {
                scrn.scroll_lines(jump);
            }
        },
        KeyCode::PageDown => {
            let jump = 1 - (scrn.get_main_height() as i16);
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.scroll_roster(-jump);
            } else {
                scrn.scroll_lines(jump);
            }
        },
        KeyCode::Char('q') => {
            if evt.modifiers.contains(event::KeyModifiers::CONTROL) {
                gv.messages.push("Force quit the client.".to_string());
                gv.run = false;
            } else {
                gv.enqueue(&Sndr::Logout("[ client quit  ]"));
            }
        },
        _ => { /* */ },
    }
}

/** Respond to keypress events in _input_ mode. */
fn input_key(evt: event::KeyEvent, scrn: &mut Screen, gv: &mut Globals) {
    match evt.code {
        KeyCode::Enter => {
            let cv = scrn.pop_input();
            respond_to_user_input(cv, scrn, gv);
        },
        KeyCode::Backspace => {
            if scrn.get_input_length() == 0 {
                gv.mode = Mode::Command;
            } else if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.input_backspace_word();
            } else {
                scrn.input_backspace();
            }
        },
        KeyCode::Delete => { 
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.input_delete_word();
            } else {
                scrn.input_delete();
            }
        },
        KeyCode::Left   => { 
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.input_skip_backword();
            } else {
                scrn.input_skip_chars(-1);
            }
        },
        KeyCode::Right  => {
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                scrn.input_skip_foreword();
            } else {
                scrn.input_skip_chars(1);
            }
        },
        KeyCode::Home => {
            let delta = scrn.get_input_length() as i16;
            scrn.input_skip_chars(-delta);
        },
        KeyCode::End => {
            let delta = scrn.get_input_length() as i16;
            scrn.input_skip_chars(delta);
        },
        KeyCode::Esc    => { gv.mode = Mode::Command; },
        KeyCode::Char('\u{1b}') => {
            if evt.modifiers.contains(event::KeyModifiers::ALT) {
                gv.mode = Mode::Command;
            }
        },
        KeyCode::Char(c) => { scrn.input_char(c); },
        _ => { /* */ }
    }
}

/** While the terminal polls that events are available, read them and
act accordingly.

Returns `true` if an event was read, so the calling code can know whether
to redraw (some portion of) the screen.
*/
fn process_user_typing(
    scrn: &mut Screen,
    gv: &mut Globals,
) -> crossterm::Result<bool> {
    let mut should_refresh: bool = false;
    
    while event::poll(JIFFY)? {
        let cur_mode = gv.mode;
        
        match event::read()? {
            Event::Key(evt) => {
                trace!("event: {:?}", evt);
                match gv.mode {
                    Mode::Command => command_key(evt, scrn, gv),
                    Mode::Input   => input_key(evt, scrn, gv),
                }
            },
            Event::Resize(w, h) => scrn.resize(w, h),
            Event::Mouse(evt) => debug!("Mouse events not supported: {:?}", evt),
        }
        
        if cur_mode != gv.mode { write_mode_line(scrn, gv); }
        should_refresh = true;
    }
    
    return Ok(should_refresh);
}

/** When the Sock coughs up a Msg, this function decides what to do with it.

Returns true if the program should quit.
*/
fn process_msg(m: Rcvr,
               scrn: &mut Screen,
               gv: &mut Globals)
-> Result<(), String> {
    debug!("process_msg(...): rec'd: {:?}", &m);
    match m {
        Rcvr::Ping => { gv.socket.enqueue(&PING); },
        
        Rcvr::Text { who, lines } => {
            for lin in &lines {
                let mut sl = Line::new();
                sl.pushf(&who, &scrn.styles().high);
                sl.push(": ");
                sl.push(lin);
                scrn.push_line(sl);
            }
        },
        
        Rcvr::Priv { who, text } => {
            let mut sl = Line::new();
            sl.push("$ ");
            sl.pushf(&who, &scrn.styles().dim);
            sl.push(": ");
            sl.push(&text);
            scrn.push_line(sl);
        },
        
        Rcvr::Logout(s) => {
            gv.messages.push(s);
            gv.run = false;
        },
        
        Rcvr::Info(s) => {
            let mut sl = Line::new();
            sl.push("* ");
            sl.push(&s);
            scrn.push_line(sl);
        },

        Rcvr::Err(s) => {
            let mut sl = Line::new();
            sl.pushf("# ", &scrn.styles().dim);
            sl.pushf(&s, &scrn.styles().dim);
            scrn.push_line(sl);
        },
        
        Rcvr::Misc { ref what, ref alt, ref data, } => match what.as_str() {
            "join" => {
                let (name, room) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                let mut sl = Line::new();
                sl.push("* ");
                if name.as_str() == gv.uname.as_str() {
                    sl.pushf("You", &scrn.styles().bold);
                    sl.push(" join ");

                    /* Set room name in upper-right status line. */
                    gv.rname = room.to_string();
                    let mut room_line = Line::new();
                    room_line.pushf(&gv.rname, &scrn.styles().high);
                    scrn.set_stat_ur(room_line);
                } else {
                    sl.pushf(name, &scrn.styles().high);
                    sl.push(" joins ");
                }
                sl.pushf(room, &scrn.styles().high);
                sl.push(".");
                gv.enqueue_bytes(&ROSTER_REQUEST);
                scrn.push_line(sl);
            },
            
            "leave" => {
                let (name, message) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                let mut sl = Line::new();
                sl.push("* ");
                sl.pushf(name, &scrn.styles().high);
                sl.push(" leaves: ");
                sl.push(message);
                gv.enqueue_bytes(&ROSTER_REQUEST);
                scrn.push_line(sl);
            },
            
            "priv_echo" => {
                let (name, text) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); }
                };
                let mut sl = Line::new();
                sl.push("$ ");
                sl.pushf("You", &scrn.styles().dim_bold);
                sl.pushf(" @ ", &scrn.styles().dim);
                sl.pushf(&name, &scrn.styles().high);
                sl.push(": ");
                sl.push(&text);
                scrn.push_line(sl);
            },
            
            "name" => {
                let (old, new) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                
                let mut sl = Line::new();
                sl.push("* ");
                if old.as_str() == gv.uname.as_str() {
                    sl.pushf("You", &scrn.styles().bold);
                    sl.push(" are now known as ");
                    gv.uname = new.clone();
                    write_mode_line(scrn, gv);
                } else {
                    sl.pushf(old, &scrn.styles().high);
                    sl.push(" is now known as ");
                }
                sl.pushf(new, &scrn.styles().high);
                sl.push(".");
                scrn.push_line(sl);
                gv.enqueue_bytes(&ROSTER_REQUEST);
            },
            
            "new_op" => {
                let (name, room) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                
                let mut sl = Line::new();
                sl.push("* ");
                if name == &gv.uname {
                    sl.pushf("You", &scrn.styles().bold);
                    sl.push(" are now the operator of ");
                } else {
                    sl.pushf(name, &scrn.styles().high);
                    sl.push(" is now the operator of ");
                }
                sl.pushf(room, &scrn.styles().bold);
                sl.push(".");
                scrn.push_line(sl);
                gv.enqueue_bytes(&ROSTER_REQUEST);
            }
            
            "roster" => {
                if data.len() < 1 { return Err(format!("Incomplete data: {:?}", &m)); }
                scrn.set_roster(data);
            },
            
            "kick_other" => {
                let (name, room) = match &data[..] {
                    [x, y] => (x, y),
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                let mut sl = Line::new();
                sl.push("* ");
                sl.pushf(name, &scrn.styles().high);
                sl.push(" has been kicked from ");
                sl.pushf(room, &scrn.styles().high);
                sl.push(".");
                scrn.push_line(sl);
                gv.enqueue_bytes(&ROSTER_REQUEST);
            },

            "kick_you" => {
                let room = match &data[..] {
                    [x] => x,
                    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
                };
                let mut sl = Line::new();
                sl.push("* ");
                sl.pushf("You", &scrn.styles().bold);
                sl.push(" have been kicked from ");
                sl.pushf(room, &scrn.styles().high);
                sl.push(".");
                scrn.push_line(sl);
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
            sl.push("# Unsupported Rcvr: ");
            sl.push(&s);
            scrn.push_line(sl);
        },
    }
    return Ok(());
}

/** When the mode line (in the lower-left-hand corner) should change,
this updates it.
*/
fn write_mode_line(scrn: &mut Screen, gv: &Globals) {
    let mut mode_line = Line::new();
    let mch: &str = match gv.mode {
        Mode::Command => "Com",
        Mode::Input => "Ipt",
    };
    mode_line.pushf(mch, &scrn.styles().high);
    mode_line.pushf(" | ", &scrn.styles().dim);
    mode_line.pushf(&(gv.uname), &scrn.styles().high);
    mode_line.push(" @ ");
    mode_line.pushf(&(gv.local_addr), &scrn.styles().high);
    scrn.set_stat_ll(mode_line);
}

fn main() {
    let cfg: ClientConfig = configure();
    #[cfg(debug)]
    let the_log_level = simplelog::LevelFilter::Trace;
    #[cfg(release)]
    let the_log_level = simplelog::LevelFilter::None;
    
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
        let b = Sndr::Query{ what: "addr", arg: "", }.bytes();
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
        run: true,
    };
    
    {
        let mut term = stdout();
        let mut scrn: Screen = match Screen::new(&mut term, cfg.roster_width){
            Ok(x) => x,
            Err(e) => {
                println!("Error setting up terminal: {}", e);
                std::process::exit(1);
            },
        };
        if let Some(cols) = cfg.colors {
            let uline = cols.underline_as_bold.unwrap_or(false);
            scrn.set_styles(
                cols.dim_foreground,
                cols.dim_background,
                cols.highlight_foreground,
                cols.highlight_background,
                uline);
        }
        
        let mut addr_line = Line::new();
        addr_line.pushf(&gv.server_addr, &scrn.styles().high);
        scrn.set_stat_ul(addr_line);
        let mut room_line = Line::new();
        room_line.pushf(&gv.rname, &scrn.styles().high);
        scrn.set_stat_ur(room_line);
        write_mode_line(&mut scrn, &gv);
        
        /* The 'main_loop repeats until the program should end, generally
        after disconnection.
        */
        'main_loop: loop {
            let loop_start = Instant::now();
            
            'input_loop: loop {
                match process_user_typing(&mut scrn, &mut gv) {
                    Err(e) => {
                        gv.messages.push(format!("Error getting event from keyboard: {}", e));
                        break 'main_loop;
                    },
                    Ok(true) => {
                        if let Err(e) = scrn.refresh(&mut term) {
                            gv.messages.push(format!("Error refreshing screen: {}", e));
                            break 'main_loop;
                        } else if gv.run == false {
                            break 'main_loop;
                        }
                    },
                    Ok(false) => { break 'input_loop; },
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
                                // This right here is what we call a hack.
                                // It'll get removed when `proto3` gets integrated.
                                //let newmsg: Rcvr = unsafe { std::mem::transmute(msg) };
                                match process_msg(msg, &mut scrn, &mut gv) {
                                    Ok(()) => { 
                                        if gv.run == false {
                                            break 'main_loop;
                                        }
                                    },
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
            
            /* If there are any changes to the state of the screen (I think
            everything but the receipt/sending of a `Msg::Ping` does this),
            redraw the areas that changed. */
            if let Err(e) = scrn.refresh(&mut term) {
                gv.messages.push(format!("Error refreshing screen: {}", e));
                break 'main_loop;
            }
            
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
