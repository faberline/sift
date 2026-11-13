# Production acceptance cost (0.2.0)

This cost is computed from configuration by `scripts/production/cost_model.py`, not from a live run. No paid run was started. The rates are the operator's rough assumptions of 2026-11-13, and the inputs are in `scripts/production/cost-inputs.020.json`.

Command:

```text
python3 -B scripts/production/cost_model.py --inputs scripts/production/cost-inputs.020.json
```

Output:

| Item | USD | Basis |
|---|---:|---|
| nodes | 63.18 | 3 nodes x 0.78 USD/h x 27.0 h |
| driver | 4.05 | 1 driver x 0.15 USD/h x 27.0 h |
| store_pvc | 923.36 | 48280 GiB x 3 replicas x 0.17 USD/GiB-month |
| control_pvc | 0.10 | 5 GiB x 3 replicas x 0.17 USD/GiB-month |
| gcs | 0.05 | 50 GiB x 0.026 USD/GiB-month |
| egress | 1.20 | 10 GiB x 0.12 USD/GiB |
| uptime_check | 1.00 | fixed uptime check |
| contingency | 198.59 | 0.2 of the lines above |
| Total | 1191.52 | |

Figure for approval: USD 1191.52 (TWD 38436), cap TWD 300, over cap: yes.

The store PVC dominates the cost until the real bytes per item are measured.

The figure is above the approved cap of TWD 300: over cap, yes. The numbers are not lowered to fit the cap.

## Config values

These are the values that `prepare.py` reads for the budget (`--format json`):

- `cost_cap_usd`: 1192
- `estimated_hourly_cost_usd`: 2.49
- `estimated_fixed_cost_usd`: 1129.2652500000004
- `store_pvc_gib`: 48280
- `control_pvc_gib`: 5

## Needs one measurement

- `measured_store_bytes_per_item` (assumed 1000; not measured)
- `node_usd_per_hour` (assumed 0.78; a rough rate, not a quote)
