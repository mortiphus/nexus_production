//! The two read-only queries, copied from the Rosehill production dashboard.
//! The only change is the scan time, which is formatted by the database
//! (HH:MM:SS) so no date type needs decoding here.

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
    WHERE CAST(o.EstimatedProductionDate AS DATE) = CAST(GETDATE() AS DATE)
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
    WHERE CAST(o.EstimatedProductionDate AS DATE) = CAST(GETDATE() AS DATE)
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
    WHERE CAST(p.LastModified AS DATE) = CAST(GETDATE() AS DATE)
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

/// Cases built into pallets for today's orders, one row per pallet, with the
/// time of its last change as seconds since midnight (negative: before
/// today). It counts the same cases as PRODUCTION's CasesCompleted, so the
/// running total ends where that figure does.
pub const HISTORY: &str = r#"
SELECT
    DATEDIFF(SECOND, CAST(GETDATE() AS DATE), p.LastModified) AS SecsIntoDay,
    SUM(pc.CaseCount) AS Cases
FROM dbo.Pallets p
JOIN dbo.PalletContents pc
    ON pc.PalletID = p.PalletID
JOIN dbo.OrderLineItems li
    ON li.LineItemID = pc.LineItemID
JOIN dbo.Orders o
    ON o.OrderID = li.OrderID
WHERE CAST(o.EstimatedProductionDate AS DATE) = CAST(GETDATE() AS DATE)
GROUP BY p.PalletID, p.LastModified
ORDER BY p.LastModified
"#;
