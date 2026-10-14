//! Audited Windows x64 ABI capsule. No raw handle is exported or inherited.
//! SAFETY invariants: each handle has one owner; aligned attribute storage and
//! its boxed JOB_LIST value outlive DeleteProcThreadAttributeList; all SDK
//! buffers have checked sizes; process creation is suspended and atomic in Job.
//! Drop closes handles for containment only. It never certifies process exit.
#![allow(unsafe_code, non_snake_case)]

use crate::{Error, Limits, Objects, Reading, validate_launch};
use std::{
    ffi::c_void,
    mem::{size_of, zeroed},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::{null, null_mut},
};
type Handle = *mut c_void;
type Result<T> = std::result::Result<T, Error>;
const FLAGS: u32 = 0x2000 | 0x100 | 0x200 | 0x8;

#[repr(C)]
#[derive(Default)]
struct Basic {
    user: i64,
    job: i64,
    flags: u32,
    min: usize,
    max: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct Extended {
    basic: Basic,
    io: [u64; 6],
    process: usize,
    job: usize,
    peak_process: usize,
    peak_job: usize,
}
#[repr(C)]
struct Startup {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    fill: u32,
    flags: u32,
    show: u16,
    reserved_bytes: u16,
    reserved2: *mut u8,
    input: Handle,
    output: Handle,
    error: Handle,
}
#[repr(C)]
struct StartupEx {
    basic: Startup,
    attributes: *mut c_void,
}
#[repr(C)]
struct ProcessInfo {
    process: Handle,
    thread: Handle,
    pid: u32,
    tid: u32,
}
const _: () = assert!(size_of::<Extended>() == 144 && size_of::<StartupEx>() == 112);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(security: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: u32, info: *const c_void, bytes: u32) -> i32;
    fn QueryInformationJobObject(
        job: Handle,
        class: u32,
        info: *mut c_void,
        bytes: u32,
        returned: *mut u32,
    ) -> i32;
    fn GetHandleInformation(handle: Handle, flags: *mut u32) -> i32;
    fn IsProcessInJob(process: Handle, job: Handle, result: *mut i32) -> i32;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        bytes: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *mut c_void,
        bytes: usize,
        previous: *mut c_void,
        returned: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn CreateProcessW(
        app: *const u16,
        cmd: *mut u16,
        process_security: *const c_void,
        thread_security: *const c_void,
        inherit: i32,
        flags: u32,
        env: *const c_void,
        cwd: *const u16,
        startup: *mut StartupEx,
        info: *mut ProcessInfo,
    ) -> i32;
    fn ResumeThread(thread: Handle) -> u32;
    fn CloseHandle(handle: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
    fn GetLastError() -> u32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn TerminateProcess(process: Handle, code: u32) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn GetWindowsDirectoryW(buffer: *mut u16, size: u32) -> u32;
    #[cfg(feature = "fixture-probes")]
    fn GetProcessId(process: Handle) -> u32;
    fn DuplicateHandle(
        source_process: Handle,
        source: Handle,
        target_process: Handle,
        target: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    #[cfg(feature = "fixture-probes")]
    fn VirtualAlloc(address: *mut c_void, bytes: usize, kind: u32, protection: u32) -> *mut c_void;
    #[cfg(feature = "fixture-probes")]
    fn VirtualFree(address: *mut c_void, bytes: usize, kind: u32) -> i32;
}
fn error(operation: &'static str) -> Error {
    // SAFETY: GetLastError takes no pointers and is read immediately on this thread.
    Error {
        operation,
        code: unsafe { GetLastError() },
    }
}
fn checked(ok: i32, operation: &'static str) -> Result<()> {
    if ok == 0 {
        Err(error(operation))
    } else {
        Ok(())
    }
}
struct Owned(Handle);
impl Owned {
    fn non_inherited(&self) -> Result<()> {
        let mut flags = 0;
        // SAFETY: valid uniquely owned handle and writable DWORD.
        checked(
            unsafe { GetHandleInformation(self.0, &mut flags) },
            "GetHandleInformation",
        )?;
        if flags & 1 != 0 {
            Err(Error {
                operation: "inheritable handle",
                code: 0,
            })
        } else {
            Ok(())
        }
    }
    fn close(&mut self) -> Result<()> {
        if !self.0.is_null() {
            // SAFETY: this owner closes its handle once, clearing it only on success.
            checked(unsafe { CloseHandle(self.0) }, "CloseHandle")?;
            self.0 = null_mut();
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if let Err(e) = self.close() {
            eprintln!("Windows handle release unconfirmed: {e:?}");
        }
    }
}
/// Unique Job and at most one original process/primary-thread record.
/// Construct/use on the supervisor worker; deliberately not Send/Sync.
pub struct Job {
    handle: Owned,
    limits: Limits,
    active: u32,
    process: Option<Owned>,
    thread: Option<Owned>,
    pid: Option<u32>,
}
impl Job {
    /// Unnamed, non-inheritable, no breakaway, ACTIVE_PROCESS=1, explicit caps.
    pub fn new(limits: Limits) -> Result<Self> {
        Self::new_active(limits, 1)
    }
    fn new_active(limits: Limits, active: u32) -> Result<Self> {
        let limits = limits.validate()?;
        // SAFETY: null security/name means non-inherited, unnamed Job.
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() {
            return Err(error("CreateJobObjectW"));
        }
        let job = Self {
            handle: Owned(raw),
            limits,
            active,
            process: None,
            thread: None,
            pid: None,
        };
        job.handle.non_inherited()?;
        let mut info = Extended::default();
        info.basic.flags = FLAGS;
        info.basic.active = active;
        info.process = limits.process_commit;
        info.job = limits.job_commit;
        // SAFETY: valid handle, exact SDK buffer/size, no borrowed values retained.
        checked(
            unsafe {
                SetInformationJobObject(
                    raw,
                    9,
                    (&info as *const Extended).cast(),
                    size_of::<Extended>() as u32,
                )
            },
            "SetInformationJobObject",
        )?;
        job.verify_limits()?;
        Ok(job)
    }
    /// Read/check exact enforced limits; no breakaway flag can pass this gate.
    pub fn verify_limits(&self) -> Result<Reading> {
        self.handle.non_inherited()?;
        let mut info = Extended::default();
        // SAFETY: valid Job and fully initialized SDK output buffer.
        checked(
            unsafe {
                QueryInformationJobObject(
                    self.handle.0,
                    9,
                    (&mut info as *mut Extended).cast(),
                    size_of::<Extended>() as u32,
                    null_mut(),
                )
            },
            "QueryInformationJobObject",
        )?;
        if info.basic.flags != FLAGS
            || info.basic.active != self.active
            || info.process != self.limits.process_commit
            || info.job != self.limits.job_commit
        {
            return Err(Error {
                operation: "Job limit readback mismatch",
                code: 0,
            });
        }
        Ok(Reading {
            peak_process_commit: info.peak_process,
            peak_job_commit: info.peak_job,
        })
    }
    /// Atomically create in JOB_LIST, suspended. On post-create failure the exact
    /// process/thread stay owned here for rollback/exit observation; never resumed.
    pub fn prepare(&mut self, executable: &Path, arguments: &[&str]) -> Result<()> {
        if self.process.is_some() {
            return Err(Error {
                operation: "second process record forbidden",
                code: 0,
            });
        }
        validate_launch(executable, arguments)?;
        self.verify_limits()?;
        let mut bytes = 0;
        // SAFETY: documented sizing call with null output list.
        let sized = unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
        let sizing_error = error("InitializeProcThreadAttributeList sizing");
        if sized != 0 || sizing_error.code != 122 || bytes == 0 || bytes > 4096 {
            return Err(sizing_error);
        }
        let mut attrs = Attributes {
            storage: vec![0_u128; bytes.div_ceil(16)].into_boxed_slice(),
            initialized: false,
            value: Box::new(self.handle.0),
        };
        let list = attrs.storage.as_mut_ptr().cast();
        // SAFETY: 16-byte aligned storage bounded by the successful sizing call.
        checked(
            unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) },
            "InitializeProcThreadAttributeList",
        )?;
        attrs.initialized = true;
        // SAFETY: boxed value is stable until attribute list deletion.
        checked(
            unsafe {
                UpdateProcThreadAttribute(
                    list,
                    0,
                    0x2000d,
                    (&mut *attrs.value as *mut Handle).cast(),
                    size_of::<Handle>(),
                    null_mut(),
                    null_mut(),
                )
            },
            "UpdateProcThreadAttribute JOB_LIST",
        )?;
        let exe = executable.to_str().expect("validated UTF-8");
        let command_bytes = exe.len()
            + arguments.iter().map(|s| s.len()).sum::<usize>()
            + 2 * (arguments.len() + 1)
            + arguments.len();
        let mut command = String::with_capacity(command_bytes);
        for (index, token) in std::iter::once(exe)
            .chain(arguments.iter().copied())
            .enumerate()
        {
            if index != 0 {
                command.push(' ');
            }
            command.push('"');
            command.push_str(token);
            command.push('"');
        }
        let mut command_wide = Vec::with_capacity(9 * 1024 + 18 + 8 + 1);
        command_wide.extend(command.encode_utf16());
        command_wide.push(0);
        let mut app = Vec::with_capacity(1025);
        app.extend(executable.as_os_str().encode_wide());
        app.push(0);
        let parent = executable.parent().ok_or(Error {
            operation: "executable parent",
            code: 0,
        })?;
        let mut cwd = Vec::with_capacity(1025);
        cwd.extend(parent.as_os_str().encode_wide());
        cwd.push(0);
        // Do not transfer Runtime tokens or parent launch controls via environment.
        let mut directory = [0_u16; 1024];
        // SAFETY: fixed writable UTF-16 buffer; returned length checked before use.
        let length =
            unsafe { GetWindowsDirectoryW(directory.as_mut_ptr(), directory.len() as u32) };
        if length == 0 {
            return Err(error("GetWindowsDirectoryW"));
        }
        if length as usize >= directory.len() {
            return Err(Error {
                operation: "Windows directory bound",
                code: 0,
            });
        }
        let mut env = Vec::with_capacity(1040);
        env.extend("SystemRoot=".encode_utf16());
        env.extend_from_slice(&directory[..length as usize]);
        env.extend([0, 0]);
        // SAFETY: all-zero optional SDK fields are valid, then required fields set.
        let mut startup: StartupEx = unsafe { zeroed() };
        startup.basic.cb = size_of::<StartupEx>() as u32;
        startup.attributes = list;
        // SAFETY: output structure initialized before native creation.
        let mut info: ProcessInfo = unsafe { zeroed() };
        // SAFETY: live NUL-terminated mutable command, immutable app/env/cwd,
        // non-inherited handles, JOB_LIST+SUSPENDED; no breakaway/fallback path.
        checked(
            unsafe {
                CreateProcessW(
                    app.as_ptr(),
                    command_wide.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    0x80000 | 4 | 0x08000000 | 0x400,
                    env.as_ptr().cast(),
                    cwd.as_ptr(),
                    &mut startup,
                    &mut info,
                )
            },
            "CreateProcessW",
        )?;
        self.process = Some(Owned(info.process));
        self.thread = Some(Owned(info.thread));
        self.pid = Some(info.pid);
        self.process.as_ref().unwrap().non_inherited()?;
        self.thread.as_ref().unwrap().non_inherited()?;
        self.verify_membership()?;
        self.verify_limits()?;
        Ok(())
    }
    /// Prove membership of the original process, never a process looked up by PID.
    pub fn verify_membership(&self) -> Result<()> {
        let process = self.process.as_ref().ok_or(Error {
            operation: "missing process",
            code: 0,
        })?;
        let mut member = 0;
        // SAFETY: valid original process and this Job; writable BOOL.
        checked(
            unsafe { IsProcessInJob(process.0, self.handle.0, &mut member) },
            "IsProcessInJob",
        )?;
        if member == 0 {
            Err(Error {
                operation: "Job membership mismatch",
                code: 0,
            })
        } else {
            Ok(())
        }
    }
    /// Recheck gates and resume the one suspended primary thread.
    pub fn resume(&mut self) -> Result<()> {
        self.verify_membership()?;
        self.verify_limits()?;
        let thread = self.thread.as_mut().ok_or(Error {
            operation: "missing suspended thread",
            code: 0,
        })?;
        // SAFETY: unique primary thread from CREATE_SUSPENDED, expected count one.
        let previous = unsafe { ResumeThread(thread.0) };
        if previous == u32::MAX {
            return Err(error("ResumeThread"));
        }
        if previous != 1 {
            return Err(Error {
                operation: "unexpected suspend count",
                code: 0,
            });
        }
        thread.close()?;
        self.thread = None;
        Ok(())
    }
    /// Zero-time original-process observation. Only signaled + checked exit code
    /// proves exit; timeout is alive, wait/query failure is unknown.
    pub fn poll_exit(&self) -> Result<Option<u32>> {
        poll(self.process.as_ref().ok_or(Error {
            operation: "missing process",
            code: 0,
        })?)
    }
    /// Request Job termination; success is not an exit receipt.
    pub fn terminate(&self, code: u32) -> Result<()> {
        // SAFETY: valid owned Job, no unrelated process belongs to this Job.
        checked(
            unsafe { TerminateJobObject(self.handle.0, code) },
            "TerminateJobObject",
        )
    }
    /// Exact suspended-process startup rollback if a post-create gate failed.
    /// Outcome still requires poll_exit and must not be retried implicitly.
    pub fn rollback(&self, code: u32) -> Result<()> {
        let process = self.process.as_ref().ok_or(Error {
            operation: "missing process",
            code: 0,
        })?;
        // SAFETY: original uniquely recorded suspended/possibly resumed process.
        checked(
            unsafe { TerminateProcess(process.0, code) },
            "TerminateProcess startup rollback",
        )
    }
    /// Retire only after actual original-process exit. Failure retains remaining
    /// handles; caller must not start another incarnation with this record.
    pub fn retire(&mut self) -> Result<()> {
        if self.process.is_some() && self.poll_exit()?.is_none() {
            return Err(Error {
                operation: "retirement before confirmed exit",
                code: 0,
            });
        }
        if let Some(thread) = &mut self.thread {
            thread.close()?;
        }
        self.thread = None;
        if let Some(process) = &mut self.process {
            process.close()?;
        }
        self.process = None;
        self.pid = None;
        self.handle.close()
    }
    /// Bounded owned-object ledger.
    pub fn objects(&self) -> Objects {
        Objects {
            jobs: usize::from(!self.handle.0.is_null()),
            processes: usize::from(self.process.is_some()),
            threads: usize::from(self.thread.is_some()),
        }
    }
    /// Diagnostic PID only; never used as an exit receipt/replacement handle.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
    /// Duplicate a read-only witness from the ORIGINAL process for acceptance
    /// guards. No Job/termination authority or inheritable handle is duplicated.
    pub fn witness(&self) -> Result<ProcessWitness> {
        let process = self.process.as_ref().ok_or(Error {
            operation: "missing process",
            code: 0,
        })?;
        let mut raw = null_mut();
        // SAFETY: current process pseudo handles and valid original source; output
        // is independent, non-inherited, SYNCHRONIZE+QUERY_LIMITED_INFORMATION only.
        checked(
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    process.0,
                    GetCurrentProcess(),
                    &mut raw,
                    0x100000 | 0x1000,
                    0,
                    0,
                )
            },
            "DuplicateHandle witness",
        )?;
        Ok(ProcessWitness(Owned(raw)))
    }
}
struct Attributes {
    storage: Box<[u128]>,
    initialized: bool,
    value: Box<Handle>,
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            // SAFETY: initialized list storage and value still allocated here.
            unsafe {
                DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast());
            }
        }
    }
}
fn poll(handle: &Owned) -> Result<Option<u32>> {
    // SAFETY: valid owned process handle, zero timeout; no PID lookup.
    match unsafe { WaitForSingleObject(handle.0, 0) } {
        0 => {
            let mut code = 0;
            // SAFETY: signaled original process, writable DWORD.
            checked(
                unsafe { GetExitCodeProcess(handle.0, &mut code) },
                "GetExitCodeProcess",
            )?;
            Ok(Some(code))
        }
        258 => Ok(None),
        _ => Err(error("WaitForSingleObject")),
    }
}
/// Read-only test/containment witness; no process termination or Job authority.
pub struct ProcessWitness(Owned);
// SAFETY: unique non-inherited read-only kernel process handle; Windows process
// waits/queries have no thread affinity. Transfer cannot duplicate its ownership.
unsafe impl Send for ProcessWitness {}
impl ProcessWitness {
    /// Observe this exact duplicated original process handle.
    pub fn poll_exit(&self) -> Result<Option<u32>> {
        poll(&self.0)
    }
}

