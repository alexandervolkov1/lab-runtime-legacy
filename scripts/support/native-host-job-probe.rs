//! Windows x64 architecture preflight only: no GUI, GL, Runtime or instruments.
#![deny(unsafe_code)]

use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, String>;
const PROCESS_COMMIT: usize = 32 * 1024 * 1024; // Probe fixture, not a GUI budget.

fn marker(path: &Path) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(file) = std::fs::File::open(path) {
            let mut bytes = Vec::with_capacity(257);
            file.take(257)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > 256 {
                return Err("oversized fixture marker".into());
            }
            if bytes.ends_with(b"\n") {
                return String::from_utf8(bytes).map_err(|e| e.to_string());
            }
        }
        if Instant::now() >= deadline {
            return Err("fixture marker deadline".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn park() -> ! {
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--park") => park(),
        Some("--commit") => {
            let error = win::commit_check()?;
            std::fs::write(
                &args[2],
                format!("small_commit=4194304 large_commit=67108864 denied_error={error}\n"),
            )
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
        Some("--owner") => {
            let job = win::Job::new(1)?;
            let child = job.spawn(&["--park"])?;
            std::fs::write(&args[2], format!("{}\n", child.pid)).map_err(|e| e.to_string())?;
            // The controller forcibly terminates THIS fixture owner. Kernel
            // handle closure, not Rust Drop, must trigger its un-inherited job.
            park();
        }
        _ => {}
    }
    let dir = Path::new(args.get(1).ok_or("missing fresh fixture directory")?);
    let job = win::Job::new(1)?;
    let child = job.spawn(&["--park"])?;
    let rejected = job
        .spawn(&["--park"])
        .err()
        .ok_or("active-process limit did not reject second fixture")?;
    if child.signaled()? {
        return Err("first child died during second-child rejection".into());
    }
    println!(
        "atomic JOB_LIST membership before ResumeThread PASS; active_process_limit=1 second_create={rejected}; first_alive=true"
    );
    job.close()?;
    child.wait_exit()?;
    println!(
        "last non-inherited job handle close -> process handle signaled PASS; containment only"
    );

    let job = win::Job::new(1)?;
    let child = job.spawn(&["--park"])?;
    job.terminate()?;
    if child.wait_exit()? != 77 {
        return Err("TerminateJobObject exit code mismatch".into());
    }
    println!("TerminateJobObject -> process handle signaled, exit=77 PASS; containment only");
    job.close()?;

    let job = win::Job::new(1)?;
    let path = dir.join("commit.txt");
    let path_text = path.to_str().ok_or("fixture path UTF-8")?;
    let child = job.spawn(&["--commit", path_text])?;
    if child.wait_exit()? != 0 {
        return Err("commit fixture failed".into());
    }
    let reading = job.reading()?;
    if reading.peak_process > PROCESS_COMMIT || reading.peak_job > PROCESS_COMMIT {
        return Err("job peak exceeded configured fixture commit limit".into());
    }
    println!(
        "committed-memory enforcement PASS: {} limit={} peak_process={} peak_job={}; not Rust bytes or VRAM",
        marker(&path)?.trim_end(),
        PROCESS_COMMIT,
        reading.peak_process,
        reading.peak_job
    );
    job.close()?;

    // Outer job owns only these two fixtures and protects failure cleanup. The
    // inner job's sole handle is owned by --owner; none reaches its child.
    let outer = win::Job::new(2)?;
    let path = dir.join("owner.txt");
    let owner = outer.spawn(&["--owner", path.to_str().ok_or("fixture path UTF-8")?])?;
    let pid: u32 = marker(&path)?
        .trim()
        .parse()
        .map_err(|_| "invalid fixture PID")?;
    let nested_child = win::Process::open_fixture(pid)?;
    if owner.signaled()? || nested_child.signaled()? {
        return Err("parent-death fixtures already exited".into());
    }
    owner.terminate()?;
    owner.wait_exit()?;
    nested_child.wait_exit()?;
    println!(
        "forced fixture owner death -> inner KILL_ON_JOB_CLOSE -> child process signaled PASS; no inherited job handle"
    );
    outer.close()?;
    println!(
        "NATIVE HOST JOB PREFLIGHT PASS: five OS expectations; no GPU cleanup/termination-deadline/GUI-budget qualification; activation=false"
    );
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("NATIVE HOST JOB PREFLIGHT BLOCKED: {error}");
        std::process::exit(1);
    }
}

// SAFETY: isolated Windows x64 SDK ABI, no GL or production dependency. Unique
// owned handles close once; job handles are non-inheritable. API buffers and
// attribute values remain allocated through DeleteProcThreadAttributeList.
// Children are created atomically in a configured job, checked while suspended,
// and only then resumed. Every process wait has a finite application deadline.
mod win {
    #![allow(unsafe_code, non_snake_case)]
    use super::{PROCESS_COMMIT, Result};
    use std::{
        ffi::c_void,
        mem::{size_of, zeroed},
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
        time::{Duration, Instant},
    };
    type Handle = *mut c_void;
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
        fn SetInformationJobObject(job: Handle, class: u32, info: *const c_void, bytes: u32)
        -> i32;
        fn QueryInformationJobObject(
            job: Handle,
            class: u32,
            info: *mut c_void,
            bytes: u32,
            returned: *mut u32,
        ) -> i32;
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
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn VirtualAlloc(
            address: *mut c_void,
            bytes: usize,
            kind: u32,
            protection: u32,
        ) -> *mut c_void;
        fn VirtualFree(address: *mut c_void, bytes: usize, kind: u32) -> i32;
    }
    fn checked(ok: i32, op: &str) -> Result<()> {
        if ok == 0 {
            Err(format!("{op}: Win32 {}", unsafe { GetLastError() }))
        } else {
            Ok(())
        }
    }
    struct Owned(Handle);
    impl Owned {
        fn close(&mut self) -> Result<()> {
            if !self.0.is_null() {
                checked(unsafe { CloseHandle(self.0) }, "CloseHandle")?;
                self.0 = null_mut();
            }
            Ok(())
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            if let Err(error) = self.close() {
                eprintln!("HANDLE CLOSE BLOCKED: {error}");
            }
        }
    }
    pub(super) struct Job(Owned);
    pub(super) struct Reading {
        pub(super) peak_process: usize,
        pub(super) peak_job: usize,
    }
    impl Job {
        pub(super) fn new(active: u32) -> Result<Self> {
            let raw = unsafe { CreateJobObjectW(null(), null()) };
            if raw.is_null() {
                return Err(format!("CreateJobObjectW: Win32 {}", unsafe {
                    GetLastError()
                }));
            }
            let job = Self(Owned(raw));
            let mut info = Extended::default();
            info.basic.flags = 0x2000 | 0x100 | 0x200 | 0x8; // KILL_CLOSE, PROCESS_MEMORY, JOB_MEMORY, ACTIVE_PROCESS.
            info.basic.active = active;
            info.process = PROCESS_COMMIT;
            info.job = PROCESS_COMMIT * active as usize;
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
            let mut actual = Extended::default();
            checked(
                unsafe {
                    QueryInformationJobObject(
                        raw,
                        9,
                        (&mut actual as *mut Extended).cast(),
                        size_of::<Extended>() as u32,
                        null_mut(),
                    )
                },
                "QueryInformationJobObject",
            )?;
            if actual.basic.flags != info.basic.flags
                || actual.basic.active != active
                || actual.process != info.process
                || actual.job != info.job
            {
                return Err("job limit readback mismatch".into());
            }
            Ok(job)
        }
        pub(super) fn close(mut self) -> Result<()> {
            self.0.close()
        }
        pub(super) fn terminate(&self) -> Result<()> {
            checked(
                unsafe { TerminateJobObject(self.0.0, 77) },
                "TerminateJobObject",
            )
        }
        pub(super) fn reading(&self) -> Result<Reading> {
            let mut info = Extended::default();
            checked(
                unsafe {
                    QueryInformationJobObject(
                        self.0.0,
                        9,
                        (&mut info as *mut Extended).cast(),
                        size_of::<Extended>() as u32,
                        null_mut(),
                    )
                },
                "QueryInformationJobObject",
            )?;
            Ok(Reading {
                peak_process: info.peak_process,
                peak_job: info.peak_job,
            })
        }
        pub(super) fn spawn(&self, args: &[&str]) -> Result<Process> {
            let mut bytes = 0;
            unsafe {
                InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes);
            }
            if bytes == 0 || bytes > 4096 {
                return Err("attribute storage bound".into());
            }
            let mut attrs = Attributes {
                storage: vec![0_u128; bytes.div_ceil(16)].into_boxed_slice(),
                initialized: false,
                value: Box::new(self.0.0),
            };
            let list = attrs.storage.as_mut_ptr().cast();
            checked(
                unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut bytes) },
                "InitializeProcThreadAttributeList",
            )?;
            attrs.initialized = true;
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
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let app: Vec<u16> = executable.as_os_str().encode_wide().chain([0]).collect();
            // Controlled fixture tokens/paths only. Quotes/backslashes at the
            // end of an argument are forbidden, so Windows quoting is exact.
            let exe = executable.to_str().ok_or("executable path UTF-8")?;
            if std::iter::once(exe)
                .chain(args.iter().copied())
                .any(|s| s.contains('"') || s.ends_with('\\'))
            {
                return Err("fixture argument quoting".into());
            }
            let command = std::iter::once(exe)
                .chain(args.iter().copied())
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(" ");
            let mut command: Vec<u16> = command.encode_utf16().chain([0]).collect();
            let mut startup: StartupEx = unsafe { zeroed() };
            startup.basic.cb = size_of::<StartupEx>() as u32;
            startup.attributes = list;
            let mut info: ProcessInfo = unsafe { zeroed() };
            checked(
                unsafe {
                    CreateProcessW(
                        app.as_ptr(),
                        command.as_mut_ptr(),
                        null(),
                        null(),
                        0,
                        0x80000 | 4 | 0x08000000,
                        null(),
                        null(),
                        &mut startup,
                        &mut info,
                    )
                },
                "CreateProcessW",
            )?;
            let process = Process {
                handle: Owned(info.process),
                pid: info.pid,
            };
            let thread = Owned(info.thread);
            let mut member = 0;
            let membership = checked(
                unsafe { IsProcessInJob(process.handle.0, self.0.0, &mut member) },
                "IsProcessInJob",
            )
            .and_then(|()| {
                if member == 0 {
                    Err("child not in configured job before execution".into())
                } else {
                    Ok(())
                }
            });
            let resume = membership.and_then(|()| {
                if unsafe { ResumeThread(thread.0) } == u32::MAX {
                    Err(format!("ResumeThread: Win32 {}", unsafe { GetLastError() }))
                } else {
                    Ok(())
                }
            });
            if let Err(original) = resume {
                // Roll back this exact OS-only fixture; even a failed membership
                // verification must not abandon a suspended process outside Job.
                let cleanup = process
                    .terminate()
                    .and_then(|()| process.wait_exit().map(|_| ()));
                return Err(format!("{original}; spawn rollback={cleanup:?}"));
            }
            Ok(process)
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
                unsafe {
                    DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast());
                }
            }
        }
    }
    pub(super) struct Process {
        handle: Owned,
        pub(super) pid: u32,
    }
    impl Process {
        pub(super) fn open_fixture(pid: u32) -> Result<Self> {
            let raw = unsafe { OpenProcess(0x100000 | 0x1000, 0, pid) }; // SYNCHRONIZE + QUERY_LIMITED_INFORMATION.
            if raw.is_null() {
                return Err(format!("OpenProcess fixture: Win32 {}", unsafe {
                    GetLastError()
                }));
            }
            Ok(Self {
                handle: Owned(raw),
                pid,
            })
        }
        pub(super) fn signaled(&self) -> Result<bool> {
            match unsafe { WaitForSingleObject(self.handle.0, 0) } {
                0 => Ok(true),
                258 => Ok(false),
                _ => Err("process wait failed".into()),
            }
        }
        pub(super) fn wait_exit(&self) -> Result<u32> {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match unsafe { WaitForSingleObject(self.handle.0, 10) } {
                    0 => {
                        let mut code = 0;
                        checked(
                            unsafe { GetExitCodeProcess(self.handle.0, &mut code) },
                            "GetExitCodeProcess",
                        )?;
                        return Ok(code);
                    }
                    258 if Instant::now() < deadline => {}
                    258 => return Err("process exit unconfirmed at application deadline".into()),
                    _ => return Err("process wait failed".into()),
                }
            }
        }
        pub(super) fn terminate(&self) -> Result<()> {
            checked(
                unsafe { TerminateProcess(self.handle.0, 88) },
                "TerminateProcess fixture owner",
            )
        }
    }
    pub(super) fn commit_check() -> Result<u32> {
        let small = unsafe { VirtualAlloc(null_mut(), 4 * 1024 * 1024, 0x3000, 4) };
        if small.is_null() {
            return Err("small commit failed".into());
        }
        let large = unsafe { VirtualAlloc(null_mut(), 64 * 1024 * 1024, 0x3000, 4) };
        let error = unsafe { GetLastError() };
        checked(
            unsafe { VirtualFree(small, 0, 0x8000) },
            "VirtualFree small",
        )?;
        if !large.is_null() {
            checked(
                unsafe { VirtualFree(large, 0, 0x8000) },
                "VirtualFree large",
            )?;
            return Err("commit limit failed to deny large request".into());
        }
        Ok(error)
    }
}
