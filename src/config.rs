use serde::Deserialize;

const CONFIG_PATH: &str = "/home/pi/nexus.toml";

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub database: DatabaseCfg,
    #[serde(default)]
    pub poll: PollCfg,
    #[serde(default)]
    pub paths: PathsCfg,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseCfg {
    #[serde(default = "default_server")]
    pub server: String,
    #[serde(default = "default_instance")]
    pub instance: Option<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_database")]
    pub database: String,
    #[serde(default = "default_username")]
    pub username: String,
    /// Empty until nexus.toml or NEXUS_DB_PASSWORD supplies it.
    #[serde(default)]
    pub password: String,
}
fn default_server() -> String { "192.168.1.12".to_string() }
fn default_instance() -> Option<String> { Some("LYNX".to_string()) }
fn default_port() -> u16 { 1433 }
fn default_database() -> String { "Nexus_Master".to_string() }
fn default_username() -> String { "ReadOnlyUser".to_string() }
impl Default for DatabaseCfg {
    fn default() -> Self {
        Self {
            server: default_server(),
            instance: default_instance(),
            port: default_port(),
            database: default_database(),
            username: default_username(),
            password: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PollCfg {
    #[serde(default = "default_production_secs")]
    pub production_secs: f64,
    #[serde(default = "default_live_secs")]
    pub live_secs: f64,
    #[serde(default = "default_history_secs")]
    pub history_secs: f64,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: f64,
}
fn default_production_secs() -> f64 { 20.0 }
fn default_live_secs() -> f64 { 10.0 }
fn default_history_secs() -> f64 { 30.0 }
fn default_timeout_secs() -> f64 { 10.0 }
impl Default for PollCfg {
    fn default() -> Self {
        Self {
            production_secs: default_production_secs(),
            live_secs: default_live_secs(),
            history_secs: default_history_secs(),
            timeout_secs: default_timeout_secs(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct PathsCfg {
    #[serde(default = "default_production_path")]
    pub production: String,
    #[serde(default = "default_live_path")]
    pub live: String,
    #[serde(default = "default_history_path")]
    pub history: String,
}
fn default_production_path() -> String { "/home/pi/Git/nexus_production/production.json".to_string() }
fn default_live_path() -> String { "/home/pi/Git/nexus_production/live.json".to_string() }
fn default_history_path() -> String { "/home/pi/Git/nexus_production/history.json".to_string() }
impl Default for PathsCfg {
    fn default() -> Self {
        Self {
            production: default_production_path(),
            live: default_live_path(),
            history: default_history_path(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = std::env::var("NEXUS_CONFIG").unwrap_or_else(|_| CONFIG_PATH.to_string());
        let mut cfg = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("Config parse error in {path}: {e} — using defaults");
                    Self::default()
                }
            },
            Err(_) => {
                eprintln!("No config file at {path} — using defaults");
                Self::default()
            }
        };
        if let Ok(pw) = std::env::var("NEXUS_DB_PASSWORD") {
            cfg.database.password = pw;
        }
        cfg
    }
}
