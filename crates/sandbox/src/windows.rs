//! Windows backend: Job Objects for CPU/memory/process limits.
//!
//! Enforcement (all real via the `windows` crate's safe wrappers):
//!
//! - Process count: `ActiveProcessLimit` (anti-fork-bomb).
//! - Memory: `ProcessMemoryLimit` + `JobMemoryLimit` from `memory_limit_mb`.
//! - CPU: `JOB_OBJECT_CPU_RATE_CONTROL` hard cap from `cpu_limit_percent`.
//! - Timeout: the existing timeout kill machinery (unchanged).
//!
//! The job is assigned post-spawn and the handle is held until the child is
//! reaped ([`PostSpawnGuard`]). Honest limitations (documented): a Job
//! Object cannot isolate the filesystem — full fs isolation on Windows
//! requires containers. Workspace confinement (cwd + symlink-escape
//! rejection) stays in `tool_runtime`; network egress has no per-process
//! control here, so `network_allowed: false` is enforced at the policy
//! layer and recorded in the receipt.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectCpuRateControlInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_CPU_RATE_CONTROL_ENABLE,
    JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
};

use crate::enforce::PostSpawnGuard;
use crate::profile::SandboxConfig;
use crate::{err, Result};

/// Default anti-fork-bomb process cap for the job.
pub const JOB_PROCESS_CAP: u32 = 64;

/// Resource limits derived from a [`SandboxConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobLimits {
    /// Max processes in the job.
    pub active_process_limit: u32,
    /// Per-process memory cap in bytes (`None` = unset).
    pub process_memory_limit: Option<usize>,
    /// Whole-job memory cap in bytes (`None` = unset).
    pub job_memory_limit: Option<usize>,
    /// CPU rate in hundredths of a percent, 100..=10000 (`None` = unset).
    pub cpu_rate: Option<u32>,
}

impl JobLimits {
    /// Derive job limits from a sandbox config.
    pub fn from_config(config: &SandboxConfig) -> Self {
        let memory = config
            .memory_limit_mb
            .map(|mb| u64::from(mb).saturating_mul(1024 * 1024) as usize);
        Self {
            active_process_limit: JOB_PROCESS_CAP,
            process_memory_limit: memory,
            job_memory_limit: memory,
            cpu_rate: config
                .cpu_limit_percent
                .map(|p| u32::from(p).saturating_mul(100)),
        }
    }
}

/// Create a limited job object and return its raw handle value.
pub fn create_limited_job(limits: &JobLimits) -> Result<isize> {
    // SAFETY: `CreateJobObjectW(None, None)` takes no pointers; the
    // returned handle is owned by the caller and closed exactly once.
    let job = unsafe { CreateJobObjectW(None, None) }
        .map_err(|e| err(&format!("CreateJobObjectW: {e}")))?;
    let mut flags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    if limits.process_memory_limit.is_some() || limits.job_memory_limit.is_some() {
        flags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_JOB_MEMORY;
    }
    let extended = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: flags,
            ActiveProcessLimit: limits.active_process_limit,
            ..Default::default()
        },
        ProcessMemoryLimit: limits.process_memory_limit.unwrap_or(0),
        JobMemoryLimit: limits.job_memory_limit.unwrap_or(0),
        ..Default::default()
    };
    // SAFETY: `&extended`/`&cpu` are live stack structs sized exactly as
    // the `cbJobObjectInformationLength` argument claims; the kernel only
    // reads them.
    unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &extended as *const _ as _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .map_err(|e| err(&format!("SetInformationJobObject(limits): {e}")))?;
    if let Some(rate) = limits.cpu_rate {
        let cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
                CpuRate: rate.clamp(100, 10_000),
            },
        };
        // SAFETY: same as above — `&cpu` is a live, correctly-sized struct.
        unsafe {
            SetInformationJobObject(
                job,
                JobObjectCpuRateControlInformation,
                &cpu as *const _ as _,
                std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
            )
        }
        .map_err(|e| err(&format!("SetInformationJobObject(cpu): {e}")))?;
    }
    Ok(job.0 as isize)
}

/// Assign a live child to a limited job; the returned guard owns the job.
pub fn assign_to_job(child: &tokio::process::Child, limits: &JobLimits) -> Result<PostSpawnGuard> {
    let job = create_limited_job(limits)?;
    let Some(handle) = child.raw_handle() else {
        close_job(job);
        return Err(err("spawned child has no OS handle"));
    };
    // SAFETY: `handle` is the live process handle of our own just-spawned
    // child; `AssignProcessToJobObject` only reads it.
    unsafe { AssignProcessToJobObject(HANDLE(job), HANDLE(handle as _)) }.map_err(|e| {
        close_job(job);
        err(&format!("AssignProcessToJobObject: {e}"))
    })?;
    Ok(PostSpawnGuard {
        placement: Some(format!("job-object(handle={job})")),
        handle: Some(job),
    })
}

