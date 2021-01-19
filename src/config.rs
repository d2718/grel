/*!
config.rs

The `grel` configuration structs and mechanism.

`grel` uses
[the `directories` crate](https://docs.rs/directories/3.0.1/directories/)
to (try to) determine an appropriate location for configuration files.
If it can't, it will look to load (or generate) one in the current directory.

2021-01-19
*/
use std::time::Duration;
use std::path::{Path, PathBuf};

use simplelog::LevelFilter;

const CLIENT_NAME: &str = "grel.toml";
const SERVER_NAME: &str = "greld.toml";

const ADDR:           &str = "127.0.0.1:51516";
const SERVER_LOG:     &str = "greld.log";
const NAME:           &str = "grel user";
const LOBBY_NAME:     &str = "Lobby";
const WELCOME:        &str = "Welcome to a grel server.";
const SERVER_TICK:     u64 = 100;
const BYTE_LIMIT:    usize = 512;
const BYTE_TICK:     usize = 6;
const CLIENT_TICK:     u64 = 100;
const BLOCK_TIMEOUT:   u64 = 5000;
const READ_SIZE:     usize = 1024;
const ROSTER_WIDTH:    u16 = 24;
const CMD_CHAR:       char = ';';
const MIN_SCROLLBACK: usize = 1000;
const MAX_SCROLLBACK: usize = 2000;

fn default_config_dir() -> PathBuf {
    match directories::BaseDirs::new() {
        None => PathBuf::new(),
        Some(d) => d.config_dir().to_path_buf(),
    }
}

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
    byte_limit: usize,
    bytes_per_tick: usize,
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
            byte_limit: BYTE_LIMIT,
            bytes_per_tick: BYTE_TICK,
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
    pub byte_limit: usize,
    pub byte_tick: usize,
}

impl ServerConfig {
    /** This single method reads a configuration file (probe `confy` for
    the details of exactly where it looks for files to read) and populates
    and returns a `ServerConfig` struct.
    */
    pub fn configure() -> ServerConfig {
        
        let mut cfg_path = default_config_dir();
        cfg_path.push(SERVER_NAME);
        
        let cfgf: ServerConfigFile = match std::fs::read_to_string(&cfg_path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(x) => x,
                Err(e) => {
                    println!("Error parsing config file {}: {}",
                             &cfg_path.display(), &e);
                    std::process::exit(1);
                },
            },
            Err(e) => {
                println!("Error reading config file {}: {}",
                         &cfg_path.display(), &e);
                println!("Using default configuration.");
                ServerConfigFile::default()
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
            byte_limit: cfgf. byte_limit,
            byte_tick: cfgf.bytes_per_tick,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Colors {
    pub dim_foreground: Option<u8>,
    pub dim_background: Option<u8>,
    pub highlight_foreground: Option<u8>,
    pub highlight_background: Option<u8>,
    pub underline_as_bold: Option<bool>,
}

impl std::default::Default for Colors {
    fn default() -> Self {
        Self {
            dim_foreground: None,
            dim_background: None,
            highlight_foreground: None,
            highlight_background: None,
            underline_as_bold: None,
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
    colors: Option<Colors>,
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
            colors:         None,
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
    pub colors:         Option<Colors>,
}

impl ClientConfig {
    pub fn configure(path: Option<&str>) -> Result<ClientConfig, String> {
        let cfg_path = match path {
            Some(s) => Path::new(&s).to_path_buf(),
            None => {
                let mut p = default_config_dir().to_path_buf();
                p.push(CLIENT_NAME);
                p
            },
        };
        
        let f: ClientConfigFile = match std::fs::read_to_string(&cfg_path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(x) => x,
                Err(e) => { return Err(format!("Error parsing config file: {}", &e)); },
            },
            Err(e) => {
                println!("Error reading config file: {}", &e);
                println!("Using default configuration.");
                ClientConfigFile::default()
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
            colors:       f.colors,
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
            colors:         Some(Colors::default()),
        };
        
        let mut cfg_path = default_config_dir();
        cfg_path.push(CLIENT_NAME);
        let cfg_str = toml::to_string(&cfg).unwrap();
        
        match std::fs::write(&cfg_path, &cfg_str) {
            Ok(()) => match cfg_path.to_str() {
                Some(x) => { return Ok(String::from(x)); },
                None => { return Ok(cfg_path.to_string_lossy().to_string()); }
            },
            Err(e) =>{
                return Err(format!("Error writing new config file {}: {}",
                                   &cfg_path.display(), &e));
            },
        }
    }
}
