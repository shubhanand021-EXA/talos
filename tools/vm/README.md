# Ephemeral VM backend (`tools/vm/`)

This directory holds the container contract for the Talos computer-use
ephemeral VM backend (`crates/computer_use/src/vm.rs`):

- `Dockerfile` — Alpine + XFCE + x11vnc + xdotool + ImageMagick. The agent
  needs exactly three guest affordances: `import -window root png:-`
  (screenshots), `xdotool` (input), and a writable `/home/agent/output`
  (export source).

## Runtime matrix

| Host    | How Docker runs                                     |
| ------- | --------------------------------------------------- |
| Linux   | Native Docker Engine (or rootless); no extra setup. |
| Windows | Docker Desktop (WSL2 backend), Linux containers.    |
| macOS   | Docker Desktop, **or** a Lima-managed Docker host:  |

```sh
# macOS without Docker Desktop: Lima provides the Docker daemon.
limactl start template://docker
docker context create lima --docker "host=unix://$HOME/.lima/docker/sock/docker.sock"
docker context use lima
```

The Talos manager shells out to the `docker` CLI (`TALOS_DOCKER_BIN`
override) — any daemon the CLI can reach works, including a remote
`DOCKER_HOST`. No Lima-specific code paths exist by design.

## Build and probe

```sh
docker build -t talos/desktop-vm:0.1 tools/vm
docker run -d --rm --cap-drop=ALL --network none \
  -v /tmp/probe:/workspace:ro talos/desktop-vm:0.1
```

Talos overrides the image with `TALOS_VM_IMAGE`. The manager probes
`docker info` before every spawn and fails closed (`vm_unavailable`) when
the daemon is unreachable — CI never touches Docker (all lifecycle tests
run against `MockTransport`).

## Security posture (guest contract)

- The agent user is non-root; the writable layer dies with the container
  (`--rm`); nothing persists between tasks.
- `/workspace` mounts `:ro` (verified by `filesystem_isolation_holds`).
- `--network none` default; VNC publishes `127.0.0.1::5900` only when the
  session opts into `vncExpose` (human viewing).
- `--cap-drop=ALL`, `--security-opt=no-new-privileges`, `--pids-limit=256`,
  memory/CPU caps on every spawn.
- Outbound artifacts leave ONLY via `computer.export_file`
  (`/home/agent/output` → task-root `vm-exports/`, hashed, size-capped).
