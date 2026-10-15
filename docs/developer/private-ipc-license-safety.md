# Private Windows IPC capsule: license and safety audit

DGUI-4C7B3B3 extends the existing internal, unpublished `lab-windows-job`
capsule. No Cargo dependency, package version, toolchain, SDK installation or
third-party license changes. Kernel32 and Advapi32 are existing Windows system
components. Workbench retains workspace `unsafe_code=forbid`.

The new [pipe capsule](../../crates/lab-windows-job/src/pipe.rs) is the only
additional unsafe module. Its ABI is Windows x64/MSVC; OVERLAPPED is 32 bytes,
SECURITY_ATTRIBUTES is 24 bytes. Token information uses a fixed aligned 1,024-byte
stack buffer. SDK-created security descriptors/SID strings use checked outcomes
and LocalFree ownership. Pipe creation reads back the actual protected DACL and
requires one allow ACE for the explicit current-user owner. Default Everyone or
anonymous access is not accepted.

Every endpoint/event handle is uniquely owned and checked non-inheritable.
Snapshot endpoints enforce outbound-server/inbound-child access; control is
duplex. Names are local, first-instance creation is mandatory, remote clients are
rejected. The parent compares the pipe peer with the PID retained by the original
Job creation record; the child checks the original server PID. No process is
opened by PID. No raw handle is exported, inherited or duplicated into the child.
Existing JOB_LIST + CREATE_SUSPENDED creation and read-back checks are unchanged.

Each OVERLAPPED and buffer occupies stable owned storage. One RX and one TX
operation are allowed per pipe. Reset/resubmission requires an observed original
completion. All read/write counts are checked against the fixed 4 KiB/64 KiB
profile before submission; completed counts cannot exceed the original request.
No unchecked declared protocol length reaches the FFI capsule.

CancelIoEx is requested once. Its success and ERROR_NOT_FOUND are not receipts.
GetOverlappedResult with `wait=false` supplies original completion observations.
Unknown API failures retain ownership. Checked endpoint/event release follows
all observations. An unexpectedly dropped unfinished capsule retains its bounded
handle/OVERLAPPED/buffer allocation rather than freeing kernel-borrowed memory.
The normal supervisor retains the same episode and prohibits replacement.

Probe-only short submissions still use real original OVERLAPPED receipts; no
completion count is fabricated. Test gates can withhold observation without
changing production API. No worker is created to evade an unresponsive OS call.

Microsoft contracts: [pipe creation and advisory kernel buffers](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea),
[pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights),
[peer identity](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getnamedpipeclientprocessid),
[security readback](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo),
and [cancellation ownership](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex).
This trusted same-user child is not a security sandbox. No universal OS completion
latency, exact kernel allocation ceiling or GPU teardown receipt is claimed.