/// Commit-denial fixture, no GPU/Rust allocation measurement.
#[cfg(feature = "fixture-probes")]
pub fn commit_fixture() -> Result<u32> {
    // SAFETY: anonymous private committed pages, bounded sizes, no dereference.
    let small = unsafe { VirtualAlloc(null_mut(), 4 * 1024 * 1024, 0x3000, 4) };
    if small.is_null() {
        return Err(error("VirtualAlloc small"));
    }
    // SAFETY: deliberately over the 32 MiB FIXTURE cap, no pointer dereference.
    let large = unsafe { VirtualAlloc(null_mut(), 64 * 1024 * 1024, 0x3000, 4) };
    let denied = error("VirtualAlloc denied");
    // SAFETY: free this exact allocation, MEM_RELEASE with size zero.
    checked(
        unsafe { VirtualFree(small, 0, 0x8000) },
        "VirtualFree small",
    )?;
    if !large.is_null() {
        // SAFETY: unexpectedly admitted allocation still released exactly once.
        checked(
            unsafe { VirtualFree(large, 0, 0x8000) },
            "VirtualFree large",
        )?;
        return Err(Error {
            operation: "fixture commit cap not enforced",
            code: 0,
        });
    }
    Ok(denied.code)
}
/// Two-process outer acceptance guard, never the production single-child policy.
#[cfg(feature = "fixture-probes")]
pub fn fixture_guard(limits: Limits) -> Result<Job> {
    Job::new_active(limits, 2)
}

