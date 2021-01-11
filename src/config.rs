/*!
config.rs

The `grel` configuration struct and mechanism.

`grel` uses `confy` to read a configuration from a `.toml` file stored
in the "usual" place (`~/.config/grel/grel.toml` is one location; you may
need to play around with `confy` to figure out others).

2021-01-11
*/
use std::time::Duration;
use directories::BaseDirs;
use simplelog::LevelFilter;

const ADDR:           &str = "127.0.0.1:51516";
const SERVER_LOG:     &str = "greld.log";
const NAME:           &str = "grel user";
const LOBBY_NAME:     &str = "Lobby";
const WELCOME:        &str = "Welcome to a grel server.";
const SERVER_TICK:     u64 = 100;
const CLIENT_TICK:     u64 = 100;
const BLOCK_TIMEOUT:   u64 = 5000;
const READ_SIZE:     usize = 1024;
const ROSTER_WIDTH:    u16 = 24;
const CMD_CHAR:       char = ';';
const MIN_SCROLLBACK: usize = 1000;
const MAX_SCROLLBACK: usize = 2000;

/** The `GrelConfigFile` deserializes from a `.toml` file to a struct
of Rust primitives. Its values are then translated into less primitive
types (or at least some of them are) and shoved into a `GrelConfig`
struct (see below) for the program to actually use.
*/
#[derive(serde::Serialize, serde::Deserialize)]
struct ServerConfigFile {
    address: String,
    tick_ms: u64,
    blackout_to_ping_ms: u64,
    blackout_to_kick_ms: u64,
    max_user_name_length: usize,
    max_room_name_length: usize,
    lobby_name: String,
    welcome: String,
    log_file: String,
    log_level: u8,
}

/** `GrelConfigFile` implements `Default` because this is the mechanism
by which `confy` supplies default values in the case that options are
missing from the configuration file.
*/
impl std::default::Default for ServerConfigFile {
    fn default() -> Self {
        Self {
            address: String::from(ADDR),
            tick_ms: SERVER_TICK,
            blackout_to_ping_ms: 5000,
            blackout_to_kick_ms: 10000,
            max_user_name_length: 24,
            max_room_name_length: 24,
            lobby_name: String::from(LOBBY_NAME),
            welcome: String::from(WELCOME),
            log_file: String::from(SERVER_LOG),
            log_level: 5,
        }
    }
}

/** The `ServerConfig` struct holds data read (and interpreted) from a
server configuration file as public members.
*/
#[derive(Debug)]
pub struct ServerConfig {
    pub address: String,
    pub min_tick: Duration,
    pub blackout_time_to_ping: Duration,
    pub blackout_time_to_kick: Duration,
    pub max_user_name_length: usize,
    pub max_room_name_length: usize,
    pub lobby_name: String,
    pub welcome: String,
    pub log_file: String,
    pub log_level: LevelFilter,
}

