//! Audited x64 overlapped pipe capsule. Kernel borrows never escape this owner.
//!
//! Each operation and its fixed buffer occupy one stable Box. Cancellation only
//! requests completion. An unfinished Drop deliberately retains the whole owner;
//! the supervisor must retain and poll it instead of creating a replacement.
#![allow(unsafe_code, non_snake_case)]

use crate::Error;
use std::{
    ffi::c_void,
    mem::size_of,
    ptr::{null, null_mut},
};

type Handle = *mut c_void;
type Result<T> = std::result::Result<T, Error>;
const PENDING: u32 = 997;
const INCOMPLETE: u32 = 996;
const CONNECTED: u32 = 535;

#[repr(C)]
#[derive(Default)]
struct Overlapped {
    internal: usize,
    high: usize,
    offset: u64,
    event: Handle,
}
#[repr(C)]
struct Security {
    size: u32,
    descriptor: *mut c_void,
    inherit: i32,
}
#[repr(C)]
struct TokenUser {
    sid: *mut c_void,
    attributes: u32,
}
#[repr(C)]
struct Acl {
    revision: u8,
    reserved: u8,
    size: u16,
    count: u16,
    reserved2: u16,
}
#[repr(C)]
struct AllowedAce {
    kind: u8,
    flags: u8,
    size: u16,
    mask: u32,
    sid: u32,
}
const _: () = assert!(size_of::<Overlapped>() == 32 && size_of::<Security>() == 24);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        access: u32,
        mode: u32,
        instances: u32,
        output: u32,
        input: u32,
        timeout: u32,
        security: *const Security,
    ) -> Handle;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const Security,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn ConnectNamedPipe(pipe: Handle, overlap: *mut Overlapped) -> i32;
    fn GetNamedPipeClientProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn GetNamedPipeServerProcessId(pipe: Handle, pid: *mut u32) -> i32;
    fn CreateEventW(
        security: *const Security,
        manual: i32,
        initial: i32,
        name: *const u16,
    ) -> Handle;
    fn ResetEvent(event: Handle) -> i32;
    fn ReadFile(
        file: Handle,
        buffer: *mut u8,
        count: u32,
        transferred: *mut u32,
        overlap: *mut Overlapped,
    ) -> i32;
    fn WriteFile(
        file: Handle,
        buffer: *const u8,
        count: u32,
        transferred: *mut u32,
        overlap: *mut Overlapped,
    ) -> i32;
    fn GetOverlappedResult(
        file: Handle,
        overlap: *mut Overlapped,
        transferred: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(file: Handle, overlap: *mut Overlapped) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetLastError() -> u32;
    fn GetHandleInformation(handle: Handle, flags: *mut u32) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
    fn GetTokenInformation(
        token: Handle,
        class: u32,
        buffer: *mut c_void,
        length: u32,
        required: *mut u32,
    ) -> i32;
    fn ConvertSidToStringSidW(sid: *const c_void, text: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        text: *const u16,
        revision: u32,
        descriptor: *mut *mut c_void,
        size: *mut u32,
    ) -> i32;
    fn GetSecurityInfo(
        handle: Handle,
        kind: u32,
        information: u32,
        owner: *mut *mut c_void,
        group: *mut *mut c_void,
        dacl: *mut *mut Acl,
        sacl: *mut *mut Acl,
        descriptor: *mut *mut c_void,
    ) -> u32;
    fn GetSecurityDescriptorOwner(
        descriptor: *const c_void,
        owner: *mut *mut c_void,
        defaulted: *mut i32,
    ) -> i32;
    fn GetSecurityDescriptorControl(
        descriptor: *const c_void,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
    fn GetAce(acl: *const Acl, index: u32, ace: *mut *mut c_void) -> i32;
    fn EqualSid(first: *const c_void, second: *const c_void) -> i32;
}
fn err(operation: &'static str) -> Error {
    Error {
        operation,
        code: unsafe { GetLastError() },
    }
}
fn invariant(operation: &'static str) -> Error {
    Error { operation, code: 0 }
}
fn check(ok: i32, operation: &'static str) -> Result<()> {
    if ok == 0 { Err(err(operation)) } else { Ok(()) }
}
struct Owned(Handle);
impl Owned {
    fn new(raw: Handle) -> Result<Self> {
        if raw.is_null() || raw as isize == -1 {
            return Err(err("pipe handle creation"));
        }
        let result = Self(raw);
        let mut flags = 0;
        check(
            unsafe { GetHandleInformation(raw, &mut flags) },
            "pipe handle flags",
        )?;
        if flags & 1 != 0 {
            return Err(invariant("inheritable pipe handle"));
        }
        Ok(result)
    }
    fn close(&mut self) -> Result<()> {
        if !self.0.is_null() {
            check(unsafe { CloseHandle(self.0) }, "pipe CloseHandle")?;
            self.0 = null_mut();
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

// Fixed aligned TOKEN_USER space includes the maximum Windows SID, no allocation
// based on the returned required length. Only this process's user gets a DACL ACE.
fn owner_descriptor() -> Result<Local> {
    let mut token = null_mut();
    check(
        unsafe { OpenProcessToken(GetCurrentProcess(), 8, &mut token) },
        "OpenProcessToken",
    )?;
    let token = Owned::new(token)?;
    let mut storage = [0u64; 128];
    let mut required = 0;
    check(
        unsafe {
            GetTokenInformation(
                token.0,
                1,
                storage.as_mut_ptr().cast(),
                size_of_val(&storage) as u32,
                &mut required,
            )
        },
        "TokenUser",
    )?;
    if required as usize > size_of_val(&storage) || (required as usize) < size_of::<TokenUser>() {
        return Err(invariant("TokenUser size"));
    }
    let user = unsafe { &*storage.as_ptr().cast::<TokenUser>() };
    let mut sid = null_mut();
    check(
        unsafe { ConvertSidToStringSidW(user.sid, &mut sid) },
        "ConvertSidToStringSidW",
    )?;
    let sid_owner = Local(sid.cast());
    let mut text = String::new();
    for i in 0..184 {
        // Maximum textual SID fits this fixed ceiling.
        let c = unsafe { *sid.add(i) };
        if c == 0 {
            break;
        }
        text.push(char::from_u32(u32::from(c)).ok_or(invariant("SID text"))?);
    }
    drop(sid_owner);
    let sddl: Vec<u16> = format!("O:{text}D:P(A;;GA;;;{text})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = null_mut();
    check(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            )
        },
        "owner-only pipe DACL",
    )?;
    Ok(Local(descriptor))
}
// Read back the actual kernel DACL. The sole allow ACE must name the explicit
// current-user owner; inherited/default Everyone/anonymous access is forbidden.
fn verify_owner_acl(handle: Handle, expected: &Local) -> Result<()> {
    let (mut owner, mut acl, mut raw) = (null_mut(), null_mut(), null_mut());
    let code = unsafe {
        GetSecurityInfo(
            handle,
            6,
            1 | 4,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut raw,
        )
    };
    if code != 0 {
        return Err(Error {
            operation: "pipe GetSecurityInfo",
            code,
        });
    }
    let actual = Local(raw);
    let (mut expected_owner, mut defaulted) = (null_mut(), 0);
    check(
        unsafe { GetSecurityDescriptorOwner(expected.0, &mut expected_owner, &mut defaulted) },
        "pipe expected owner",
    )?;
    let (mut control, mut revision) = (0, 0);
    check(
        unsafe { GetSecurityDescriptorControl(actual.0, &mut control, &mut revision) },
        "pipe DACL control",
    )?;
    if owner.is_null()
        || acl.is_null()
        || control & 0x1000 == 0
        || unsafe { (*acl).count } != 1
        || unsafe { EqualSid(owner, expected_owner) } == 0
    {
        return Err(invariant("pipe owner-only DACL readback"));
    }
    let mut ace = null_mut();
    check(unsafe { GetAce(acl, 0, &mut ace) }, "pipe sole owner ACE")?;
    let ace = unsafe { &*ace.cast::<AllowedAce>() };
    if ace.kind != 0
        || ace.flags != 0
        || ace.size < 12
        || !matches!(ace.mask, 0x10000000 | 0x001f01ff)
        || unsafe { EqualSid((&ace.sid as *const u32).cast(), owner) } == 0
    {
        return Err(invariant("pipe owner allow ACE"));
    }
    Ok(())
}
fn name(value: &str) -> Result<Vec<u16>> {
    if !value.starts_with(r"\\.\pipe\lab-workbench-") || value.len() > 200 || value.contains('\0') {
        return Err(invariant("local pipe name"));
    }
    Ok(value.encode_utf16().chain(Some(0)).collect())
}

struct Operation<const N: usize> {
    overlap: Overlapped,
    event: Owned,
    buffer: Box<[u8]>,
    pending: bool,
    ready: Option<Result<usize>>,
    requested: usize,
    offset: usize,
}
impl<const N: usize> Operation<N> {
    fn new() -> Result<Box<Self>> {
        let event = Owned::new(unsafe { CreateEventW(null(), 1, 0, null()) })?;
        Ok(Box::new(Self {
            overlap: Overlapped {
                event: event.0,
                ..Default::default()
            },
            event,
            buffer: vec![0; N].into_boxed_slice(),
            pending: false,
            ready: None,
            requested: 0,
            offset: 0,
        }))
    }
    fn reset(&mut self, count: usize) -> Result<()> {
        if self.pending || self.ready.is_some() || count > N {
            return Err(invariant("borrowed pipe buffer or size"));
        }
        check(unsafe { ResetEvent(self.event.0) }, "ResetEvent")?;
        self.overlap = Overlapped {
            event: self.event.0,
            ..Default::default()
        };
        self.requested = count;
        Ok(())
    }
    fn submitted(&mut self, ok: i32, connect: bool) -> Result<()> {
        if ok != 0 {
            self.pending = true;
            return Ok(());
        }
        let failure = err("overlapped submit");
        if failure.code == PENDING {
            self.pending = true;
            Ok(())
        } else if connect && failure.code == CONNECTED {
            self.ready = Some(Ok(0));
            Ok(())
        } else {
            Err(failure)
        }
    }
    fn poll(&mut self, handle: Handle) -> Result<Option<usize>> {
        if let Some(result) = self.ready.take() {
            return result.map(Some);
        }
        if !self.pending {
            return Ok(None);
        }
        let mut count = 0;
        let ok = unsafe { GetOverlappedResult(handle, &mut self.overlap, &mut count, 0) };
        if ok == 0 {
            let failure = err("GetOverlappedResult");
            if failure.code == INCOMPLETE {
                return Ok(None);
            }
            // Defined terminal outcomes of pipe operations. Unknown API failures
            // do not certify that the kernel stopped borrowing the operation.
            if !matches!(failure.code, 995 | 109 | 232 | 233) {
                return Err(failure);
            }
            self.pending = false;
            return Err(failure);
        }
        self.pending = false;
        if count as usize > self.requested {
            return Err(invariant("completion count"));
        }
        Ok(Some(count as usize))
    }
}

/// One local duplex byte pipe with one fixed RX and TX operation. Not Send/Sync:
/// creation, submission, polling and retirement belong to its one OS worker.
pub struct Pipe<const N: usize> {
    handle: Option<Owned>,
    read: Option<Box<Operation<N>>>,
    write: Option<Box<Operation<N>>>,
    connecting: bool,
    canceled: bool,
    server: bool,
    #[cfg(feature = "fixture-probes")]
    write_quantum: usize,
}
impl<const N: usize> Pipe<N> {
    fn from_handle(handle: Owned, server: bool) -> Result<Self> {
        if !matches!(N, 4096 | 65536) {
            return Err(invariant("pipe profile buffer size"));
        }
        Ok(Self {
            handle: Some(handle),
            read: Some(Operation::new()?),
            write: Some(Operation::new()?),
            connecting: false,
            canceled: false,
            server,
            #[cfg(feature = "fixture-probes")]
            write_quantum: N,
        })
    }
    /// First instance, local-only, owner-only ACL, non-inheritable overlapped pipe.
    pub fn server(name_value: &str) -> Result<Self> {
        if !matches!(N, 4096 | 65536) {
            return Err(invariant("pipe profile buffer size"));
        }
        let name = name(name_value)?;
        let descriptor = owner_descriptor()?;
        let security = Security {
            size: size_of::<Security>() as u32,
            descriptor: descriptor.0,
            inherit: 0,
        };
        let handle = Owned::new(unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                (if N == 65536 { 2 } else { 3 }) | 0x40000000 | 0x80000,
                8,
                1,
                N as u32,
                N as u32,
                0,
                &security,
            )
        })?;
        verify_owner_acl(handle.0, &descriptor)?;
        let mut pipe = Self::from_handle(handle, true)?;
        let read = pipe.read.as_mut().unwrap();
        read.reset(0)?;
        let ok = unsafe { ConnectNamedPipe(pipe.handle.as_ref().unwrap().0, &mut read.overlap) };
        read.submitted(ok, true)?;
        pipe.connecting = true;
        Ok(pipe)
    }
    /// Opens an existing local pipe once. No WaitNamedPipe, retry or fallback.
    /// Security SQOS identification prevents server impersonation of this client.
    pub fn client(name_value: &str) -> Result<Self> {
        if !matches!(N, 4096 | 65536) {
            return Err(invariant("pipe profile buffer size"));
        }
        let name = name(name_value)?;
        let raw = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x80000000 | if N == 65536 { 0 } else { 2 },
                0,
                null(),
                3,
                0x40000000 | 0x100000 | 0x10000,
                null_mut(),
            )
        };
        Self::from_handle(Owned::new(raw)?, false)
    }
    /// Poll connection completion; authenticates the OS peer against the process
    /// identity retained by the caller, never opening any process by PID.
    pub fn connected(&mut self, expected_pid: u32) -> Result<bool> {
        if self.canceled {
            return Err(invariant("canceled pipe"));
        }
        if self.connecting {
            if self
                .read
                .as_mut()
                .unwrap()
                .poll(self.handle.as_ref().unwrap().0)?
                .is_none()
            {
                return Ok(false);
            }
            self.connecting = false;
        }
        let mut actual = 0;
        let handle = self.handle.as_ref().ok_or(invariant("retired pipe"))?.0;
        check(
            unsafe {
                if self.server {
                    GetNamedPipeClientProcessId(handle, &mut actual)
                } else {
                    GetNamedPipeServerProcessId(handle, &mut actual)
                }
            },
            "pipe peer identity",
        )?;
        if expected_pid == 0 || actual != expected_pid {
            return Err(invariant("wrong pipe peer"));
        }
        Ok(true)
    }
    /// Copy bounded bytes into the sole idle TX buffer and submit one operation.
    pub fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.write_with(bytes.len(), |buffer| {
            buffer.copy_from_slice(bytes);
            Ok(())
        })
    }
    /// Encode directly in the idle TX buffer; no extra assembly chunk is needed.
    pub fn write_with(
        &mut self,
        count: usize,
        fill: impl FnOnce(&mut [u8]) -> Result<()>,
    ) -> Result<()> {
        if self.canceled || self.connecting || count == 0 {
            return Err(invariant("pipe write state"));
        }
        if N == 65536 && !self.server {
            return Err(invariant("snapshot write direction"));
        }
        let op = self.write.as_mut().unwrap();
        op.reset(count)?;
        fill(&mut op.buffer[..count])?;
        #[cfg(feature = "fixture-probes")]
        let count = count.min(self.write_quantum);
        op.requested = count;
        let ok = unsafe {
            WriteFile(
                self.handle.as_ref().unwrap().0,
                op.buffer.as_ptr(),
                count as u32,
                null_mut(),
                &mut op.overlap,
            )
        };
        op.submitted(ok, false)
    }
    /// Receipt for a TX operation, including actual partial completion length.
    pub fn poll_write(&mut self) -> Result<Option<usize>> {
        self.write
            .as_mut()
            .unwrap()
            .poll(self.handle.as_ref().unwrap().0)
    }
    /// Resubmit a completed partial TX using the same fixed bytes.
    pub fn write_remainder(&mut self, offset: usize, count: usize) -> Result<()> {
        if N == 65536 && !self.server {
            return Err(invariant("snapshot write direction"));
        }
        if self.canceled || count == 0 || offset.checked_add(count).is_none_or(|end| end > N) {
            return Err(invariant("partial write bounds"));
        }
        let op = self.write.as_mut().unwrap();
        op.reset(count)?;
        #[cfg(feature = "fixture-probes")]
        let count = count.min(self.write_quantum);
        op.requested = count;
        let ok = unsafe {
            WriteFile(
                self.handle.as_ref().unwrap().0,
                op.buffer.as_ptr().add(offset),
                count as u32,
                null_mut(),
                &mut op.overlap,
            )
        };
        op.submitted(ok, false)
    }
    /// Submit one bounded RX operation into storage owned until its receipt.
    pub fn read(&mut self, count: usize) -> Result<()> {
        self.read_at(0, count)
    }
    /// Append a partial frame in the same RX allocation after its previous receipt.
    pub fn read_at(&mut self, offset: usize, count: usize) -> Result<()> {
        if self.canceled
            || self.connecting
            || count == 0
            || offset.checked_add(count).is_none_or(|end| end > N)
        {
            return Err(invariant("pipe read state"));
        }
        if N == 65536 && self.server {
            return Err(invariant("snapshot read direction"));
        }
        let op = self.read.as_mut().unwrap();
        op.reset(count)?;
        op.offset = offset;
        let ok = unsafe {
            ReadFile(
                self.handle.as_ref().unwrap().0,
                op.buffer.as_mut_ptr().add(offset),
                count as u32,
                null_mut(),
                &mut op.overlap,
            )
        };
        op.submitted(ok, false)
    }
    /// Receipt and immutable completed bytes. The borrow prevents resubmission.
    pub fn poll_read(&mut self) -> Result<Option<&[u8]>> {
        let op = self.read.as_mut().unwrap();
        Ok(op
            .poll(self.handle.as_ref().unwrap().0)?
            .map(|count| &op.buffer[op.offset..op.offset + count]))
    }
    /// Borrow previously completed RX bytes, never memory involved in pending I/O.
    pub fn received(&self, count: usize) -> Result<&[u8]> {
        let op = self.read.as_ref().ok_or(invariant("retired pipe"))?;
        if op.pending || count > N {
            return Err(invariant("borrowed receive buffer"));
        }
        Ok(&op.buffer[..count])
    }
    /// Number of currently borrowed operations (including connection setup).
    pub fn pending(&self) -> usize {
        usize::from(self.read.as_ref().is_some_and(|op| op.pending))
            + usize::from(self.write.as_ref().is_some_and(|op| op.pending))
    }
    /// Owned endpoint and operation event handles, including retirement pending.
    pub fn handles(&self) -> usize {
        usize::from(self.handle.as_ref().is_some_and(|h| !h.0.is_null()))
            + usize::from(self.read.as_ref().is_some_and(|op| !op.event.0.is_null()))
            + usize::from(self.write.as_ref().is_some_and(|op| !op.event.0.is_null()))
    }
    /// Fixed encoded RX/TX storage retained by this endpoint, excluding metadata.
    pub fn buffer_bytes(&self) -> usize {
        self.read.as_ref().map_or(0, |op| op.buffer.len())
            + self.write.as_ref().map_or(0, |op| op.buffer.len())
    }
    /// Probe-only short OS submissions. Receipts still come from the original
    /// OVERLAPPED; no completion or transferred-byte count is fabricated.
    #[cfg(feature = "fixture-probes")]
    pub fn fixture_write_quantum(&mut self, count: usize) -> Result<()> {
        if count == 0 || count > N || self.write.as_ref().is_some_and(|op| op.pending) {
            return Err(invariant("fixture write quantum"));
        }
        self.write_quantum = count;
        Ok(())
    }
    /// Request cancellation once. Success and ERROR_NOT_FOUND are not receipts.
    pub fn cancel(&mut self) -> Result<()> {
        if self.canceled {
            return Ok(());
        }
        self.canceled = true;
        if let Some(handle) = &self.handle {
            let ok = unsafe { CancelIoEx(handle.0, null_mut()) };
            if ok == 0 {
                let failure = err("CancelIoEx");
                if failure.code != 1168 {
                    return Err(failure);
                }
            }
        }
        Ok(())
    }
    /// Poll every original operation. Release endpoint/events only after all
    /// receipts; false retains all buffers and handles for another bounded poll.
    pub fn retire(&mut self) -> Result<bool> {
        self.cancel()?;
        let Some(handle) = &mut self.handle else {
            return Ok(true);
        };
        for op in [&mut self.read, &mut self.write].into_iter().flatten() {
            if let Err(e) = op.poll(handle.0)
                && op.pending
            {
                return Err(e);
            }
        }
        if self.pending() != 0 {
            return Ok(false);
        }
        self.handle.as_mut().unwrap().close()?;
        for op in [&mut self.read, &mut self.write].into_iter().flatten() {
            op.event.close()?;
        }
        self.read = None;
        self.write = None;
        self.handle = None;
        Ok(true)
    }
}
impl<const N: usize> Drop for Pipe<N> {
    fn drop(&mut self) {
        if self.retire() != Ok(true) {
            // A detached root may disappear during panic. Leak a bounded episode
            // instead of freeing memory still borrowed by the Windows kernel.
            std::mem::forget(self.handle.take());
            std::mem::forget(self.read.take());
            std::mem::forget(self.write.take());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    fn until<T>(mut poll: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(value) = poll() {
                return value;
            }
            assert!(Instant::now() < deadline, "native receipt deadline");
            std::thread::yield_now();
        }
    }
    #[test]
    fn native_first_instance_peer_and_cancellation_receipts() {
        let name = format!(r"\\.\pipe\lab-workbench-capsule-{}", std::process::id());
        let mut server = Pipe::<4096>::server(&name).unwrap();
        assert_eq!(server.pending(), 1);
        assert!(Pipe::<4096>::server(&name).is_err());
        let mut client = Pipe::<4096>::client(&name).unwrap();
        until(|| server.connected(std::process::id()).unwrap().then_some(()));
        assert!(client.connected(std::process::id()).unwrap());
        assert!(server.connected(std::process::id() + 1).is_err());
        server.read(4096).unwrap();
        assert_eq!(server.pending(), 1);
        client.write(b"original payload").unwrap();
        until(|| client.poll_write().unwrap());
        let received = until(|| server.poll_read().unwrap().map(|b| b.to_vec()));
        assert_eq!(received, b"original payload");
        server.read(4096).unwrap();
        server.cancel().unwrap();
        assert_eq!(
            server.pending(),
            1,
            "CancelIoEx is not our completion observation"
        );
        assert!(server.received(1).is_err());
        until(|| server.retire().unwrap().then_some(()));
        assert_eq!(server.pending(), 0);
        assert!(client.retire().unwrap());
    }
}
