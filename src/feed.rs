use crate::db::Row;
use serde_json::{Value, json};
use std::path::Path;

/// Whole-number value of a column, 0 when it is missing or NULL.
fn int(row: &Row, key: &str) -> i64 {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)).unwrap_or(0),
        _ => 0,
    }
}

fn opt_int(row: &Row, key: &str) -> Value {
    match row.get(key) {
        Some(Value::Number(n)) => json!(n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64))),
        _ => Value::Null,
    }
}

fn text(row: &Row, key: &str) -> Value {
    row.get(key).cloned().unwrap_or(Value::Null)
}

/// Pallets still to build for a product. Whole units (combos, bulk, frames,
/// backs) count one per pallet; everything else is 50 cases to a pallet.
fn pallets_remaining(remaining: i64, description: &str) -> i64 {
    if remaining <= 0 {
        return 0;
    }
    let d = description.to_uppercase();
    if ["COMBO", "BULK", "FRAME", "FRAMES", "BACK", "BACKS"].iter().any(|k| d.contains(k)) {
        remaining
    } else {
        (remaining + 49) / 50
    }
}

pub fn production_rows(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let ordered = int(r, "CasesOrdered");
            let completed = int(r, "CasesCompleted");
            let remaining = (ordered - completed).max(0);
            let description = r.get("CaseDescription").and_then(Value::as_str).unwrap_or("");
            json!({
                "product_code": text(r, "LocalMaterialID"),
                "case_description": description,
                "cases_ordered": ordered,
                "cases_completed": completed,
                "cases_remaining": remaining,
                "pallets_remaining": pallets_remaining(remaining, description),
            })
        })
        .collect()
}

/// One point per pallet: when it last changed (seconds since midnight,
/// negative for an earlier day) and the cases on it for today's orders.
pub fn history_rows(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| json!({ "secs": int(r, "SecsIntoDay"), "cases": int(r, "Cases") }))
        .collect()
}

pub fn live_rows(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "pallet_id": text(r, "PalletID"),
                "scan_type": text(r, "ScanType"),
                "order_id": text(r, "LocalOrderID"),
                "customer": text(r, "CustomerName"),
                "product_code": text(r, "ProductCode"),
                "last_modified": text(r, "LastModified"),
                "cases_on_pallet": int(r, "CasesOnPallet"),
                "cases_ordered": opt_int(r, "CasesOrdered"),
                "cases_completed": int(r, "CasesCompleted"),
                "cases_remaining": opt_int(r, "CasesRemaining"),
            })
        })
        .collect()
}

/// The state of one feed file. `data` is the last good result: a database
/// outage leaves it in place and sets `ok` false with the reason, so the
/// dashboard can say the figures are old rather than showing nothing.
pub struct Feed {
    data: Vec<Value>,
    updated: Option<String>,
    updated_epoch: Option<i64>,
}

impl Feed {
    pub fn new() -> Self {
        Self { data: Vec::new(), updated: None, updated_epoch: None }
    }

    pub fn succeeded(&mut self, data: Vec<Value>) {
        let now = chrono::Local::now();
        self.data = data;
        self.updated = Some(now.format("%Y-%m-%dT%H:%M:%S").to_string());
        self.updated_epoch = Some(now.timestamp());
    }

    pub fn write(&self, path: &str, error: Option<&str>) -> std::io::Result<()> {
        let body = json!({
            "ok": error.is_none(),
            "error": error,
            "updated": self.updated,
            "updated_epoch": self.updated_epoch,
            "data": self.data,
        });
        let tmp = format!("{path}.tmp");
        if let Some(dir) = Path::new(path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&tmp, body.to_string())?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pallets_use_fifty_cases_unless_the_product_is_whole_units() {
        assert_eq!(pallets_remaining(0, "WINGS"), 0);
        assert_eq!(pallets_remaining(1, "WINGS"), 1);
        assert_eq!(pallets_remaining(50, "WINGS"), 1);
        assert_eq!(pallets_remaining(51, "WINGS"), 2);
        assert_eq!(pallets_remaining(7, "Combo Bin"), 7);
        assert_eq!(pallets_remaining(7, "chicken backs"), 7);
    }

    #[test]
    fn history_keeps_a_pallets_time_and_cases() {
        let mut r = Row::new();
        r.insert("SecsIntoDay".into(), json!(-3600));
        r.insert("Cases".into(), json!(48));
        let out = history_rows(&[r]);
        assert_eq!(out[0]["secs"], -3600);
        assert_eq!(out[0]["cases"], 48);
    }

    #[test]
    fn production_never_reports_negative_remaining() {
        let mut r = Row::new();
        r.insert("LocalMaterialID".into(), json!(1234));
        r.insert("CaseDescription".into(), json!("WINGS"));
        r.insert("CasesOrdered".into(), json!(100));
        r.insert("CasesCompleted".into(), json!(130));
        let out = production_rows(&[r]);
        assert_eq!(out[0]["cases_remaining"], 0);
        assert_eq!(out[0]["pallets_remaining"], 0);
    }
}
