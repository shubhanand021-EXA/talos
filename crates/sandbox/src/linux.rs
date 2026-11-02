//! Linux backend: bubblewrap namespaces + prlimit resource bounds.
//!
//! Confinement (all real, all testable):
//!
//! - Filesystem: the whole host root is read-only (`--ro-bind / /`), then
//!   the workspace is bound read-write (canonicalized, tmpfs-before-bind
//!   ordering), `/tmp` is a tmpfs, `/proc` + `/dev` are fresh mounts.
//!   Nothing outside the workspace is writable.
//! - Network: `--unshare-net` when `network_allowed` is false. When true,
//!   the host network is shared and egress is policy-allowlisted only
//!   (bubblewrap alone cannot filter hosts; the receipt records this).
//! - Resources: `prlimit` prefix — `RLIMIT_CPU` bounded by the timeout,
//!   `RLIMIT_AS` from `memory_limit_mb`, `RLIMIT_NPROC` anti-fork-bomb cap;
//!   best-effort cgroup v2 clamp (`cpu.max`, `memory.max`, `pids.max`) when
//!   a writable cgroup exists (`TALOS_CGROUP_ROOT` overrides the mount for
//!   tests), otherwise noted in the enforcement method.
//!
//! Fail closed: when `bwrap` is missing (override via `TALOS_BWRAP_BIN` for
//! tests), availability is false and execution is denied.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::enforce::PostSpawnGuard;
use crate::profile::SandboxConfig;
use crate::{err, Result};

/// Override for the bubblewrap binary (tests point it at nothing).
pub const BWRAP_BIN_ENV: &str = "TALOS_BWRAP_BIN";
/// Override for the cgroup v2 mount (tests point it at a temp dir).
pub const CGROUP_ROOT_ENV: &str = "TALOS_CGROUP_ROOT";
/// Default bubblewrap binary.
pub const BWRAP_BIN: &str = "bwrap";
/// Anti-fork-bomb process cap applied via `RLIMIT_NPROC`.
pub const NPROC_CAP: u64 = 128;

/// Linux bubblewrap backend.
#[derive(Debug, Default)]
pub struct BubblewrapSandbox;

impl BubblewrapSandbox {
    /// Create the backend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Bubblewrap binary (env override for tests).
    fn bwrap_bin() -> String {
        std::env::var(BWRAP_BIN_ENV).unwrap_or_else(|_| BWRAP_BIN.to_owned())
    }

