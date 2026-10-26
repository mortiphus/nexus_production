use std::time::{Duration, Instant};
use tokio::time::{MissedTickBehavior, interval};

mod config;
mod db;
mod feed;
mod queries;
mod stats;

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

    let (night_start, _) = cfg.shifts.minutes();
    let queries: &'static queries::Queries = Box::leak(Box::new(queries::Queries::new(night_start)));

    let production = tokio::spawn(poll(cfg, queries, Kind::Production));
    let live = tokio::spawn(poll(cfg, queries, Kind::Live));
    let history = tokio::spawn(poll(cfg, queries, Kind::History));
    let orders = tokio::spawn(poll(cfg, queries, Kind::Orders));
    let holds = tokio::spawn(poll(cfg, queries, Kind::Holds));
    let activity = tokio::spawn(poll(cfg, queries, Kind::Activity));
    let pallets = tokio::spawn(poll(cfg, queries, Kind::Pallets));
    let past_orders = tokio::spawn(poll(cfg, queries, Kind::PastOrders));
    // Any task ending is a bug; exit so systemd restarts the service.
    let _ = tokio::join!(production, live, history, orders, holds, activity, pallets, past_orders);
    std::process::exit(1);
}

/// How often a feed that is still failing says so again in the journal.
const REMIND_EVERY: Duration = Duration::from_secs(600);
/// How often each feed logs how long its queries have been taking.
const SUMMARY_EVERY: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy)]
enum Kind {
    Production,
    Live,
    History,
    Orders,
    Holds,
    Activity,
    Pallets,
    PastOrders,
}

async fn poll(cfg: &'static config::Config, queries: &'static queries::Queries, kind: Kind) {
    let (name, secs, sql, path) = match kind {
        Kind::Production => ("production", cfg.poll.production_secs, &queries.production, &cfg.paths.production),
        Kind::Live => ("live", cfg.poll.live_secs, &queries.live, &cfg.paths.live),
        Kind::History => ("history", cfg.poll.history_secs, &queries.history, &cfg.paths.history),
        Kind::Orders => ("orders", cfg.poll.orders_secs, &queries.orders, &cfg.paths.orders),
        Kind::Holds => ("holds", cfg.poll.holds_secs, &queries.holds, &cfg.paths.holds),
        Kind::Activity => ("activity", cfg.poll.activity_secs, &queries.activity, &cfg.paths.activity),
        Kind::Pallets => ("pallets", cfg.poll.pallets_secs, &queries.pallets, &cfg.paths.pallets),
        Kind::PastOrders => ("past_orders", cfg.poll.past_orders_secs, &queries.past_orders, &cfg.paths.past_orders),
    };
    let limits = db::Limits { connect: cfg.poll.connect_timeout(), query: cfg.poll.query_timeout(name) };
    let mut tick = interval(Duration::from_secs_f64(secs.max(1.0)));
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let (night_start, _) = cfg.shifts.minutes();
    let day_shift_at = cfg.shifts.day_shift_offset_secs();
    let mut feed = feed::Feed::new().with_meta(serde_json::json!({
        "day_start": i64::from(night_start) * 60,
        "day_shift_at": day_shift_at,
        // How often this file is rewritten, so the dashboard knows how old is too old.
        "interval_secs": secs.max(1.0),
    }));
    let mut last_error: Option<String> = None;
    let mut failing_since = Instant::now();
    let mut last_logged = Instant::now();
    let mut was_slow = false;
    let mut stats = stats::PollStats::new(Instant::now());
    loop {
        tick.tick().await;
        let started = Instant::now();
        let result = db::run_query(&cfg.database, sql, limits).await;
        let elapsed = started.elapsed();
        stats.record(elapsed);
        if result.is_ok() {
            let slow = stats::is_slow(elapsed, limits.query);
            if slow != was_slow {
                if slow {
                    eprintln!("{name}: slow query, {:.1}s of the {}s allowed", elapsed.as_secs_f64(), limits.query.as_secs_f64());
                } else {
                    println!("{name}: query back to normal speed, {:.1}s", elapsed.as_secs_f64());
                }
                was_slow = slow;
            }
        }
        if let Some(line) = stats.summary(Instant::now(), SUMMARY_EVERY) {
            println!("{name}: last hour: {line}");
        }
        let error = match result {
            Ok(rows) => {
                feed.succeeded(match kind {
                    Kind::Production => feed::production_rows(&rows, cfg.orders.combo_below_cases),
                    Kind::Live => feed::live_rows(&rows),
                    Kind::History => feed::history_rows(&rows, cfg.orders.combo_below_cases, day_shift_at),
                    Kind::Orders => feed::orders_rows(&rows),
                    Kind::Holds => feed::holds_rows(&rows),
                    Kind::Activity => feed::activity_rows(&rows, day_shift_at),
                    Kind::Pallets => feed::pallets_rows(&rows, day_shift_at),
                    Kind::PastOrders => feed::past_orders_rows(&rows),
                });
                None
            }
            Err(e) => Some(e),
        };
        // Report a change, not every failed attempt, and a reminder every so
        // often while it lasts so the journal shows how long it has gone on.
        if error != last_error {
            match &error {
                Some(e) => {
                    eprintln!("{name}: {e}");
                    failing_since = Instant::now();
                    last_logged = Instant::now();
                }
                None => println!(
                    "{name}: database reachable again after {} min",
                    failing_since.elapsed().as_secs() / 60
                ),
            }
            last_error = error.clone();
        } else if let Some(e) = &error {
            if last_logged.elapsed() >= REMIND_EVERY {
                eprintln!("{name}: still failing after {} min: {e}", failing_since.elapsed().as_secs() / 60);
                last_logged = Instant::now();
            }
        }
        if let Err(e) = feed.write(path, error.as_deref()) {
            eprintln!("{name}: cannot write {path}: {e}");
            std::process::exit(1);
        }
    }
}
