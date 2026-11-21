# Single-node acceptance cost

This cost is computed from configuration by `scripts/production/cost_model.py`, not from a live run. No paid run was started. The rates are the operator's rough assumptions: the node rate is the same assumed 0.78 USD/h as the 0.2.0 page, and it is not a quote. The inputs are in `scripts/production/cost-inputs.single-node.json`. The owner approves the TWD figure before any paid run.

Command:

```text
python3 -B scripts/production/cost_model.py --inputs scripts/production/cost-inputs.single-node.json
```

Output:

| Item | USD | Basis |
|---|---:|---|
| nodes | 3.71 | 1 nodes x 0.78 USD/h x 4.75 h |
| driver | 0.71 | 1 driver x 0.15 USD/h x 4.75 h |
| store_pvc | 0.27 | 242 GiB x 1 replicas x 0.17 USD/GiB-month |
| control_pvc | 0.01 | 5 GiB x 1 replicas x 0.17 USD/GiB-month |
| gcs | 0.00 | 1 GiB x 0.026 USD/GiB-month |
| egress | 0.12 | 1 GiB x 0.12 USD/GiB |
| uptime_check | 1.00 | fixed uptime check |
| contingency | 1.16 | 0.2 of the lines above |
| Total | 6.98 | |

Figure for approval: USD 6.98 (TWD 225), cap TWD 300, over cap: no.

Node hours: 4.75 (one node). PVC size: 242 GiB for the store plus 5 GiB for control. Total: USD 6.98, TWD 225. The phase retention (G2, 0.25 h) is part of these hours. The owner approves the new TWD figure before the next paid run.

The figure is under the approved cap of TWD 300: over cap, no. The numbers are not changed to fit the cap.

## Config values

These are the values that `prepare.py` reads for the budget (`--format json`):

- `cost_cap_usd`: 7
- `estimated_hourly_cost_usd`: 0.93
- `estimated_fixed_cost_usd`: 0
- `store_pvc_gib`: 242
- `control_pvc_gib`: 5

## Needs one measurement

- `measured_store_bytes_per_item` (assumed 1000; not measured)
- `node_usd_per_hour` (assumed 0.78; a rough rate, not a quote)
- `rated_items_per_second` (assumed 50 items per second for a small team)