    /// True when `bwrap --version` succeeds.
    fn bwrap_present() -> bool {
        std::process::Command::new(Self::bwrap_bin())
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// True when the `prlimit` binary resolves on `PATH`.
    fn prlimit_present() -> bool {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .map(|dir| dir.join("prlimit"))
            .any(|candidate| candidate.is_file())
    }

    /// Writable cgroup v2 mount, if any (`TALOS_CGROUP_ROOT` wins for tests).
    fn cgroup_root() -> Option<PathBuf> {
        if let Some(root) = std::env::var_os(CGROUP_ROOT_ENV) {
            let root = PathBuf::from(root);
            if root.is_dir() {
                return Some(root);
            }
            return None;
        }
        let root = Path::new("/sys/fs/cgroup");
        if root.join("cgroup.controllers").is_file() {
            return Some(root.to_path_buf());
        }
        None
    }

    /// Toolchain roots to read-only bind (host paths that exist).
    fn ro_roots(config: &SandboxConfig) -> Vec<PathBuf> {
        let mut roots = vec![
            PathBuf::from("/usr"),
            PathBuf::from("/bin"),
            PathBuf::from("/lib"),
            PathBuf::from("/lib64"),
            PathBuf::from("/etc"),
        ];
        for home in [
            std::env::var_os("CARGO_HOME"),
            std::env::var_os("RUSTUP_HOME"),
        ]
        .into_iter()
        .flatten()
        {
            roots.push(PathBuf::from(home));
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push(home.join(".cargo"));
            roots.push(home.join(".rustup"));
        }
        roots
            .into_iter()
            .filter(|root| root.exists() && *root != config.workspace_root)
            .collect()
    }
}

/// Build the `bwrap` argv prefix for a config (pure; unit-tested).
///
/// Order matters: the read-only root comes first, then `/proc`, `/dev`,
/// and `--tmpfs /tmp` precede the workspace bind so a workspace under
/// `/tmp` stays visible and nothing outside the workspace is writable.
#[must_use]
pub fn bwrap_argv(config: &SandboxConfig, workspace: &Path) -> Vec<String> {
    let mut argv = vec![
        "--die-with-parent".to_owned(),
        "--new-session".to_owned(),
        "--unshare-pid".to_owned(),
    ];
    if !config.network_allowed {
        argv.push("--unshare-net".to_owned());
    }
    // Whole host root read-only first: without this, paths outside the
    // explicit binds (e.g. /root, /home, /var) would stay writable.
    argv.push("--ro-bind".to_owned());
    argv.push("/".to_owned());
    argv.push("/".to_owned());
    for root in BubblewrapSandbox::ro_roots(config) {
        let root = root.to_string_lossy().into_owned();
        argv.push("--ro-bind".to_owned());
        argv.push(root.clone());
        argv.push(root);
    }
    argv.push("--proc".to_owned());
    argv.push("/proc".to_owned());
    argv.push("--dev".to_owned());
    argv.push("/dev".to_owned());
    // tmpfs before the workspace bind: a workspace under /tmp must survive it.
    argv.push("--tmpfs".to_owned());
    argv.push("/tmp".to_owned());
    let workspace = workspace.to_string_lossy().into_owned();
    argv.push("--bind".to_owned());
    argv.push(workspace.clone());
    argv.push(workspace.clone());
    argv.push("--chdir".to_owned());
    argv.push(workspace);
    argv
}

/// Build the `prlimit` argv prefix for resource bounds (pure; unit-tested).
///
/// `RLIMIT_CPU` is bounded by the timeout (plus grace); `RLIMIT_AS` only
/// when `memory_limit_mb` is set; `RLIMIT_NPROC` always caps fork bombs.
/// Uses `=` syntax: util-linux resource flags take optional arguments, so
/// space-separated values would be mistaken for the command. Returns an
/// empty prefix when `prlimit` is unavailable.
pub fn prlimit_argv(config: &SandboxConfig) -> Vec<String> {
    if !BubblewrapSandbox::prlimit_present() {
        return Vec::new();
    }
    let cpu_secs = config.timeout_ms.div_ceil(1000).saturating_add(10);
    let mut argv = vec![format!("--cpu={cpu_secs}:{cpu_secs}")];
    if let Some(memory_mb) = config.memory_limit_mb {
        let bytes = u64::from(memory_mb).saturating_mul(1024 * 1024);
        argv.push(format!("--as={bytes}:{bytes}"));
    }
    argv.push(format!("--nproc={NPROC_CAP}:{NPROC_CAP}"));
    argv
}

/// Best-effort cgroup v2 clamp for one pid (pure logic over a root path).
///
/// Writes `cpu.max`, `memory.max`, `pids.max` under `<root>/talos-<pid>`
/// and moves the pid into `cgroup.procs`. Returns the placement on success.
/// Never panics; every failure is an `Err` the caller records honestly.
pub fn place_in_cgroup(root: &Path, pid: u32, config: &SandboxConfig) -> Result<String> {
    use std::fmt::Write as _;
    let group = root.join(format!("talos-{pid}"));
    std::fs::create_dir_all(&group).map_err(|e| err(&format!("cgroup mkdir: {e}")))?;
    let mut applied = vec![format!("pids.max={NPROC_CAP}")];
    std::fs::write(group.join("pids.max"), format!("{NPROC_CAP}"))
        .map_err(|e| err(&format!("cgroup pids.max: {e}")))?;
    if let Some(cpu) = config.cpu_limit_percent {
        // cpu.max: `$MAX $PERIOD` with period 100000µs.
        let quota = u32::from(cpu).saturating_mul(1000);
        std::fs::write(group.join("cpu.max"), format!("{quota} 100000"))
            .map_err(|e| err(&format!("cgroup cpu.max: {e}")))?;
        let mut note = String::new();
        let _ = write!(note, "cpu.max={quota}/100000");
        applied.push(note);
    }
    if let Some(memory_mb) = config.memory_limit_mb {
        let bytes = u64::from(memory_mb).saturating_mul(1024 * 1024);
        std::fs::write(group.join("memory.max"), format!("{bytes}"))
            .map_err(|e| err(&format!("cgroup memory.max: {e}")))?;
        applied.push(format!("memory.max={bytes}"));
    }
    std::fs::write(group.join("cgroup.procs"), format!("{pid}"))
        .map_err(|e| err(&format!("cgroup attach: {e}")))?;
    Ok(format!(
        "{} ({})",
        group.to_string_lossy(),
        applied.join(", ")
    ))
}

impl crate::enforce::SandboxEnforcer for BubblewrapSandbox {
    fn name(&self) -> &'static str {
        "bubblewrap"
    }

