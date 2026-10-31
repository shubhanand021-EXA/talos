# policy_broker

Talos Policy Broker: the security and orchestration authority. No agent action with side effects bypasses policy evaluation.

## Rules (baseline, default deny)

1. Malformed identifiers → `deny`
2. `irreversible` risk → `require_compensation`
3. High-risk + non-reversible → `require_human_review`
4. Write-like (`write_file`, `execute_command`, `system_control`) → `require_evidence`
5. Low-risk read (`read_file`, `open_browser`) → `allow`
6. Anything else → `deny`

Unknown capability strings fail deserialization; `evaluate_request_json` turns that into a deny-evaluation.

## Run

```sh
cargo test -p policy_broker
cargo run -p policy_broker --bin talos-policy-broker -- --selftest
```
