# agent_runtime

Deterministic in-memory orchestration for the Talos local control plane. No network, no LLMs: `LocalOrchestrator` runs one diagnostic loop (create task → spawn `MockEchoAgent` → propose action → policy verdict → record evidence → complete) and records ordered `RuntimeEvent`s for the Tauri layer to emit.

```sh
cargo test -p agent_runtime
```
