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

/// A column as a decimal, 0 when it is missing or NULL.
fn num(row: &Row, key: &str) -> f64 {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
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

pub fn production_rows(rows: &[Row], combo_below: i64) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let ordered = int(r, "CasesOrdered");
            let completed = int(r, "CasesCompleted");
            let remaining = (ordered - completed).max(0);
            let description = r.get("CaseDescription").and_then(Value::as_str).unwrap_or("");
            json!({
                "product_code": text(r, "LocalMaterialID"),
                "case_description": description,
                "is_combo": ordered < combo_below,
                "cases_ordered": ordered,
                "cases_completed": completed,
                "cases_remaining": remaining,
                "pallets_remaining": pallets_remaining(remaining, description),
            })
        })
        .collect()
}

/// Which shift a pallet was built in: "pre" before the production day began
/// (built ahead of time), "night" until the day shift starts, then "day".
pub fn shift_of(secs: i64, day_shift_at: i64) -> &'static str {
    if secs < 0 {
        "pre"
    } else if secs < day_shift_at {
        "night"
    } else {
        "day"
    }
}

/// One point per pallet and product: when it last changed (seconds since
/// midnight, negative for an earlier day), the cases on it for today's orders,
/// and whether they are combos.
pub fn history_rows(rows: &[Row], combo_below: i64, day_shift_at: i64) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let secs = int(r, "SecsIntoDay");
            json!({
                "pallet": text(r, "PalletID"),
                "product": text(r, "ProductCode"),
                "secs": secs,
                "shift": shift_of(secs, day_shift_at),
                "cases": int(r, "Cases"),
                "net_lb": num(r, "NetLb"),
                "label_lb": num(r, "LabelLb"),
                "combo": int(r, "ProductOrdered") < combo_below,
            })
        })
        .collect()
}

/// One row per order for the production day.
pub fn orders_rows(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "order": text(r, "OrderNo"),
                "customer": text(r, "Customer"),
                "transfer": r.get("Transfer").and_then(Value::as_bool).unwrap_or(false),
                "due_secs": int(r, "DueSecs"),
                "ordered": int(r, "Ordered"),
                "built": int(r, "Built"),
            })
        })
        .collect()
}

/// One row per pallet built in the production day, for orders or inventory.
pub fn activity_rows(rows: &[Row], day_shift_at: i64) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            let secs = int(r, "SecsIntoDay");
            json!({
                "pallet": text(r, "PalletID"),
                "secs": secs,
                "shift": shift_of(secs, day_shift_at),
                "order": int(r, "ForOrder") == 1,
            })
        })
        .collect()
}

/// One row per hold that has pallets on it.
pub fn holds_rows(rows: &[Row]) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "hold": text(r, "Hold"),
                "pallets": int(r, "Pallets"),
                "cases": int(r, "Cases"),
                "since": text(r, "Since"),
            })
        })
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
                "built_at": text(r, "BuiltTime"),
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
    /// Extra top-level fields for the file, such as when the production day
    /// starts. The dashboard needs them to turn times into clock times.
    meta: serde_json::Map<String, Value>,
}

impl Feed {
    pub fn new() -> Self {
        Self { data: Vec::new(), updated: None, updated_epoch: None, meta: serde_json::Map::new() }
    }

    pub fn with_meta(mut self, meta: Value) -> Self {
        if let Value::Object(m) = meta {
            self.meta = m;
        }
        self
    }

    pub fn succeeded(&mut self, data: Vec<Value>) {
        let now = chrono::Local::now();
        self.data = data;
        self.updated = Some(now.format("%Y-%m-%dT%H:%M:%S").to_string());
        self.updated_epoch = Some(now.timestamp());
    }

    pub fn write(&self, path: &str, error: Option<&str>) -> std::io::Result<()> {
        let mut body = json!({
            "ok": error.is_none(),
            "error": error,
            "updated": self.updated,
            "updated_epoch": self.updated_epoch,
            "data": self.data,
        });
        if let Value::Object(b) = &mut body {
            for (k, v) in &self.meta {
                b.insert(k.clone(), v.clone());
            }
        }
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
        r.insert("ProductOrdered".into(), json!(300));
        r.insert("NetLb".into(), json!(2011.5));
        r.insert("LabelLb".into(), json!(2000.0));
        r.insert("PalletID".into(), json!(88123));
        r.insert("ProductCode".into(), json!(1002));
        let out = history_rows(&[r], 22, 32_400);
        assert_eq!(out[0]["secs"], -3600);
        assert_eq!(out[0]["cases"], 48);
        assert_eq!(out[0]["combo"], false);
        assert_eq!(out[0]["pallet"], 88123);
        assert_eq!(out[0]["product"], 1002);
        assert_eq!(out[0]["shift"], "pre");
        assert_eq!(out[0]["net_lb"], 2011.5);
        assert_eq!(out[0]["label_lb"], 2000.0);
    }

    #[test]
    fn an_order_row_carries_its_due_time_and_progress() {
        let mut r = Row::new();
        r.insert("OrderNo".into(), json!("SON1"));
        r.insert("Customer".into(), json!("Kroger"));
        r.insert("Transfer".into(), json!(false));
        r.insert("DueSecs".into(), json!(32_400));
        r.insert("Ordered".into(), json!(300));
        r.insert("Built".into(), json!(120));
        let out = orders_rows(&[r]);
        assert_eq!(out[0]["due_secs"], 32_400);
        assert_eq!(out[0]["transfer"], false);
        assert_eq!(out[0]["built"], 120);
    }

    #[test]
    fn activity_marks_inventory_pallets_and_their_shift() {
        let mut r = Row::new();
        r.insert("PalletID".into(), json!(7));
        r.insert("SecsIntoDay".into(), json!(40_000));
        r.insert("ForOrder".into(), json!(0));
        let out = activity_rows(&[r], 32_400);
        assert_eq!(out[0]["order"], false);
        assert_eq!(out[0]["shift"], "day");
    }

    #[test]
    fn pallets_are_tagged_by_the_shift_they_were_built_in() {
        // Production day starts at 11 PM; the day shift starts nine hours later.
        assert_eq!(shift_of(-1, 32_400), "pre");
        assert_eq!(shift_of(0, 32_400), "night");
        assert_eq!(shift_of(32_399, 32_400), "night");
        assert_eq!(shift_of(32_400, 32_400), "day");
    }

    #[test]
    fn a_product_ordered_under_the_limit_is_a_combo() {
        let mut r = Row::new();
        r.insert("LocalMaterialID".into(), json!(9));
        r.insert("CaseDescription".into(), json!("WINGS"));
        r.insert("CasesOrdered".into(), json!(21));
        r.insert("CasesCompleted".into(), json!(0));
        assert_eq!(production_rows(&[r.clone()], 22)[0]["is_combo"], true);
        r.insert("CasesOrdered".into(), json!(22));
        assert_eq!(production_rows(&[r], 22)[0]["is_combo"], false);
    }

    #[test]
    fn production_never_reports_negative_remaining() {
        let mut r = Row::new();
        r.insert("LocalMaterialID".into(), json!(1234));
        r.insert("CaseDescription".into(), json!("WINGS"));
        r.insert("CasesOrdered".into(), json!(100));
        r.insert("CasesCompleted".into(), json!(130));
        let out = production_rows(&[r], 22);
        assert_eq!(out[0]["cases_remaining"], 0);
        assert_eq!(out[0]["pallets_remaining"], 0);
    }
}
