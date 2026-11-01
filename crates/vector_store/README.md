# vector_store

SQLite-backed vector memory for Talos semantic recall.

```sh
cargo test -p vector_store
```

- Tables live in the shared `EvidenceDb`: `vector_entries`
  (`{source_type}:{source_id}`, text, 384-dim `f32` blob, content hash,
  verified flag, task link, metadata) + `memory_index_state` (change
  detection so reindexing unchanged rows is a no-op).
- `upsert_entry` embeds + stores (skips unchanged content);
  `search` filters first (source type, task, date range, verified-only),
  then brute-force cosine in Rust, score desc, id asc.
- `remove_by_source`, `counts_by_type`, `count`, `last_indexed_at`,
  `clear_all`. At <10k entries top-10 search answers in well under 100ms.
