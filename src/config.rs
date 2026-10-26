use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const CONFIG_PATH: &str = "/home/pi/nexus.toml";

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub database: DatabaseCfg,
    #[serde(default)]
    pub orders: OrdersCfg,
    #[serde(default)]
    pub shifts: ShiftsCfg,
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
pub struct ShiftsCfg {
    /// When the night shift, and so the production day, starts ("HH:MM").
    #[serde(default = "default_night_start")]
    pub night_start: String,
    /// When the day shift starts. It runs until the next night shift.
    #[serde(default = "default_day_start")]
    pub day_start: String,
}
fn default_night_start() -> String { "23:00".to_string() }
fn default_day_start() -> String { "08:00".to_string() }
impl Default for ShiftsCfg {
    fn default() -> Self {
        Self { night_start: default_night_start(), day_start: default_day_start() }
    }
}

/// "23:00" as minutes after midnight, or None if it is not a time.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl ShiftsCfg {
    /// (night start, day start) in minutes after midnight. A time that does not
    /// parse is reported and replaced by its default.
    pub fn minutes(&self) -> (u32, u32) {
        let pick = |text: &str, name: &str, default: u32| {
            parse_hhmm(text).unwrap_or_else(|| {
                eprintln!("[shifts] {name} = \"{text}\" is not an HH:MM time — using the default");
                default
            })
        };
        (pick(&self.night_start, "night_start", 23 * 60), pick(&self.day_start, "day_start", 8 * 60))
    }

    /// Seconds from the production day's start to the day shift's start.
    pub fn day_shift_offset_secs(&self) -> i64 {
        let (night, day) = self.minutes();
        i64::from((day + 1440 - night) % 1440) * 60
    }
}

#[derive(Debug, Deserialize)]
pub struct OrdersCfg {
    /// A product with fewer cases than this ordered today counts as a combo.
    #[serde(default = "default_combo_below")]
    pub combo_below_cases: i64,
}
fn default_combo_below() -> i64 { 22 }
impl Default for OrdersCfg {
    fn default() -> Self { Self { combo_below_cases: default_combo_below() } }
}

