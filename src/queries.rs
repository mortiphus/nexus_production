//! The three read-only queries. Production and Live are the Rosehill
//! dashboard's queries, History is new.
//!
//! "Today" is the production day, which starts when the night shift does (11 PM
//! by default) and is named for the morning it ends in: pallets built from 11 PM
//! Monday are for Tuesday's orders. In the SQL, __PROD_DATE__ is that date and
//! __DAY_START__ is when the production day began; `Queries::new` fills both in.
//! The scan time in Live is formatted by the database (HH:MM:SS) so no date
//! type needs decoding here.

pub const PRODUCTION: &str = r#"
WITH OrderedCTE AS (
    SELECT
        p.LocalMaterialID,
        p.CaseDescription,
        SUM(li.CasesOrdered) AS CasesOrdered
    FROM Orders o
    JOIN OrderLineItems li
        ON o.OrderID = li.OrderID
    JOIN Products p
        ON li.ProductID = p.ProductID
    WHERE CAST(o.EstimatedProductionDate AS DATE) = __PROD_DATE__
    GROUP BY
        p.LocalMaterialID,
        p.CaseDescription
),
CompletedCTE AS (
    SELECT
        p.LocalMaterialID,
        SUM(pc.CaseCount) AS CasesCompleted
    FROM PalletContents pc
    JOIN OrderLineItems li
        ON pc.LineItemID = li.LineItemID
    JOIN Orders o
        ON li.OrderID = o.OrderID
    JOIN Products p
        ON li.ProductID = p.ProductID
    WHERE CAST(o.EstimatedProductionDate AS DATE) = __PROD_DATE__
    GROUP BY p.LocalMaterialID
)
SELECT
    o.LocalMaterialID,
    o.CaseDescription,
    o.CasesOrdered,
    ISNULL(c.CasesCompleted, 0) AS CasesCompleted
FROM OrderedCTE o
LEFT JOIN CompletedCTE c
    ON o.LocalMaterialID = c.LocalMaterialID
WHERE o.CasesOrdered > 0
ORDER BY o.LocalMaterialID
"#;

pub const LIVE: &str = r#"
WITH RecentPallets AS (
    SELECT TOP 5
        p.PalletID,
        p.OrderID,
        p.LastModified
    FROM dbo.Pallets p
    WHERE p.LastModified >= __DAY_START__
    ORDER BY p.LastModified DESC
),
PalletDetail AS (
    SELECT
        rp.PalletID,
        rp.OrderID,
        rp.LastModified,
        pc.LineItemID,
        pc.ProductID,
        SUM(pc.CaseCount) AS CasesOnPallet
    FROM RecentPallets rp
    JOIN dbo.PalletContents pc
        ON pc.PalletID = rp.PalletID
    GROUP BY
        rp.PalletID,
        rp.OrderID,
        rp.LastModified,
        pc.LineItemID,
        pc.ProductID
),
CompletedByLineItem AS (
    SELECT
        pc.LineItemID,
        SUM(pc.CaseCount) AS CasesCompleted
    FROM dbo.PalletContents pc
    WHERE pc.LineItemID IS NOT NULL
    GROUP BY pc.LineItemID
)
SELECT
    pd.PalletID,
    CASE
        WHEN pd.OrderID IS NULL THEN 'Inventory'
        ELSE 'Sales Order'
    END AS ScanType,
    o.LocalOrderID,
    c.CustomerName,
    COALESCE(pr_li.LocalProductID, pr_pc.LocalProductID) AS ProductCode,
    CONVERT(VARCHAR(8), pd.LastModified, 108) AS LastModified,
    pd.CasesOnPallet,
    li.CasesOrdered,
    ISNULL(cb.CasesCompleted, 0) AS CasesCompleted,
    CASE
        WHEN li.CasesOrdered IS NULL THEN NULL
        ELSE li.CasesOrdered - ISNULL(cb.CasesCompleted, 0)
    END AS CasesRemaining
