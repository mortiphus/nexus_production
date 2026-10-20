use std::time::Duration;
use tokio::time::{MissedTickBehavior, interval};

mod config;
mod db;
mod feed;
mod queries;

// ========================================
// Name: nexus_production (version in Cargo.toml, stamped by build.rs)
// Author: David Beckwith
// License: GNU General Public License v2.0
// ========================================

#[tokio::main]
async fn main() {
    println!("nexus_production {} · {}", env!("APP_VERSION"), env!("GIT_COMMIT"));
    let cfg = config::Config::load();
    if cfg.database.password.is_empty() {
        eprintln!("No database password: set [database] password in nexus.toml or NEXUS_DB_PASSWORD");
    }
    let cfg: &'static config::Config = Box::leak(Box::new(cfg));

    let production = tokio::spawn(poll(cfg, Kind::Production));
    let live = tokio::spawn(poll(cfg, Kind::Live));
    // Either task ending is a bug; exit so systemd restarts the service.
    let _ = tokio::join!(production, live);
    std::process::exit(1);
}

#[derive(Clone, Copy)]
enum Kind {
    Production,
    Live,
}

async fn poll(cfg: &'static config::Config, kind: Kind) {
    let (name, secs, sql, path) = match kind {
        Kind::Production => ("production", cfg.poll.production_secs, queries::PRODUCTION, &cfg.paths.production),
        Kind::Live => ("live", cfg.poll.live_secs, queries::LIVE, &cfg.paths.live),
    };
    let limit = Duration::from_secs_f64(cfg.poll.timeout_secs.max(1.0));
    let mut tick = interval(Duration::from_secs_f64(secs.max(1.0)));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut feed = feed::Feed::new();
    let mut last_error: Option<String> = None;
    loop {
        tick.tick().await;
        let error = match db::run_query(&cfg.database, sql, limit).await {
            Ok(rows) => {
                feed.succeeded(match kind {
                    Kind::Production => feed::production_rows(&rows),
                    Kind::Live => feed::live_rows(&rows),
                });
                None
            }
            Err(e) => Some(e),
        };
        // Report a change, not every failed attempt.
        if error != last_error {
            match &error {
                Some(e) => eprintln!("{name}: {e}"),
                None => println!("{name}: database reachable again"),
            }
            last_error = error.clone();
        }
        if let Err(e) = feed.write(path, error.as_deref()) {
            eprintln!("{name}: cannot write {path}: {e}");
            std::process::exit(1);
        }
    }
}
