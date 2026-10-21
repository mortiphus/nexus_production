use crate::config::DatabaseCfg;
use serde_json::{Map, Value, json};
use std::time::Duration;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, SqlBrowser};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// One result row as a JSON object keyed by column name.
pub type Row = Map<String, Value>;

/// Pause before the one retry, long enough for a deadlock to clear.
const RETRY_DELAY: Duration = Duration::from_millis(1500);

/// Connects, runs one query and returns its rows. A fresh connection each
/// time: the queries run every few seconds, and a dropped connection then
/// costs nothing to recover from. A connect failure or a deadlock is tried
/// once more before it is reported, since both usually clear within seconds
/// and a reported failure puts "Database unreachable" on the dashboard.
pub async fn run_query(db: &DatabaseCfg, sql: &str, limit: Duration) -> Result<Vec<Row>, String> {
    match attempt(db, sql, limit).await {
        Err(e) if is_transient(&e) => {
            tokio::time::sleep(RETRY_DELAY).await;
            attempt(db, sql, limit).await
        }
        result => result,
    }
}

async fn attempt(db: &DatabaseCfg, sql: &str, limit: Duration) -> Result<Vec<Row>, String> {
    timeout(limit, run(db, sql))
        .await
        .map_err(|_| format!("timed out after {}s", limit.as_secs()))?
}

/// Errors worth one more try: no connection (SQL Browser timing out while
/// resolving the instance), a failed login, or this query picked as the
/// victim of a deadlock. A timeout is not retried; it already took the whole
/// allowance.
fn is_transient(error: &str) -> bool {
    error.starts_with("connect:") || error.starts_with("login:") || error.contains("deadlock victim")
}

async fn run(db: &DatabaseCfg, sql: &str) -> Result<Vec<Row>, String> {
    let mut cfg = Config::new();
    cfg.host(&db.server);
    match &db.instance {
        Some(name) => cfg.instance_name(name),
        None => cfg.port(db.port),
    }
    cfg.database(&db.database);
    cfg.authentication(AuthMethod::sql_server(&db.username, &db.password));
    // Same as the Rosehill dashboard: Encrypt=no, TrustServerCertificate=yes.
    cfg.encryption(EncryptionLevel::NotSupported);
    cfg.trust_cert();

    let tcp = TcpStream::connect_named(&cfg).await.map_err(|e| format!("connect: {e}"))?;
    tcp.set_nodelay(true).ok();
    let mut client = Client::connect(cfg, tcp.compat_write())
        .await
        .map_err(|e| format!("login: {e}"))?;

    let rows = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("query: {e}"))?
        .into_first_result()
        .await
        .map_err(|e| format!("query: {e}"))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let names: Vec<String> = row.columns().iter().map(|c| c.name().to_string()).collect();
            names
                .into_iter()
                .zip(row.into_iter())
                .map(|(name, data)| (name, to_json(data)))
                .collect()
        })
        .collect())
}

/// Any column value as JSON. Whole numbers stay integers; the column types in
/// the Nexus tables are not known here, so every numeric type is accepted.
fn to_json(data: ColumnData<'_>) -> Value {
    match data {
        ColumnData::U8(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::I16(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::I32(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::I64(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::F32(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::F64(v) => v.map_or(Value::Null, |n| json!(n)),
        ColumnData::Bit(v) => v.map_or(Value::Null, |b| json!(b)),
        ColumnData::String(v) => v.map_or(Value::Null, |s| json!(s.trim_end())),
        ColumnData::Guid(v) => v.map_or(Value::Null, |g| json!(g.to_string())),
        ColumnData::Numeric(v) => v.map_or(Value::Null, |n| {
            if n.scale() == 0 {
                json!(n.value() as i64)
            } else {
                json!(n.value() as f64 / 10f64.powi(n.scale() as i32))
            }
        }),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::is_transient;

    #[test]
    fn a_sql_browser_timeout_is_retried() {
        assert!(is_transient("connect: Conversion error: SQL browser timeout during resolving instance LYNX."));
    }

    #[test]
    fn a_deadlock_victim_is_retried() {
        assert!(is_transient("query: Token error: 'Transaction (Process ID 94) was deadlocked on lock resources with another process and has been chosen as the deadlock victim. Rerun the transaction.' on server X"));
    }

    #[test]
    fn a_timeout_or_bad_sql_is_not_retried() {
        assert!(!is_transient("timed out after 10s"));
        assert!(!is_transient("query: Token error: 'Invalid column name' on server X"));
    }
}
