//! Bubblewrap (`bwrap`) process isolation runner and resource limiter.
//!
//! Provides secure execution of untrusted format parsers (PDF rendering, media probing,
//! and metadata extraction) inside an unprivileged namespace container with dropped capabilities,
//! network isolation (`--unshare-net`), and strict read-only filesystem bindings.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use crate::quicklook::error::{QuickLookError, Result};

static HAS_BWRAP: OnceLock<bool> = OnceLock::new();

/// Checks whether the Bubblewrap (`bwrap`) binary is installed and executable on the host system.
#[must_use]
pub fn has_bubblewrap() -> bool {
    *HAS_BWRAP.get_or_init(|| {
        std::process::Command::new("bwrap")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    })
}

/// Configuration options for sandboxed command execution.
#[derive(Debug, Clone)]
pub struct SandboxConfig<'a> {
    /// Target file to be inspected or parsed inside the sandbox container.
    pub target_file: &'a Path,
    /// Additional host directory paths to mount as read-only inside the container.
    pub allowed_ro_dirs: &'a [&'a Path],
    /// Maximum execution duration before the child process is killed.
    pub timeout: Duration,
}

impl<'a> SandboxConfig<'a> {
    /// Creates a new [`SandboxConfig`] with default 2000ms timeout and no extra read-only directories.
    #[must_use]
    pub const fn new(target_file: &'a Path) -> Self {
        Self {
            target_file,
            allowed_ro_dirs: &[],
            timeout: Duration::from_millis(2000),
        }
    }

    /// Configures additional read-only directory mounts.
    #[must_use]
    pub const fn with_ro_dirs(mut self, allowed_ro_dirs: &'a [&'a Path]) -> Self {
        self.allowed_ro_dirs = allowed_ro_dirs;
        self
    }

    /// Sets the maximum execution duration.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

fn build_bwrap_command(
    program: &str,
    args: &[&str],
    config: &SandboxConfig<'_>,
    rw_dirs: &[&Path],
) -> tokio::process::Command {
    let mut bwrap = tokio::process::Command::new("bwrap");
    bwrap
        .arg("--unshare-net")
        .arg("--unshare-pid")
        .arg("--die-with-parent")
        .arg("--ro-bind")
        .arg("/usr")
        .arg("/usr")
        .arg("--ro-bind-try")
        .arg("/lib")
        .arg("/lib")
        .arg("--ro-bind-try")
        .arg("/lib64")
        .arg("/lib64")
        .arg("--ro-bind-try")
        .arg("/bin")
        .arg("/bin")
        .arg("--ro-bind-try")
        .arg("/etc/fonts")
        .arg("/etc/fonts")
        .arg("--ro-bind-try")
        .arg("/etc/ld.so.cache")
        .arg("/etc/ld.so.cache")
        .arg("--ro-bind-try")
        .arg("/etc/ld.so.conf")
        .arg("/etc/ld.so.conf")
        .arg("--ro-bind-try")
        .arg("/etc/ld.so.conf.d")
        .arg("/etc/ld.so.conf.d")
        .arg("--ro-bind-try")
        .arg("/etc/alternatives")
        .arg("/etc/alternatives")
        .arg("--ro-bind-try")
        .arg("/usr/share/fontconfig")
        .arg("/usr/share/fontconfig")
        .arg("--tmpfs")
        .arg("/tmp");

    for rw in rw_dirs {
        bwrap.arg("--bind").arg(rw).arg(rw);
    }

    for ro in config.allowed_ro_dirs {
        bwrap.arg("--ro-bind").arg(ro).arg(ro);
    }

    if config.target_file.exists() {
        bwrap
            .arg("--ro-bind")
            .arg(config.target_file)
            .arg(config.target_file);
    }

    bwrap.arg("--cap-drop").arg("ALL").arg("--");
    bwrap.arg(program);
    bwrap.args(args);
    bwrap
}

fn build_fallback_command(program: &str, args: &[&str]) -> tokio::process::Command {
    let mut fallback = tokio::process::Command::new(program);
    fallback.args(args);

    #[allow(unsafe_code)]
    // SAFETY: The closure only invokes async-signal-safe POSIX setrlimit calls
    // in the child process between fork and exec.
    unsafe {
        fallback.pre_exec(|| {
            use nix::sys::resource::{Resource, setrlimit};
            // CPU time limit: 5 seconds
            let _ = setrlimit(Resource::RLIMIT_CPU, 5, 5);
            // Address space limit: 1 GB
            let as_limit = 1024 * 1024 * 1024;
            let _ = setrlimit(Resource::RLIMIT_AS, as_limit, as_limit);
            Ok(())
        });
    }

    fallback
}

