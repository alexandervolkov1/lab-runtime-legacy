//! Internal Windows Job capsule. No GUI, Runtime, network or output authority.
//! Handles never escape as raw values. This is containment, not a security sandbox.
#![deny(unsafe_code)]

use std::path::Path;

/// Checked kernel operation; numeric error has no allocator-backed payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// Exact failed operation or local validation invariant.
    pub operation: &'static str,
    /// Windows error code; zero denotes a local invariant failure.
    pub code: u32,
}
/// Explicit commit policy. No production GUI default is selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Per-process committed-memory limit, not resident/Rust/VRAM bytes.
    pub process_commit: usize,
    /// Whole single-child Job committed-memory limit.
    pub job_commit: usize,
}
impl Limits {
    /// Reject zero caps; callers must select both caps explicitly.
    pub fn validate(self) -> Result<Self, Error> {
        if self.process_commit == 0 || self.job_commit == 0 {
            Err(Error {
                operation: "zero commit limit",
                code: 0,
            })
        } else {
            Ok(self)
        }
    }
}
/// Read-back OS accounting, separate from Preview and driver memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    /// Peak committed memory charged to a process by the Job API.
    pub peak_process_commit: usize,
    /// Peak committed memory charged to the Job.
    pub peak_job_commit: usize,
}
/// Current owned kernel-object counts, not global OS allocations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Objects {
    /// Job handles owned by this capsule.
    pub jobs: usize,
    /// Original process handles.
    pub processes: usize,
    /// Suspended primary thread handles.
    pub threads: usize,
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{Job, ProcessWitness};
#[cfg(all(windows, feature = "fixture-probes"))]
pub use windows::{commit_fixture, fixture_guard};

/// Check launch-token bounds without any filesystem or OS call.
/// The narrow quoting contract rejects embedded quotes/NUL and terminal slash.
pub fn validate_launch(executable: &Path, arguments: &[&str]) -> Result<(), Error> {
    if executable.as_os_str().len() > 1024 || arguments.len() > 8 {
        return Err(Error {
            operation: "launch token bounds",
            code: 0,
        });
    }
    let Some(exe) = executable.to_str() else {
        return Err(Error {
            operation: "executable UTF-8",
            code: 0,
        });
    };
    if !executable.is_absolute()
        || arguments.len() > 8
        || std::iter::once(exe)
            .chain(arguments.iter().copied())
            .any(|s| {
                s.len() > 4096
                    || s.is_empty()
                    || s.contains(['\0', '"'])
                    || s.ends_with('\\')
                    || s.encode_utf16().count() > 1024
            })
    {
        return Err(Error {
            operation: "launch token bounds",
            code: 0,
        });
    }
    Ok(())
}
