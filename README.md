# nexus_production

Today's production and live pallet scanning for the production line monitoring system. A small Rust service for a Raspberry Pi that reads the Nexus SQL Server database and writes three JSON files. The [`websocket_server`](https://github.com/mortiphus/websocket_server) dashboard reads those files and shows them under Second Processing.

It is a standalone port of the two queries in the Rosehill production dashboard, written in the same shape as [`bpm`](https://github.com/mortiphus/bpm).

## How It Works

- **production.json** (every 20 s): for each product ordered for today, cases ordered, completed and remaining, and pallets remaining. Combos, bulk, frames and backs count one per pallet; everything else is 50 cases to a pallet. Each product is also tagged `is_combo` when fewer than `combo_below_cases` (22) of it are ordered today.
- **history.json** (every 30 s): one point per pallet and product built for today's orders, with the pallet, the product, when it last changed, its cases and whether they are combos. The dashboard turns these into the cases-completed-through-the-day chart. Pallets built on an earlier day for today's orders come through as negative times and make the chart's starting level.
- **live.json** (every 10 s): the last five pallets scanned today, with order, customer, product, cases on the pallet and cases remaining on the order.
- It only runs `SELECT` queries. Use a read-only database login.
- Each file holds `ok`, `error`, `updated`, `updated_epoch` and `data`. If the database cannot be reached, `data` keeps the last good result and `ok` goes false with the reason, so the dashboard can say the figures are old.
- Files are written whole and renamed into place, so the dashboard never reads half a file.
- A change in database health is printed to the journal; a repeated failure is not repeated.

## Settings

`/home/pi/nexus.toml` (or `NEXUS_CONFIG`). See `nexus.toml.example` for every key. The database password can instead be set in the `NEXUS_DB_PASSWORD` environment variable.

It has its own file, not `dashboard.toml`, because saving the dashboard's setup page rewrites `dashboard.toml` and drops sections it does not know, and this file holds a database login.

## Install

```bash
git clone <repo> /home/pi/Git/nexus_production
cd /home/pi/Git/nexus_production
cp nexus.toml.example /home/pi/nexus.toml   # then set the password
cargo build --release
sudo cp nexus_production.service /etc/systemd/system/
sudo systemctl enable --now nexus_production
```

The Pi must be able to reach the database server (`192.168.1.12` by default). If SQL Server Browser is blocked, remove `instance` from `nexus.toml` and set `port`.
