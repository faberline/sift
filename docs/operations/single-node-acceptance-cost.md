# Single-node acceptance cost

This cost is computed from configuration by `scripts/production/cost_model.py`, not from a live run. No paid run was started. The rates are the operator's rough assumptions: the node rate is the same assumed 0.78 USD/h as the 0.2.0 page, and it is not a quote. The inputs are in `scripts/production/cost-inputs.single-node.json`. The owner approves the TWD figure before any paid run.

Command:

```text
python3 -B scripts/production/cost_model.py --inputs scripts/production/cost-inputs.single-node.json
```

Output:

| Item | USD | Basis |
|---|---:|---|
| nodes | 3.86 | 1 nodes x 0.78 USD/h x 4.949999999999999 h |
| driver | 0.74 | 1 driver x 0.15 USD/h x 4.949999999999999 h |
| store_pvc | 0.28 | 242 GiB x 1 replicas x 0.17 USD/GiB-month |
| control_pvc | 0.01 | 5 GiB x 1 replicas x 0.17 USD/GiB-month |
| gcs | 0.00 | 1 GiB x 0.026 USD/GiB-month |
| egress | 0.12 | 1 GiB x 0.12 USD/GiB |
| uptime_check | 1.00 | fixed uptime check |
| contingency | 1.20 | 0.2 of the lines above |
| Total | 7.21 | |

Figure for approval: USD 7.21 (TWD 233), cap TWD 300, over cap: no.

Node hours: 4.95 (one node). PVC size: 242 GiB for the store plus 5 GiB for control. Total: USD 7.21, TWD 233. The phase install_one_namespace (G1, 0.1 h), the phase retention (G2, 0.25 h) and the phase deleted_pod_logs (G3, 0.1 h) are part of these hours. The owner approves the new TWD figure before the next paid run.

The phase name load_24h is only a name. In this profile the phase load_24h runs 7080 seconds (2.0 hours of budget minus 120 seconds of headroom, from load_seconds in scripts/single_node/acceptance/phases.py) at 50 items per second, as in run 44740023742. It does not run 24 hours. Other workflows and the production scripts use the name, so it is not renamed.

The figure is under the approved cap of TWD 300: over cap, no. The numbers are not changed to fit the cap.

## Config values

These are the values that `prepare.py` reads for the budget (`--format json`):

- `cost_cap_usd`: 8
- `estimated_hourly_cost_usd`: 0.93
- `estimated_fixed_cost_usd`: 0
- `store_pvc_gib`: 242
- `control_pvc_gib`: 5

## Needs one measurement

- `measured_store_bytes_per_item` (assumed 1000; not measured)
- `node_usd_per_hour` (assumed 0.78; a rough rate, not a quote)
- `rated_items_per_second` (assumed 50 items per second for a small team)

## Three-phase run (estimate)

This estimate is hand-computed from the same inputs in `scripts/production/cost-inputs.single-node.json`. `cost_model.py` needs the five load phases, so it cannot compute a run of three phases. It is an estimate, not a quote.

Run input: `phases=install_one_namespace,retention,deleted_pod_logs`.

Pool `faberline.dev/pool=sift-server`, 1 node, 1 driver.

Minutes: setup 60 (the budget in `cost-inputs.single-node.json`), install_one_namespace 6, retention 15, deleted_pod_logs 6. Total about 87 min (1.45 h). The three phases alone are 27 min (0.45 h).

Estimated cost: USD 3.06 = TWD 99 (nodes 1.13, driver 0.22, store PVC 0.08, egress 0.12, uptime check 1.00, contingency 0.51). A full run is USD 7.21 = TWD 233 (section above).

The desk asks the owner with this number (faberline/aw#722). A partial run is not a full acceptance: its evidence is marked `partial`, and the receipt script refuses it.