#[cfg(feature = "fixture-probes")]
impl Job {
    /// Test-only report of the original child process identity before execution.
    /// Never a production protocol, Job handle, or termination authority transfer.
    pub fn fixture_process_identity(&self) -> Result<(u32, usize)> {
        let process = self.process.as_ref().ok_or(Error {
            operation: "missing fixture process",
            code: 0,
        })?;
        Ok((self.pid.unwrap(), process.0 as usize))
    }
    /// Outer acceptance guard duplicates an inner ORIGINAL process handle from
    /// its still-live fixture parent; no unfenced PID lookup is used.
    pub fn fixture_duplicate_child(
        &self,
        original: usize,
        expected_pid: u32,
    ) -> Result<ProcessWitness> {
        let parent = self.process.as_ref().ok_or(Error {
            operation: "missing fixture parent",
            code: 0,
        })?;
        let mut raw = null_mut();
        // SAFETY: controlled fixture report while source parent is alive. Only
        // SYNCHRONIZE/QUERY rights are duplicated, with inheritance disabled.
        checked(
            unsafe {
                DuplicateHandle(
                    parent.0,
                    original as Handle,
                    GetCurrentProcess(),
                    &mut raw,
                    0x100000 | 0x1000,
                    0,
                    0,
                )
            },
            "DuplicateHandle fixture child",
        )?;
        let witness = ProcessWitness(Owned(raw));
        // SAFETY: valid read-only process handle returned by DuplicateHandle.
        if unsafe { GetProcessId(witness.0.0) } != expected_pid {
            return Err(Error {
                operation: "fixture process identity mismatch",
                code: 0,
            });
        }
        Ok(witness)
    }
}
