# memory_indexer

Ledger → vector recall with hybrid retrieval and skill distillation.

```sh
cargo test -p memory_indexer
```

- `index_incremental` embeds new/changed claims, evidence, completed tasks,
  and skills (content-hash dedupe); proposes quarantined skills from
  completed tasks with verified claims.
- `rebuild_index` clears vectors + state, then reindexes everything.
- `hybrid_search` (scope filter → lexical + semantic → normalized blend →
  recency/verified rerank → content-hash dedupe → top-K) with
  `RetrievalMode::{Lexical, Semantic, Hybrid}`.
- `spawn_background_indexer` ticks incremental indexing on an interval.
- `propose_skill_from_task` distills one quarantined skill per completed
  task (deduped by source task).
