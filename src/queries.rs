//! The read-only queries. Production and Live began as the Rosehill
//! dashboard's queries (Live now goes by when a pallet was built, not when it
//! was last touched); History, Orders and Holds are new.
//!
//! "Today" is the production day, which starts when the night shift does (11 PM
//! by default) and is named for the morning it ends in: pallets built from 11 PM
//! Monday are for Tuesday's orders. In the SQL, __PROD_DATE__ is that date and
//! __DAY_START__ is when the production day began; `Queries::new` fills both in.
//! Times are returned as text or as seconds, so no date type needs decoding.

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
        p.Created
    FROM dbo.Pallets p
    WHERE p.Created >= __DAY_START__
    ORDER BY p.Created DESC
),
PalletDetail AS (
    SELECT
        rp.PalletID,
        rp.OrderID,
        rp.Created,
        pc.LineItemID,
        pc.ProductID,
        SUM(pc.CaseCount) AS CasesOnPallet
    FROM RecentPallets rp
    JOIN dbo.PalletContents pc
        ON pc.PalletID = rp.PalletID
    GROUP BY
        rp.PalletID,
        rp.OrderID,
        rp.Created,
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
    CONVERT(VARCHAR(8), pd.Created, 108) AS BuiltTime,
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
    pd.Created DESC
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
    SUM(pc.CaseCount) AS Cases,
    SUM(pc.NetWeight) AS NetLb,
    SUM(pc.LabelWeight) AS LabelLb
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

/// Today's orders, each with its due time (the order's estimated production
/// time, as seconds since the production day began), cases ordered and cases
/// built so far. Nexus order status is not kept up to date, so progress comes
/// from the pallets.
pub const ORDERS: &str = r#"
SELECT
    o.LocalOrderID AS OrderNo,
    c.CustomerName AS Customer,
    o.IsTransferOrder AS Transfer,
    DATEDIFF(SECOND, __DAY_START__, o.EstimatedProductionDate) AS DueSecs,
    SUM(li.CasesOrdered) AS Ordered,
    ISNULL((
        SELECT SUM(pc.CaseCount)
        FROM dbo.PalletContents pc
        JOIN dbo.OrderLineItems l2
            ON l2.LineItemID = pc.LineItemID
        WHERE l2.OrderID = o.OrderID
    ), 0) AS Built
FROM dbo.Orders o
JOIN dbo.OrderLineItems li
    ON li.OrderID = o.OrderID
LEFT JOIN dbo.Customers c
    ON c.CustomerID = o.CustomerID
WHERE CAST(o.EstimatedProductionDate AS DATE) = __PROD_DATE__
GROUP BY o.OrderID, o.LocalOrderID, c.CustomerName, o.IsTransferOrder, o.EstimatedProductionDate
HAVING SUM(li.CasesOrdered) > 0
ORDER BY o.EstimatedProductionDate, o.LocalOrderID
"#;

/// Pallets not shipped and not available for use, by the hold they are on.
/// Usually empty.
pub const HOLDS: &str = r#"
SELECT
    ps.StatusName AS Hold,
    COUNT(DISTINCT p.PalletID) AS Pallets,
    ISNULL(SUM(pc.CaseCount), 0) AS Cases,
    CONVERT(VARCHAR(16), MIN(p.Created), 120) AS Since
FROM dbo.Pallets p
JOIN dbo.PalletStatus ps
    ON ps.StatusID = p.StatusID
LEFT JOIN dbo.PalletContents pc
    ON pc.PalletID = p.PalletID
WHERE p.StatusID <> 1
    AND p.IsShipped = 0
    AND p.IsActive = 1
GROUP BY ps.StatusName
ORDER BY Pallets DESC
"#;

/// Every pallet built in the production day, whether it is for an order or for
/// inventory. History only has pallets for today's orders; this is what shows
/// whether the station is busy.
pub const ACTIVITY: &str = r#"
SELECT
    p.PalletID AS PalletID,
    DATEDIFF(SECOND, __DAY_START__, p.Created) AS SecsIntoDay,
    CASE WHEN p.OrderID IS NULL THEN 0 ELSE 1 END AS ForOrder
FROM dbo.Pallets p
WHERE p.Created >= __DAY_START__
ORDER BY p.Created
"#;

/// Every pallet built in the production day, one row per pallet and product,
/// with its weights and how long its cases took. FillSecs runs from the first
/// case record on the pallet to the last, which is how long the pallet's cases
/// took to come off the line; LagSecs is from the last case to the pallet being
/// created. Both are NULL if the pallet has no case records.
pub const PALLETS: &str = r#"
SELECT
    p.PalletID AS PalletID,
    DATEDIFF(SECOND, __DAY_START__, p.Created) AS SecsIntoDay,
    pr.LocalMaterialID AS ProductCode,
    CASE WHEN p.OrderID IS NULL THEN 0 ELSE 1 END AS ForOrder,
    SUM(pc.CaseCount) AS Cases,
    SUM(pc.NetWeight) AS NetLb,
    SUM(pc.LabelWeight) AS LabelLb,
    (SELECT DATEDIFF(SECOND, MIN(cm.Created), MAX(cm.Created))
        FROM dbo.CaseManifest cm WHERE cm.PalletID = p.PalletID) AS FillSecs,
    (SELECT DATEDIFF(SECOND, MAX(cm.Created), p.Created)
        FROM dbo.CaseManifest cm WHERE cm.PalletID = p.PalletID) AS LagSecs
FROM dbo.Pallets p
JOIN dbo.PalletContents pc
    ON pc.PalletID = p.PalletID
JOIN dbo.Products pr
    ON pr.ProductID = pc.ProductID
WHERE p.Created >= __DAY_START__
GROUP BY p.PalletID, p.Created, p.OrderID, pr.LocalMaterialID
ORDER BY p.Created
"#;

/// The orders of the last 14 production days before today, each with cases
/// ordered and built, to show how often orders close short and for whom.
pub const PAST_ORDERS: &str = r#"
SELECT
    CONVERT(VARCHAR(10), o.EstimatedProductionDate, 120) AS ProdDate,
    o.LocalOrderID AS OrderNo,
    c.CustomerName AS Customer,
    o.IsTransferOrder AS Transfer,
    x.ordered AS Ordered,
    b.built AS Built
FROM dbo.Orders o
CROSS APPLY (
    SELECT SUM(li.CasesOrdered) AS ordered
    FROM dbo.OrderLineItems li
    WHERE li.OrderID = o.OrderID
) x
CROSS APPLY (
    SELECT ISNULL(SUM(pc.CaseCount), 0) AS built
    FROM dbo.PalletContents pc
    JOIN dbo.OrderLineItems l2
        ON l2.LineItemID = pc.LineItemID
    WHERE l2.OrderID = o.OrderID
) b
LEFT JOIN dbo.Customers c
    ON c.CustomerID = o.CustomerID
WHERE CAST(o.EstimatedProductionDate AS DATE) < __PROD_DATE__
    AND CAST(o.EstimatedProductionDate AS DATE) >= DATEADD(DAY, -14, __PROD_DATE__)
    AND x.ordered > 0
ORDER BY o.EstimatedProductionDate, o.LocalOrderID
"#;

pub struct Queries {
    pub production: String,
    pub live: String,
    pub history: String,
    pub orders: String,
    pub holds: String,
    pub activity: String,
    pub pallets: String,
    pub past_orders: String,
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
        Self {
            production: fill(PRODUCTION),
            live: fill(LIVE),
            history: fill(HISTORY),
            orders: fill(ORDERS),
            holds: fill(HOLDS),
            activity: fill(ACTIVITY),
            pallets: fill(PALLETS),
            past_orders: fill(PAST_ORDERS),
        }
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
        assert!(!q.orders.contains("__"));
        assert!(!q.holds.contains("__"));
        assert!(!q.activity.contains("__"));
        assert!(!q.pallets.contains("__"));
        assert!(!q.past_orders.contains("__"));
    }

    #[test]
    fn a_midnight_start_is_the_calendar_day() {
        let q = Queries::new(0);
        assert!(q.production.contains("DATEADD(MINUTE, 0, GETDATE())"));
    }
}