    fn enforcement_method(&self, config: &SandboxConfig) -> String {
        let net = if config.network_allowed {
            if config.allowed_hosts.is_empty() {
                "host-network (egress allowlisted at policy layer only)"
            } else {
                "host-network (policy allowlist; bwrap cannot filter hosts)"
            }
        } else {
            "no-network (unshare-net)"
        };
        let limits = if Self::prlimit_present() {
            "rlimit(cpu-by-timeout,as,nproc)"
        } else {
            "timeout-kill only (prlimit unavailable)"
        };
        let cgroup = if Self::cgroup_root().is_some() {
            "cgroup-v2"
        } else {
            "no-cgroup (exact cpu-percent unavailable)"
        };
        format!("bubblewrap+prlimit [{net}; {limits}; {cgroup}]")
    }

    fn is_available(&self) -> bool {
        Self::bwrap_present()
    }

    fn availability_detail(&self) -> String {
        if Self::bwrap_present() {
            let prlimit = if Self::prlimit_present() {
                "prlimit available"
            } else {
                "prlimit missing (resource limits fall back to timeout kill)"
            };
            let cgroup = if Self::cgroup_root().is_some() {
                "writable cgroup v2 present"
            } else {
                "no writable cgroup v2 (exact cpu-percent unavailable)"
            };
            format!("bwrap present; {prlimit}; {cgroup}")
        } else {
            format!(
                "bubblewrap binary {:?} not runnable (install bubblewrap)",
                Self::bwrap_bin()
            )
        }
    }

    fn wrap_command(
        &self,
        config: &SandboxConfig,
        spec: crate::enforce::CommandSpec,
    ) -> Result<tokio::process::Command> {
        if !self.is_available() {
            return Err(err(&format!(
                "bubblewrap unavailable ({}); execution denied (sandbox_unavailable)",
                self.availability_detail()
            )));
        }
        // Canonicalize the workspace so the bind cannot escape via symlink.
        let workspace = config
            .workspace_root
            .canonicalize()
            .map_err(|e| err(&format!("workspace root unavailable: {e}")))?;
        let prlimit_prefix = prlimit_argv(config);
        // prlimit wraps bwrap: `prlimit … -- bwrap … program args`.
        // The `--` separator is required: resource flags take optional
        // arguments, so without it values could be mistaken for the command.
        let (program, mut args): (OsString, Vec<OsString>) = if prlimit_prefix.is_empty() {
            (Self::bwrap_bin().into(), Vec::new())
        } else {
            let mut prefixed: Vec<OsString> = prlimit_prefix.into_iter().map(Into::into).collect();
            prefixed.push("--".into());
            prefixed.push(Self::bwrap_bin().into());
            ("prlimit".into(), prefixed)
        };
        for arg in bwrap_argv(config, &workspace) {
            args.push(arg.into());
        }
        args.push(spec.program);
        args.extend(spec.args);
        Ok(crate::enforce::standard_child(
            program, &args, &spec.envs, &workspace,
        ))
    }

