//! Opt-in bounded MP-1 diagnostics; no production policy or raw handle export.
//! Snapshots are sequential API reads, not an atomic cross-API measurement.
#![allow(unsafe_code)]

use super::*;

/// Independently queried PROCESS_MEMORY_COUNTERS_EX, all sizes in bytes.
#[derive(Debug, Default)]
#[repr(C)]
pub struct ProcessMemory {
    /// Structure size supplied to K32GetProcessMemoryInfo.
    pub cb: u32,
    /// Page fault count, not bytes.
    pub faults: u32,
    /// Peak resident working set.
    pub peak_working_set: usize,
    /// Current resident working set.
    pub working_set: usize,
    /// Peak paged pool quota.
    pub peak_paged_pool: usize,
    /// Current paged pool quota.
    pub paged_pool: usize,
    /// Peak nonpaged pool quota.
    pub peak_nonpaged_pool: usize,
    /// Current nonpaged pool quota.
    pub nonpaged_pool: usize,
    /// Current process commit charge (historical API field name PagefileUsage).
    pub pagefile: usize,
    /// Peak process commit charge (PeakPagefileUsage).
    pub peak_pagefile: usize,
    /// Private commit charge.
    pub private: usize,
}

#[repr(C)]
#[derive(Default)]
struct Accounting {
    times: [i64; 4],
    faults: u32,
    total: u32,
    active: u32,
    terminated: u32,
}
#[repr(C)]
#[derive(Default)]
struct Pids {
    assigned: u32,
    count: u32,
    ids: [usize; 8],
}

/// Checked exact-Job accounting and original-process counters.
#[derive(Debug)]
pub struct Snapshot {
    /// Extended information returned byte count.
    pub extended_bytes: u32,
    /// Basic accounting information returned byte count.
    pub accounting_bytes: u32,
    /// Process ID list returned byte count.
    pub pid_bytes: u32,
    /// Active process limit read back (OS helpers can still appear).
    pub active_limit: u32,
    /// Limit flags read back from the kernel.
    pub flags: u32,
    /// Process cap in bytes.
    pub process_limit: usize,
    /// Aggregate cap in bytes.
    pub job_limit: usize,
    /// Job API process peak, bytes.
    pub peak_process: usize,
    /// Job API aggregate peak, bytes.
    pub peak_job: usize,
    /// Lifetime associated process count.
    pub total: u32,
    /// Currently active process count.
    pub active: u32,
    /// Limit-terminated count (not all ordinary exits).
    pub terminated: u32,
    /// Current associated PIDs, bounded to eight diagnostic entries.
    pub pids: Vec<usize>,
    /// Diagnostic members opened read-only and rechecked against this exact Job.
    /// These handles are never used as original-process exit or restart evidence.
    pub members: Vec<Member>,
    /// Original process membership checked with IsProcessInJob.
    pub member: bool,
    /// Independent counters; an API error remains explicit after exit.
    pub process: Result<ProcessMemory>,
}

/// Independently identified current Job member, with explicit query failures.
#[derive(Debug)]
pub struct Member {
    /// Enumerated diagnostic PID, not a supervision identity.
    pub pid: usize,
    /// Image name and memory read from one short-lived, membership-checked handle.
    pub reading: Result<(String, ProcessMemory)>,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(
        process: Handle,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn K32GetProcessMemoryInfo(process: Handle, counters: *mut ProcessMemory, size: u32) -> i32;
    fn CreateIoCompletionPort(file: Handle, port: Handle, key: usize, threads: u32) -> Handle;
    fn GetQueuedCompletionStatus(
        port: Handle,
        bytes: *mut u32,
        key: *mut usize,
        overlapped: *mut *mut c_void,
        milliseconds: u32,
    ) -> i32;
    fn GetSystemInfo(info: *mut SystemInfo);
    fn VirtualQuery(address: *const c_void, info: *mut Region, bytes: usize) -> usize;
}
#[repr(C)]
struct Region {
    base: usize,
    allocation_base: usize,
    allocation_protection: u32,
    partition: u16,
    region_size: usize,
    state: u32,
    protection: u32,
    kind: u32,
}
#[repr(C)]
struct SystemInfo {
    architecture: u32,
    page_size: u32,
    min: usize,
    max: usize,
    mask: usize,
    processors: u32,
    processor_type: u32,
    granularity: u32,
    level: u16,
    revision: u16,
}
const _: () = assert!(size_of::<ProcessMemory>() == 80 && size_of::<Accounting>() == 48);
const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<Basic>() == 64 && size_of::<Extended>() == 144);
    assert!(offset_of!(Basic, flags) == 16 && offset_of!(Basic, active) == 40);
    assert!(offset_of!(Extended, process) == 112 && offset_of!(Extended, job) == 120);
    assert!(offset_of!(Extended, peak_process) == 128 && offset_of!(Extended, peak_job) == 136);
    assert!(offset_of!(ProcessMemory, private) == 72 && size_of::<SystemInfo>() == 48);
    assert!(size_of::<Region>() == 48 && offset_of!(Region, state) == 32);
};

