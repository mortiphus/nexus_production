# nexus_production

Today's production and live pallet scanning for the production line monitoring system. A small Rust service for a Raspberry Pi that reads the Nexus SQL Server database and writes JSON files. The [`websocket_server`](https://github.com/mortiphus/websocket_server) dashboard reads those files and shows them under Second Processing.

It is a standalone port of the two queries in the Rosehill production dashboard, written in the same shape as [`bpm`](https://github.com/mortiphus/bpm).

## How It Works

## The production day and the shifts

"Today" is the production day, not the calendar date. It starts when the night shift does (11 PM) and is named for the morning it ends in, so pallets built from 11 PM Monday are for Tuesday's orders. This matches Nexus: pallets for an order dated Tuesday are created from Monday 11 PM until about Tuesday 4 PM, with some built a day or more earlier. Nothing resets at midnight.

- Night shift: `night_start` (23:00) to `day_start` (08:00).
- Day shift: `day_start` until the next `night_start`.
- Pallets built before the production day started are tagged `pre` ("built ahead").
- Build time is the pallet's `Created` time. `LastModified` also moves when a pallet is edited or touched later, sometimes hours after it was built.

Every file also carries `interval_secs`, how often it is rewritten, so the dashboard can tell a slow feed from a stalled one.

Each history point carries `shift` (`pre`, `night` or `day`), and its `secs` count from the start of the production day. The file also holds `day_start` (when the production day starts, in seconds after midnight) and `day_shift_at` (when the day shift starts, in seconds into the production day) so the dashboard can turn them into clock times.

## The feeds

- **production.json** (every 20 s): for each product ordered for today, cases ordered, completed and remaining, and pallets remaining. Combos, bulk, frames and backs count one per pallet; everything else is 50 cases to a pallet. Each product is also tagged `is_combo` when fewer than `combo_below_cases` (22) of it are ordered today.
- **history.json** (every 30 s): one point per pallet and product built for today's orders, with the pallet, the product, when it last changed, its cases and whether they are combos. The dashboard turns these into the cases-completed-through-the-day chart. Pallets built on an earlier day for today's orders come through as negative times and make the chart's starting level.
- **live.json** (every 10 s): the last five pallets built in the production day, newest first by when they were created, with order, customer, product, cases on the pallet and cases remaining on the order. The Rosehill query sorted by `LastModified`, which also moves when a pallet is edited or touched later, so it showed pallets that were not new.
- **orders.json** (every 30 s): each order for the production day with its due time (the order's estimated production time), cases ordered and cases built. Nexus order status is not kept up to date (everything stays "Not Started"), so progress comes from the pallets.
- **holds.json** (every 60 s): unshipped pallets on a quality hold (foreign material, metal detection, allergen and so on). Usually empty.
- **pallets.json** (every 60 s): each pallet built in the production day with its product, cases, net and label weight, and two times from the case records: the fill time (first case on the pallet to the last, how long its cases took to come off the line) and the lag (last case to the pallet being created). Used for pallet weight and fill-time figures.
- **past_orders.json** (every 5 min): the orders of the last 14 production days with cases ordered and built, for how often orders close short and for which customers.
- **activity.json** (every 30 s): every pallet built in the production day, for an order or for inventory, with its shift. History only has pallets for today's orders; this is what the dashboard uses for "last pallet" and gaps.
- It only runs `SELECT` queries. Use a read-only database login.
- Each file holds `ok`, `error`, `updated`, `updated_epoch` and `data`. If the database cannot be reached, `data` keeps the last good result and `ok` goes false with the reason, so the dashboard can say the figures are old.
- Files are written whole and renamed into place, so the dashboard never reads half a file.
- A change in database health is printed to the journal; a repeated failure is not repeated.

## Settings

`/home/pi/nexus.toml` (or `NEXUS_CONFIG`). See `nexus.toml.example` for every key, including `[shifts]`. The database password can instead be set in the `NEXUS_DB_PASSWORD` environment variable.

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