/// Close a job handle value (no-op on failure).
pub fn close_job(raw: isize) {
    // SAFETY: `raw` is a live Job handle created by `create_limited_job`
    // in this process and closed exactly once (guard `Drop` takes it).
    // `CloseHandle` on an owned handle cannot touch other memory.
    let _ = unsafe { CloseHandle(HANDLE(raw)) };
}

/// Windows Job Object backend.
#[derive(Debug, Default)]
pub struct WindowsSandbox;

impl WindowsSandbox {
    /// Create the backend.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl crate::enforce::SandboxEnforcer for WindowsSandbox {
    fn name(&self) -> &'static str {
        "windows-job-objects"
    }

    fn enforcement_method(&self, config: &SandboxConfig) -> String {
        let net = if config.network_allowed {
            "host-network (egress allowlisted at policy layer only)"
        } else {
            "no per-process egress control on Windows (policy layer denies)"
        };
        format!("windows-job-objects+cwd-confinement [{net}; no fs namespace — containers required for full isolation]")
    }

    fn is_available(&self) -> bool {
        // Job Objects are core Win32, always present. Verify by creating
        // and closing a probe job — failure means fail closed.
        let probe = JobLimits {
            active_process_limit: 1,
            process_memory_limit: None,
            job_memory_limit: None,
            cpu_rate: None,
        };
        match create_limited_job(&probe) {
            Ok(raw) => {
                close_job(raw);
                true
            }
            Err(_) => false,
        }
    }

    fn availability_detail(&self) -> String {
        if self.is_available() {
            "Job Objects available (cpu/memory/process-count limits enforced; no fs namespace — containers required for full isolation)"
                .to_owned()
        } else {
            "Job Object creation failed; execution denied (fail closed)".to_owned()
        }
    }

    fn wrap_command(
        &self,
        config: &SandboxConfig,
        spec: crate::enforce::CommandSpec,
    ) -> Result<tokio::process::Command> {
        if !self.is_available() {
            return Err(err(
                "windows job objects unavailable; execution denied (sandbox_unavailable)",
            ));
        }
        // Confinement here is cwd + cleared env (from the caller) plus
        // job limits applied post-spawn. The workspace is canonicalized so
        // the cwd cannot escape via symlink.
        let workspace = config
            .workspace_root
            .canonicalize()
            .map_err(|e| err(&format!("workspace root unavailable: {e}")))?;
        Ok(crate::enforce::standard_child(
            spec.program,
            &spec.args,
            &spec.envs,
            &workspace,
        ))
    }

    fn on_spawn(
        &self,
        config: &SandboxConfig,
        child: &tokio::process::Child,
    ) -> Result<crate::enforce::PostSpawnGuard> {
        assign_to_job(child, &JobLimits::from_config(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enforce::SandboxEnforcer;

    #[test]
    fn job_backend_is_available_on_windows() {
        let backend = WindowsSandbox::new();
        assert!(backend.is_available(), "job objects must exist on Windows");
        assert!(backend.availability_detail().contains("Job Objects"));
    }

    #[test]
    fn job_limits_derive_from_config() {
        let config = SandboxConfig {
            cpu_limit_percent: Some(25),
            memory_limit_mb: Some(256),
            ..SandboxConfig::default()
        };
        let limits = JobLimits::from_config(&config);
        assert_eq!(limits.active_process_limit, JOB_PROCESS_CAP);
        assert_eq!(limits.process_memory_limit, Some(256 * 1024 * 1024));
        assert_eq!(limits.cpu_rate, Some(2500));
    }

    #[tokio::test]
    async fn job_assignment_confines_two_processes_to_one_slot() {
        // ActiveProcessLimit=1: assigning a second live child must fail,
        // proving the limit is real (anti-fork-bomb).
        let limits = JobLimits {
            active_process_limit: 1,
            process_memory_limit: None,
            job_memory_limit: None,
            cpu_rate: None,
        };
        let job = create_limited_job(&limits).expect("job creates");
        let mut first = tokio::process::Command::new("cmd")
            .arg("/c")
            .arg("ping -n 30 127.0.0.1 >NUL")
            .spawn()
            .expect("first sleeper spawns");
        let first_handle = first.raw_handle().expect("first has a handle");
        // SAFETY: test-only; both handles are live owned process/job
        // handles in this process.
        unsafe { AssignProcessToJobObject(HANDLE(job), HANDLE(first_handle as _)) }
            .expect("first assign succeeds");
        let mut second = tokio::process::Command::new("cmd")
            .arg("/c")
            .arg("ping -n 30 127.0.0.1 >NUL")
            .spawn()
            .expect("second sleeper spawns");
        let second_handle = second.raw_handle().expect("second has a handle");
        let denied =
            unsafe { AssignProcessToJobObject(HANDLE(job), HANDLE(second_handle as _)) }.is_err();
        let _ = first.kill().await;
        let _ = second.kill().await;
        let _ = first.wait().await;
        let _ = second.wait().await;
        close_job(job);
        assert!(denied, "second process must exceed ActiveProcessLimit=1");
    }
}