fn query<T>(job: Handle, class: u32, value: &mut T) -> Result<u32> {
    let mut returned = 0;
    // SAFETY: caller supplies the exact repr(C) SDK buffer for this class.
    checked(
        unsafe {
            QueryInformationJobObject(
                job,
                class,
                (value as *mut T).cast(),
                size_of::<T>() as u32,
                &mut returned,
            )
        },
        "QueryInformationJobObject accounting",
    )?;
    Ok(returned)
}

fn process_memory(handle: Handle) -> Result<ProcessMemory> {
    let mut info = ProcessMemory {
        cb: size_of::<ProcessMemory>() as u32,
        ..ProcessMemory::default()
    };
    // SAFETY: valid process handle and exact writable counters buffer.
    checked(
        unsafe { K32GetProcessMemoryInfo(handle, &mut info, size_of::<ProcessMemory>() as u32) },
        "K32GetProcessMemoryInfo",
    )?;
    Ok(info)
}

fn member(job: Handle, pid: usize) -> Result<(String, ProcessMemory)> {
    // SAFETY: diagnostic QUERY_INFORMATION|VM_READ only, no inheritance.
    let raw = unsafe { OpenProcess(0x400 | 0x10, 0, pid as u32) };
    if raw.is_null() {
        return Err(error("OpenProcess diagnostic member"));
    }
    let mut owned = Owned(raw);
    let mut belongs = 0;
    // SAFETY: exact Job and newly owned diagnostic process handle.
    checked(
        unsafe { IsProcessInJob(raw, job, &mut belongs) },
        "IsProcessInJob diagnostic member",
    )?;
    if belongs == 0 {
        return Err(Error {
            operation: "diagnostic member changed",
            code: 0,
        });
    }
    let mut name = [0_u16; 1024];
    let mut length = name.len() as u32;
    // SAFETY: bounded writable UTF-16 image buffer for this handle.
    checked(
        unsafe { QueryFullProcessImageNameW(raw, 0, name.as_mut_ptr(), &mut length) },
        "QueryFullProcessImageNameW",
    )?;
    let reading = process_memory(raw)?;
    owned.close()?;
    Ok((String::from_utf16_lossy(&name[..length as usize]), reading))
}

/// Independent self counters for the CPU fixture, without opening a PID.
pub fn current_process_memory() -> Result<ProcessMemory> {
    // SAFETY: documented current process pseudo handle, not closed or exported.
    process_memory(unsafe { GetCurrentProcess() })
}

/// Whether the controller itself belongs to any outer Job (identity not inferred).
pub fn controller_in_job() -> Result<bool> {
    let mut member = 0;
    // SAFETY: current process and null Job ask about any Job membership.
    checked(
        unsafe { IsProcessInJob(GetCurrentProcess(), null_mut(), &mut member) },
        "IsProcessInJob controller",
    )?;
    Ok(member != 0)
}