#[derive(Debug, Deserialize)]
pub struct PollCfg {
    #[serde(default = "default_production_secs")]
    pub production_secs: f64,
    #[serde(default = "default_live_secs")]
    pub live_secs: f64,
    #[serde(default = "default_history_secs")]
    pub history_secs: f64,
    #[serde(default = "default_orders_secs")]
    pub orders_secs: f64,
    #[serde(default = "default_holds_secs")]
    pub holds_secs: f64,
    #[serde(default = "default_activity_secs")]
    pub activity_secs: f64,
    #[serde(default = "default_pallets_secs")]
    pub pallets_secs: f64,
    #[serde(default = "default_past_orders_secs")]
    pub past_orders_secs: f64,
    /// How long a query may run, per attempt. Feeds with a heavier query get
    /// at least `HEAVY_QUERY_SECS` (see `query_timeout`).
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: f64,
    /// How long connecting and logging in may take, per attempt.
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: f64,
    /// A query limit for one feed by name (`[poll.query_timeout_secs]`,
    /// `pallets = 45`), in place of `timeout_secs`.
    #[serde(default)]
    pub query_timeout_secs: HashMap<String, f64>,
}
/// Feeds whose queries read the large case and pallet tables, and so get more
/// time than the light ones unless nexus.toml says otherwise.
const HEAVY_FEEDS: [&str; 2] = ["pallets", "past_orders"];
const HEAVY_QUERY_SECS: f64 = 30.0;
impl PollCfg {
    /// The most a feed's query may run before it is reported as timed out.
    pub fn query_timeout(&self, feed: &str) -> Duration {
        let secs = match self.query_timeout_secs.get(feed) {
            Some(secs) => *secs,
            None if HEAVY_FEEDS.contains(&feed) => self.timeout_secs.max(HEAVY_QUERY_SECS),
            None => self.timeout_secs,
        };
        Duration::from_secs_f64(secs.max(1.0))
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs_f64(self.connect_timeout_secs.max(1.0))
    }
}
fn default_production_secs() -> f64 { 20.0 }
fn default_live_secs() -> f64 { 10.0 }
fn default_history_secs() -> f64 { 30.0 }
fn default_orders_secs() -> f64 { 30.0 }
fn default_holds_secs() -> f64 { 60.0 }
fn default_activity_secs() -> f64 { 30.0 }
fn default_pallets_secs() -> f64 { 60.0 }
fn default_past_orders_secs() -> f64 { 300.0 }
fn default_timeout_secs() -> f64 { 10.0 }
fn default_connect_timeout_secs() -> f64 { 10.0 }
impl Default for PollCfg {
    fn default() -> Self {
        Self {
            production_secs: default_production_secs(),
            live_secs: default_live_secs(),
            history_secs: default_history_secs(),
            orders_secs: default_orders_secs(),
            holds_secs: default_holds_secs(),
            activity_secs: default_activity_secs(),
            pallets_secs: default_pallets_secs(),
            past_orders_secs: default_past_orders_secs(),
            timeout_secs: default_timeout_secs(),
            connect_timeout_secs: default_connect_timeout_secs(),
            query_timeout_secs: HashMap::new(),
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
    #[serde(default = "default_orders_path")]
    pub orders: String,
    #[serde(default = "default_holds_path")]
    pub holds: String,
    #[serde(default = "default_activity_path")]
    pub activity: String,
    #[serde(default = "default_pallets_path")]
    pub pallets: String,
    #[serde(default = "default_past_orders_path")]
    pub past_orders: String,
}
fn default_production_path() -> String { "/home/pi/Git/nexus_production/production.json".to_string() }
fn default_live_path() -> String { "/home/pi/Git/nexus_production/live.json".to_string() }
fn default_history_path() -> String { "/home/pi/Git/nexus_production/history.json".to_string() }
fn default_orders_path() -> String { "/home/pi/Git/nexus_production/orders.json".to_string() }
fn default_holds_path() -> String { "/home/pi/Git/nexus_production/holds.json".to_string() }
fn default_activity_path() -> String { "/home/pi/Git/nexus_production/activity.json".to_string() }
fn default_pallets_path() -> String { "/home/pi/Git/nexus_production/pallets.json".to_string() }
fn default_past_orders_path() -> String { "/home/pi/Git/nexus_production/past_orders.json".to_string() }
impl Default for PathsCfg {
    fn default() -> Self {
        Self {
            production: default_production_path(),
            live: default_live_path(),
            history: default_history_path(),
            orders: default_orders_path(),
            holds: default_holds_path(),
            activity: default_activity_path(),
            pallets: default_pallets_path(),
            past_orders: default_past_orders_path(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_parse_as_minutes_after_midnight() {
        assert_eq!(parse_hhmm("23:00"), Some(1380));
        assert_eq!(parse_hhmm("8:05"), Some(485));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("noon"), None);
    }

    #[test]
    fn the_day_shift_starts_nine_hours_after_an_11_pm_night_shift() {
        assert_eq!(ShiftsCfg::default().day_shift_offset_secs(), 9 * 3600);
    }

    #[test]
    fn heavy_feeds_get_more_time_than_light_ones() {
        let poll = PollCfg::default();
        assert_eq!(poll.query_timeout("live"), Duration::from_secs(10));
        assert_eq!(poll.query_timeout("pallets"), Duration::from_secs(30));
        assert_eq!(poll.query_timeout("past_orders"), Duration::from_secs(30));
    }

    #[test]
    fn a_feed_named_in_the_config_uses_its_own_limit() {
        let poll: PollCfg = toml::from_str("timeout_secs = 5\n[query_timeout_secs]\npallets = 45\nlive = 2").unwrap();
        assert_eq!(poll.query_timeout("pallets"), Duration::from_secs(45));
        assert_eq!(poll.query_timeout("live"), Duration::from_secs(2));
        assert_eq!(poll.query_timeout("orders"), Duration::from_secs(5));
        assert_eq!(poll.query_timeout("past_orders"), Duration::from_secs(30));
    }
}