    fn on_spawn(
        &self,
        config: &SandboxConfig,
        child: &tokio::process::Child,
    ) -> Result<PostSpawnGuard> {
        let Some(pid) = child.id() else {
            return Ok(PostSpawnGuard::default());
        };
        let Some(root) = Self::cgroup_root() else {
            return Ok(PostSpawnGuard::default());
        };
        // Best-effort: a failed placement is recorded, never fatal (bwrap +
        // rlimits already confine the child; cgroup is the exact-percent
        // upgrade).
        match place_in_cgroup(&root, pid, config) {
            Ok(placement) => Ok(PostSpawnGuard {
                placement: Some(placement),
                #[cfg(windows)]
                handle: None,
            }),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enforce::SandboxEnforcer;

    fn config() -> SandboxConfig {
        SandboxConfig {
            workspace_root: std::env::temp_dir(),
            ..SandboxConfig::default()
        }
    }

    #[test]
    fn bwrap_argv_confines_and_orders_mounts() {
        let argv = bwrap_argv(&config(), Path::new("/tmp/ws-1"));
        let ro_root = argv
            .windows(3)
            .position(|w| w == ["--ro-bind", "/", "/"])
            .expect("read-only root bind");
        let tmpfs = argv.iter().position(|a| a == "/tmp").expect("tmpfs");
        let bind = argv
            .iter()
            .position(|a| a == "/tmp/ws-1")
            .expect("workspace bind");
        assert!(ro_root < tmpfs, "read-only root must come first");
        assert!(tmpfs < bind, "tmpfs must precede the workspace bind");
        assert!(argv.contains(&"--unshare-net".to_owned()));
        assert!(argv.contains(&"--die-with-parent".to_owned()));
        assert!(argv.contains(&"--chdir".to_owned()));
    }

    #[test]
    fn bwrap_argv_shares_net_only_when_allowed() {
        let mut allowed = config();
        allowed.network_allowed = true;
        let argv = bwrap_argv(&allowed, Path::new("/tmp/ws-1"));
        assert!(!argv.contains(&"--unshare-net".to_owned()));
    }

    #[test]
    fn prlimit_argv_bounds_cpu_as_and_nproc() {
        // prlimit may be missing on some dev machines; the builder reports
        // honestly either way.
        let argv = prlimit_argv(&config());
        if BubblewrapSandbox::prlimit_present() {
            assert!(argv.iter().any(|a| a.starts_with("--cpu=")));
            assert!(argv
                .iter()
                .any(|a| a == &format!("--nproc={NPROC_CAP}:{NPROC_CAP}")));
            let mut capped = config();
            capped.memory_limit_mb = Some(512);
            let argv = prlimit_argv(&capped);
            assert!(argv.iter().any(|a| a.starts_with("--as=")));
            assert!(argv
                .iter()
                .any(|a| a == &format!("--as={}:{}", 512 * 1024 * 1024, 512 * 1024 * 1024)));
        } else {
            assert!(argv.is_empty());
        }
    }

    #[test]
    fn cgroup_placement_writes_limits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut limited = config();
        limited.cpu_limit_percent = Some(50);
        limited.memory_limit_mb = Some(256);
        let placement = place_in_cgroup(dir.path(), 4242, &limited).expect("placement");
        assert!(placement.contains("talos-4242"));
        let group = dir.path().join("talos-4242");
        assert_eq!(
            std::fs::read_to_string(group.join("cpu.max")).expect("cpu.max"),
            "50000 100000"
        );
        assert_eq!(
            std::fs::read_to_string(group.join("memory.max")).expect("memory.max"),
            format!("{}", 256 * 1024 * 1024)
        );
        assert_eq!(
            std::fs::read_to_string(group.join("pids.max")).expect("pids.max"),
            format!("{NPROC_CAP}")
        );
        assert_eq!(
            std::fs::read_to_string(group.join("cgroup.procs")).expect("procs"),
            "4242"
        );
    }

    #[test]
    fn missing_backend_is_detected() {
        std::env::set_var(BWRAP_BIN_ENV, "/nonexistent/bwrap-for-tests");
        let backend = BubblewrapSandbox::new();
        assert!(!backend.is_available());
        assert!(backend.availability_detail().contains("not runnable"));
        std::env::remove_var(BWRAP_BIN_ENV);
    }

    #[test]
    fn profile_is_part_of_receipt_method() {
        let backend = BubblewrapSandbox::new();
        let method = backend.enforcement_method(&config());
        assert!(
            method.contains("no-network"),
            "default denies network: {method}"
        );
    }
}