FROM PalletDetail pd
LEFT JOIN dbo.OrderLineItems li
    ON li.LineItemID = pd.LineItemID
LEFT JOIN dbo.Products pr_li
    ON pr_li.ProductID = li.ProductID
LEFT JOIN dbo.Products pr_pc
    ON pr_pc.ProductID = pd.ProductID
LEFT JOIN dbo.Orders o
    ON o.OrderID = li.OrderID
LEFT JOIN dbo.Customers c
    ON c.CustomerID = o.CustomerID
LEFT JOIN CompletedByLineItem cb
    ON cb.LineItemID = pd.LineItemID
ORDER BY
    pd.LastModified DESC
"#;

/// Cases built into pallets for today's orders, one row per pallet and
/// product, with the time of its last change as seconds since midnight
/// (negative: before today). ProductOrdered is the product's total ordered
/// today, which decides whether it counts as a combo. It counts the same cases
/// as PRODUCTION's CasesCompleted, so the running total ends where that
/// figure does.
pub const HISTORY: &str = r#"
WITH OrderedCTE AS (
    SELECT
        p.LocalMaterialID,
        SUM(li.CasesOrdered) AS CasesOrdered
    FROM Orders o
    JOIN OrderLineItems li
        ON o.OrderID = li.OrderID
    JOIN Products p
        ON li.ProductID = p.ProductID
    WHERE CAST(o.EstimatedProductionDate AS DATE) = __PROD_DATE__
    GROUP BY p.LocalMaterialID
)
SELECT
    pal.PalletID AS PalletID,
    pr.LocalMaterialID AS ProductCode,
    DATEDIFF(SECOND, __DAY_START__, pal.Created) AS SecsIntoDay,
    ord.CasesOrdered AS ProductOrdered,
    SUM(pc.CaseCount) AS Cases
FROM dbo.Pallets pal
JOIN dbo.PalletContents pc
    ON pc.PalletID = pal.PalletID
JOIN dbo.OrderLineItems li
    ON li.LineItemID = pc.LineItemID
JOIN dbo.Orders o
    ON o.OrderID = li.OrderID
JOIN dbo.Products pr
    ON pr.ProductID = li.ProductID
JOIN OrderedCTE ord
    ON ord.LocalMaterialID = pr.LocalMaterialID
WHERE CAST(o.EstimatedProductionDate AS DATE) = __PROD_DATE__
GROUP BY pal.PalletID, pal.Created, pr.LocalMaterialID, ord.CasesOrdered
ORDER BY pal.Created
"#;

pub struct Queries {
    pub production: String,
    pub live: String,
    pub history: String,
}

impl Queries {
    /// `night_start_mins` is when the production day starts, in minutes after
    /// midnight (1380 for 11 PM).
    pub fn new(night_start_mins: u32) -> Self {
        // Minutes to add to now to land on the date the production day is named
        // for, and to take back off midnight of that date to find its start.
        let x = (1440 - night_start_mins % 1440) % 1440;
        let prod_date = format!("CAST(DATEADD(MINUTE, {x}, GETDATE()) AS DATE)");
        let day_start = format!("DATEADD(MINUTE, -{x}, CAST({prod_date} AS DATETIME))");
        let fill = |sql: &str| sql.replace("__DAY_START__", &day_start).replace("__PROD_DATE__", &prod_date);
        Self { production: fill(PRODUCTION), live: fill(LIVE), history: fill(HISTORY) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_11_pm_production_day_is_named_for_the_next_morning() {
        let q = Queries::new(23 * 60);
        assert!(q.production.contains("DATEADD(MINUTE, 60, GETDATE())"));
        assert!(q.history.contains("DATEADD(MINUTE, -60, CAST("));
        assert!(!q.production.contains("__"));
        assert!(!q.live.contains("__"));
        assert!(!q.history.contains("__"));
    }

    #[test]
    fn a_midnight_start_is_the_calendar_day() {
        let q = Queries::new(0);
        assert!(q.production.contains("DATEADD(MINUTE, 0, GETDATE())"));
    }
}
