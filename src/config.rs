/*!
config.rs

The `grel` configuration struct and mechanism.

`grel` uses `confy` to read a configuration from a `.toml` file stored
in the "usual" place (`~/.config/grel/grel.toml` is one location; you may
need to play around with `confy` to figure out others).

2020-12-24
*/
use std::time::Duration;
use directories::BaseDirs;
use simplelog::LevelFilter;

const ADDR:          &str = "127.0.0.1:51516";
const SERVER_LOG:    &str = "greld.log";
const NAME:          &str = "grel user";
const SERVER_TICK:    u64 = 100;
const CLIENT_TICK:    u64 = 100;
const BLOCK_TIMEOUT:  u64 = 5000;
const READ_SIZE:    usize = 1024;
const ROSTER_WIDTH:   u16 = 24;
const CMD_CHAR:      char = ';';

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
            log_file: String::from(SERVER_LOG),
            log_level: 5,
        }
    }
}

/** The `GrelConfig` struct holds data read (and interpreted) from a
configuration file as public members.
*/
#[derive(Debug)]
pub struct ServerConfig {
    pub address: String,
    pub min_tick: Duration,
    pub blackout_time_to_ping: Duration,
    pub blackout_time_to_kick: Duration,
    pub max_user_name_length: usize,
    pub max_room_name_length: usize,
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
            log_file: cfgf.log_file,
            log_level: logl,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ClientConfigFile {
    address:      String,
    name:         String,
    timeout_ms:   u64,
    block_ms:     u64,
    read_size:    usize,
    roster_width: u16,
    cmd_char:     char,
}

impl std::default::Default for ClientConfigFile {
    fn default() -> Self {
        Self {
            address: String::from(ADDR),
            name:    String::from(NAME),
            timeout_ms:   CLIENT_TICK,
            block_ms:     BLOCK_TIMEOUT,
            read_size:    READ_SIZE,
            roster_width: ROSTER_WIDTH,
            cmd_char:     CMD_CHAR,
        }
    }
}

#[derive(Debug)]
pub struct ClientConfig {
    pub address:      String,
    pub name:         String,
    pub tick:         Duration,
    pub block:        Duration,
    pub read_size:    usize,
    pub roster_width: u16,
    pub cmd_char:     char,
}

impl ClientConfig {
    pub fn configure(path: Option<&str>) -> ClientConfig {
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
                println!("Error loading configuration: {}", e);
                std::process::exit(1);
            },
        };
        ClientConfig {
            address: f.address,
            name: f.name,
            tick: Duration::from_millis(f.timeout_ms),
            block: Duration::from_millis(f.block_ms),
            read_size: f.read_size,
            roster_width: f.roster_width,
            cmd_char: f.cmd_char,
        }
    }
    
    pub fn generate() -> Result<String, String> {
        let cfg = ClientConfigFile::default();
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