impl ServerConfig {
    /** This single method reads a configuration file (probe `confy` for
    the details of exactly where it looks for files to read) and populates
    and returns a `ServerConfig` struct.
    */
    pub fn configure() -> ServerConfig {
        let cfgf: ServerConfigFile = match confy::load("greld") {
            Ok(x) => x,
            Err(e) => {
                println!("Error loading configuration: {}", e);
                std::process::exit(1);
            },
        };
        
        let logl: LevelFilter = match cfgf.log_level {
            0 => LevelFilter::Off,
            1 => LevelFilter::Error,
            2 => LevelFilter::Warn,
            3 => LevelFilter::Info,
            4 => LevelFilter::Debug,
            5 => LevelFilter::Trace,
            _ => {
                println!("Log levels higher than 5 not supported; setting to 5.");
                LevelFilter::Trace
            },
        };
        
        ServerConfig {
            address: cfgf.address,
            min_tick: Duration::from_millis(cfgf.tick_ms),
            blackout_time_to_ping: Duration::from_millis(cfgf.blackout_to_ping_ms),
            blackout_time_to_kick: Duration::from_millis(cfgf.blackout_to_kick_ms),
            max_user_name_length:  cfgf.max_user_name_length,
            max_room_name_length:  cfgf.max_room_name_length,
            lobby_name: cfgf.lobby_name,
            welcome: cfgf.welcome,
            log_file: cfgf.log_file,
            log_level: logl,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ClientConfigFile {
    address:        Option<String>,
    name:           Option<String>,
    timeout_ms:     Option<u64>,
    block_ms:       Option<u64>,
    read_size:      Option<usize>,
    roster_width:   Option<u16>,
    cmd_char:       Option<char>,
    max_scrollback: Option<usize>,
    min_scrollback: Option<usize>,
}

impl std::default::Default for ClientConfigFile {
    fn default() -> Self {
        Self {
            address:        None,
            name:           None,
            timeout_ms:     None,
            block_ms:       None,
            read_size:      None,
            roster_width:   None,
            cmd_char:       None,
            max_scrollback: None,
            min_scrollback: None,
        }
    }
}

/** The `ClientConfig` struct holds data read (and interpreted) from a
client configuration file as public members.
*/
#[derive(Debug)]
pub struct ClientConfig {
    pub address:        String,
    pub name:           String,
    pub tick:           Duration,
    pub block:          Duration,
    pub read_size:      usize,
    pub roster_width:   u16,
    pub cmd_char:       char,
    pub max_scrollback: usize,
    pub min_scrollback: usize,
}

impl ClientConfig {
    pub fn configure(path: Option<&str>) -> Result<ClientConfig, String> {
        /* I think I let myself get carried away with the matches here.
        The inner match matches on the option argument, calling a different
        confy function depending on whether a filename is supplied; the
        outer match matches on the Result from whichever function was called.
        */
        let f: ClientConfigFile = match {
            match path {
                None =>    confy::load("grel"),
                Some(s) => confy::load_path(s),
            }
        } {
            Ok(x) => x,
            Err(e) => {
                return Err(format!("Error loading configuration: {}", e));
            },
        };
        
        let max_scroll = f.max_scrollback.unwrap_or(MAX_SCROLLBACK);
        let min_scroll = f.min_scrollback.unwrap_or(MIN_SCROLLBACK);
        let cmd_char   = f.cmd_char.unwrap_or(CMD_CHAR);
        
        if max_scroll < min_scroll {
            return Err("max_scrollback cannot be smaller than min_scrollback".to_string());
        };
        if (cmd_char as u32) > 128 {
            return Err("cmd_char must be an ASCII character".to_string());
        };
        
        let cc = ClientConfig {
            address:      f.address .unwrap_or(String::from(ADDR)),
            name:         f.name    .unwrap_or(String::from(NAME)),
            tick:  Duration::from_millis(f.timeout_ms.unwrap_or(CLIENT_TICK)),
            block: Duration::from_millis(f.block_ms.unwrap_or(BLOCK_TIMEOUT)),
            read_size:    f.read_size.unwrap_or(READ_SIZE),
            roster_width: f.roster_width.unwrap_or(ROSTER_WIDTH),
            cmd_char:       cmd_char,
            max_scrollback: max_scroll,
            min_scrollback: min_scroll,
        };
        
        return Ok(cc);
    }
    
    pub fn generate() -> Result<String, String> {
        let cfg = ClientConfigFile {
            address:        Some(String::from(ADDR)),
            name:           Some(String::from(NAME)),
            timeout_ms:     Some(CLIENT_TICK),
            block_ms:       Some(BLOCK_TIMEOUT),
            read_size:      Some(READ_SIZE),
            roster_width:   Some(ROSTER_WIDTH),
            cmd_char:       Some(CMD_CHAR),
            max_scrollback: Some(MAX_SCROLLBACK),
            min_scrollback: Some(MIN_SCROLLBACK),
        };
        
        if let Err(e) = confy::store("grel", cfg) {
            return Err(format!("Error writing new configuration file: {}", e));
        }
        let bdirs = match BaseDirs::new() {
            None => { return Ok(String::from("a directory that could not be determined")); },
            Some(x) => x,
        };
        let mut d = std::path::PathBuf::from(bdirs.config_dir());
        d.push("grel"); d.push("grel");
        d.set_extension("toml");
        
        match d.to_str() {
            None => {
                let x = d.to_string_lossy();
                return Ok(String::from(x.as_ref()));
            },
            Some(x) => { return Ok(String::from(x)); },
        }
    }
}