impl Job {
    /// Sequential checked reads, using this Job and its original process handle.
    pub fn accounting_snapshot(&self) -> Result<Snapshot> {
        self.verify_membership()?;
        self.verify_limits()?;
        let mut extended = Extended::default();
        let bytes = query(self.handle.0, 9, &mut extended)?;
        if bytes as usize != size_of::<Extended>() {
            return Err(Error {
                operation: "extended returned size",
                code: bytes,
            });
        }
        let mut basic = Accounting::default();
        let accounting_bytes = query(self.handle.0, 1, &mut basic)?;
        if accounting_bytes as usize != size_of::<Accounting>() {
            return Err(Error {
                operation: "basic accounting returned size",
                code: accounting_bytes,
            });
        }
        let mut pids = Pids::default();
        let pid_bytes = query(self.handle.0, 3, &mut pids)?;
        if pids.count > 8 || pids.assigned != pids.count {
            return Err(Error {
                operation: "PID list bound",
                code: pids.count,
            });
        }
        Ok(Snapshot {
            extended_bytes: bytes,
            accounting_bytes,
            pid_bytes,
            active_limit: extended.basic.active,
            flags: extended.basic.flags,
            process_limit: extended.process,
            job_limit: extended.job,
            peak_process: extended.peak_process,
            peak_job: extended.peak_job,
            total: basic.total,
            active: basic.active,
            terminated: basic.terminated,
            pids: pids.ids[..pids.count as usize].to_vec(),
            members: pids.ids[..pids.count as usize]
                .iter()
                .map(|&pid| Member {
                    pid,
                    reading: member(self.handle.0, pid),
                })
                .collect(),
            member: true,
            process: process_memory(self.process.as_ref().unwrap().0),
        })
    }

    /// Associate an owned completion port before process creation. Job messages
    /// are best effort; an absent message is not proof a limit was not reached.
    pub fn accounting_notifications(&self) -> Result<Notifications> {
        // SAFETY: documented creation with INVALID_HANDLE_VALUE, no existing port.
        let raw = unsafe { CreateIoCompletionPort(usize::MAX as Handle, null_mut(), 0, 1) };
        if raw.is_null() {
            return Err(error("CreateIoCompletionPort"));
        }
        let owner = Owned(raw);
        owner.non_inherited()?;
        let association = [1_usize, raw as usize];
        // SAFETY: two pointer-sized fields of JOBOBJECT_ASSOCIATE_COMPLETION_PORT.
        checked(
            unsafe {
                SetInformationJobObject(
                    self.handle.0,
                    7,
                    association.as_ptr().cast(),
                    size_of_val(&association) as u32,
                )
            },
            "SetInformationJobObject completion port",
        )?;
        Ok(Notifications(owner))
    }
}

/// Diagnostic port owner, independent of the production Job/process ledger.
pub struct Notifications(Owned);
impl Notifications {
    /// Drain at most 64 zero-time messages; records message code and reported PID.
    pub fn drain(&self) -> Result<Vec<(u32, usize)>> {
        let mut messages = Vec::new();
        for _ in 0..64 {
            let (mut message, mut key, mut pid) = (0, 0, null_mut());
            // SAFETY: valid owned port and writable outputs; PID is never dereferenced.
            let ok =
                unsafe { GetQueuedCompletionStatus(self.0.0, &mut message, &mut key, &mut pid, 0) };
            if ok == 0 {
                let failure = error("GetQueuedCompletionStatus");
                if failure.code == 258 {
                    return Ok(messages);
                }
                return Err(failure);
            }
            if key != 1 {
                return Err(Error {
                    operation: "completion key mismatch",
                    code: 0,
                });
            }
            messages.push((message, pid as usize));
        }
        Err(Error {
            operation: "completion message bound",
            code: 0,
        })
    }
    /// Explicit CloseHandle receipt, after Job retirement in the probe controller.
    pub fn close(&mut self) -> Result<()> {
        self.0.close()
    }
}

