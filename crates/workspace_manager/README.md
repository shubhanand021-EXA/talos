# workspace_manager

Per-task isolated coding workspaces: the only filesystem root a task's agent
may touch.

```sh
cargo test -p workspace_manager
```

## Model

- `WorkspaceManager::new(base_dir)` owns all workspaces under one base dir
  (`<temp>/talos-workspaces` by default).
- `create_workspace(task_id, repo_hint)`:
  - When `repo_hint` points inside a git repo, a linked worktree is added
    (`git worktree add <base>/<task_id> -b talos/<task_id>`); the parent repo
    is never written to directly — all git operations stay scoped to the
    worktree root.
  - Otherwise an isolated directory is created.
  - Idempotent: an existing dir is reused.
- `cleanup(task_id)` unregisters the worktree (`worktree remove --force`)
  first, then removes the directory; missing dirs are OK.
- Task ids are validated (1..=128 chars, no path separators); branch names
  are sanitized to `[A-Za-z0-9_-]` (max 64 chars).

## Safety

- This crate only manages directories. Confinement is enforced by
  `tool_runtime` sandbox checks (`check_relative`, `contained_path`) on top
  of these paths, and every file/shell/git action is policy-gated.
- Git dir overrides (`--git-dir`, `--work-tree`, `-C`) are rejected by the
  `git.*` tools, so operations cannot escape the workspace.