/// Executes an external program within an isolated Bubblewrap container or resource-limited fallback.
///
/// # Errors
///
/// Returns [`QuickLookError::Timeout`] if execution exceeds the configured duration.
/// Returns [`QuickLookError::Sandbox`] if process spawning fails or exits with a non-zero status.
pub async fn run_sandboxed(
    program: &str,
    args: &[&str],
    config: &SandboxConfig<'_>,
) -> Result<std::process::Output> {
    run_sandboxed_with_rw(program, args, config, &[]).await
}

/// Executes an external program with optional read-write bind mounts.
///
/// # Errors
///
/// Returns [`QuickLookError::Timeout`] if execution exceeds the configured duration.
/// Returns [`QuickLookError::Sandbox`] if process spawning fails or exits with a non-zero status.
pub async fn run_sandboxed_with_rw(
    program: &str,
    args: &[&str],
    config: &SandboxConfig<'_>,
    rw_dirs: &[&Path],
) -> Result<std::process::Output> {
    let mut cmd = if has_bubblewrap() {
        build_bwrap_command(program, args, config, rw_dirs)
    } else {
        build_fallback_command(program, args)
    };

    cmd.kill_on_drop(true);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = cmd.spawn().map_err(|e| {
        QuickLookError::sandbox(
            config.target_file,
            format!("Failed to spawn child process '{program}': {e}"),
        )
    })?;

    let pid = child.id();
    let timeout_duration = config.timeout;

    match tokio::time::timeout(timeout_duration, child.wait_with_output()).await {
        Ok(Ok(output)) => {
            if output.status.success() {
                Ok(output)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(QuickLookError::sandbox(
                    config.target_file,
                    format!(
                        "Process '{program}' exited with {}: {}",
                        output.status,
                        stderr.trim()
                    ),
                ))
            }
        }
        Ok(Err(io_err)) => Err(QuickLookError::sandbox(
            config.target_file,
            format!("Process I/O error during execution of '{program}': {io_err}"),
        )),
        Err(_elapsed) => {
            if let Some(pid_i32) = pid.and_then(|p| i32::try_from(p).ok()) {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid_i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            Err(QuickLookError::timeout(
                config.target_file,
                u64::try_from(timeout_duration.as_millis()).unwrap_or(u64::MAX),
            ))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[test]
    fn test_has_bubblewrap_detection() {
        let available = has_bubblewrap();
        // Just verify it returns without panic and caches correctly
        assert_eq!(available, has_bubblewrap());
    }

    #[tokio::test]
    async fn test_sandbox_benign_command() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = SandboxConfig::new(temp_file.path()).with_timeout(Duration::from_secs(3));

        let result = run_sandboxed("/bin/echo", &["quicklook_sandbox_test"], &config).await;
        assert!(
            result.is_ok(),
            "Expected echo command to succeed in sandbox: {result:?}"
        );

        let output = result.unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("quicklook_sandbox_test"));
    }

    #[tokio::test]
    async fn test_sandbox_timeout_kill() {
        let temp_file = NamedTempFile::new().unwrap();
        // 50ms timeout on a 2-second sleep must trigger timeout error
        let config = SandboxConfig::new(temp_file.path()).with_timeout(Duration::from_millis(50));

        let start = std::time::Instant::now();
        let result = run_sandboxed("/bin/sleep", &["2"], &config).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "Expected command to time out");
        match result.unwrap_err() {
            QuickLookError::Timeout { timeout_ms, .. } => {
                assert_eq!(timeout_ms, 50);
            }
            other => panic!("Expected QuickLookError::Timeout, got: {other:?}"),
        }
        assert!(
            elapsed < Duration::from_millis(1500),
            "Process should be killed promptly"
        );
    }

    #[tokio::test]
    async fn test_sandbox_nonzero_exit() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = SandboxConfig::new(temp_file.path()).with_timeout(Duration::from_secs(3));

        let result = run_sandboxed("/bin/false", &[], &config).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            QuickLookError::Sandbox { message, .. } => {
                assert!(message.contains("exited with"));
            }
            other => panic!("Expected QuickLookError::Sandbox, got: {other:?}"),
        }
    }
}