/// A bounded private address reservation, never a GPU allocator.
pub struct Reservation {
    address: *mut c_void,
    bytes: usize,
    committed: usize,
}
impl Reservation {
    /// Reserve up to 256 MiB without committing or accessing pages.
    pub fn reserve(bytes: usize) -> Result<Self> {
        if bytes == 0 || bytes > 256 * 1024 * 1024 || !bytes.is_multiple_of(65536) {
            return Err(Error {
                operation: "reservation bound",
                code: 0,
            });
        }
        // SAFETY: bounded anonymous MEM_RESERVE; no pointer exposed.
        let address = unsafe { VirtualAlloc(null_mut(), bytes, 0x2000, 4) };
        if address.is_null() {
            return Err(error("VirtualAlloc MEM_RESERVE"));
        }
        Ok(Self {
            address,
            bytes,
            committed: 0,
        })
    }
    /// Commit the next bounded, page-aligned region without touching it.
    pub fn commit(&mut self, bytes: usize) -> Result<()> {
        if self.address.is_null()
            || bytes == 0
            || !bytes.is_multiple_of(65536)
            || bytes > self.bytes - self.committed
        {
            return Err(Error {
                operation: "commit bound",
                code: 0,
            });
        }
        // SAFETY: checked offset/length within this live reservation.
        let address = unsafe { self.address.cast::<u8>().add(self.committed).cast() };
        // SAFETY: MEM_COMMIT within this reservation; last error captured immediately.
        let result = unsafe { VirtualAlloc(address, bytes, 0x1000, 4) };
        if result.is_null() {
            return Err(error("VirtualAlloc MEM_COMMIT"));
        }
        self.committed += bytes;
        Ok(())
    }
    /// VirtualQuery state/size at the first uncommitted address. A failed commit
    /// leaves MEM_RESERVE (0x2000), independently of the high-water counters.
    pub fn next_region(&self) -> Result<(u32, usize)> {
        if self.address.is_null() || self.committed == self.bytes {
            return Err(Error {
                operation: "next region bound",
                code: 0,
            });
        }
        // SAFETY: checked address within this live reservation; exact SDK output.
        let mut info: Region = unsafe { zeroed() };
        // SAFETY: querying an owned mapped address does not access page contents.
        let bytes = unsafe {
            VirtualQuery(
                self.address.cast::<u8>().add(self.committed).cast(),
                &mut info,
                size_of::<Region>(),
            )
        };
        if bytes != size_of::<Region>() {
            return Err(error("VirtualQuery"));
        }
        Ok((info.state, info.region_size))
    }
    /// Write one byte per system page, only within successfully committed memory.
    pub fn touch(&mut self) {
        assert!(!self.address.is_null(), "touch after reservation release");
        // SAFETY: zero-initialized SDK output, then GetSystemInfo fills it.
        let mut info: SystemInfo = unsafe { zeroed() };
        // SAFETY: exact writable SYSTEM_INFO buffer.
        unsafe { GetSystemInfo(&mut info) };
        assert!(info.page_size >= 4096 && info.page_size.is_power_of_two());
        for offset in (0..self.committed).step_by(info.page_size as usize) {
            // SAFETY: offset strictly inside successfully committed private pages.
            unsafe { self.address.cast::<u8>().add(offset).write_volatile(1) };
        }
    }
    /// Decommit all committed pages while keeping the address reservation.
    pub fn decommit(&mut self) -> Result<()> {
        if self.committed != 0 {
            // SAFETY: exact owned live committed prefix, MEM_DECOMMIT.
            checked(
                unsafe { VirtualFree(self.address, self.committed, 0x4000) },
                "VirtualFree MEM_DECOMMIT",
            )?;
            self.committed = 0;
        }
        Ok(())
    }
    /// Release the complete address reservation, clearing ownership on success.
    pub fn release(&mut self) -> Result<()> {
        if !self.address.is_null() {
            // SAFETY: original reservation base and MEM_RELEASE size zero.
            checked(
                unsafe { VirtualFree(self.address, 0, 0x8000) },
                "VirtualFree MEM_RELEASE",
            )?;
            self.address = null_mut();
            self.committed = 0;
        }
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Err(e) = self.release() {
            eprintln!("reservation release unconfirmed: {e:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Reservation;

    #[test]
    fn released_reservation_rejects_commit_before_pointer_arithmetic() {
        let mut reservation = Reservation::reserve(65536).unwrap();
        reservation.release().unwrap();
        let error = reservation.commit(65536).unwrap_err();
        assert_eq!(error.operation, "commit bound");
        reservation.decommit().unwrap();
        reservation.release().unwrap();
    }
}
