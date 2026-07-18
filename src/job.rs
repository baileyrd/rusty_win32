//! Windows Job Objects — the primitive rush's own
//! `docs/WINDOWS_JOB_CONTROL.md` designs background-job tracking against
//! (`&`, `jobs`, `wait`, `kill`, `$!`). A Job Object groups processes for
//! lifetime management, the closest Windows analog of a POSIX process group
//! for this purpose (not a perfect match — see that design doc's
//! "Terminology" note). Assigning a suspended process (see
//! [`crate::process::spawn_suspended`]) to a job *before* resuming its main
//! thread, with [`set_kill_on_close`] applied, means closing the job handle
//! (e.g. on shell exit) kills every process in it — the Windows analog of
//! the process-group-wide cleanup rush's Unix `job.rs` gets from signals.
//! Any child the job's own member processes spawn inherits job membership
//! automatically (a job property, not something a child opts into), which
//! is what makes this correct for a whole subtree, not just one process.

use crate::error::Win32Error;
use crate::handle::RawHandle;

extern crate alloc;
use alloc::vec::Vec;

/// `SetInformationJobObject`'s `LimitFlags` bit: close the job handle (or
/// have every handle to it close, e.g. on process exit) and every process
/// still assigned to it terminates.
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

/// `QueryInformationJobObject`/`SetInformationJobObject`'s
/// `JobObjectExtendedLimitInformation` class.
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;
/// `QueryInformationJobObject`'s `JobObjectBasicProcessIdList` class.
const JOB_OBJECT_BASIC_PROCESS_ID_LIST_CLASS: u32 = 3;

// Layouts below verified against mingw-w64's `winnt.h` (`_IO_COUNTERS`,
// `_JOBOBJECT_BASIC_LIMIT_INFORMATION`, `_JOBOBJECT_EXTENDED_LIMIT_INFORMATION`,
// `_JOBOBJECT_BASIC_PROCESS_ID_LIST`) the same way as `process.rs`'s
// structs: a `_Static_assert`-based probe compiled against the real header,
// not hand-computed padding.

/// `IO_COUNTERS`: `size_of` 48, `align_of` 8 on x86_64.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}
const _: () = assert!(core::mem::size_of::<IoCounters>() == 48);
const _: () = assert!(core::mem::align_of::<IoCounters>() == 8);

/// `JOBOBJECT_BASIC_LIMIT_INFORMATION`: `size_of` 64, `align_of` 8.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}
const _: () = assert!(core::mem::size_of::<JobObjectBasicLimitInformation>() == 64);
const _: () = assert!(core::mem::align_of::<JobObjectBasicLimitInformation>() == 8);
const _: () = assert!(core::mem::offset_of!(JobObjectBasicLimitInformation, limit_flags) == 16);

/// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`: `size_of` 144, `align_of` 8.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}
const _: () = assert!(core::mem::size_of::<JobObjectExtendedLimitInformation>() == 144);
const _: () = assert!(core::mem::align_of::<JobObjectExtendedLimitInformation>() == 8);
const _: () = assert!(core::mem::offset_of!(JobObjectExtendedLimitInformation, io_info) == 64);
const _: () =
    assert!(core::mem::offset_of!(JobObjectExtendedLimitInformation, process_memory_limit) == 112);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(job_attributes: *const core::ffi::c_void, name: *const u16) -> RawHandle;
    fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
    fn TerminateJobObject(job: RawHandle, exit_code: u32) -> i32;
    fn SetInformationJobObject(
        job: RawHandle,
        job_object_information_class: u32,
        job_object_information: *const core::ffi::c_void,
        job_object_information_length: u32,
    ) -> i32;
    fn QueryInformationJobObject(
        job: RawHandle,
        job_object_information_class: u32,
        job_object_information: *mut core::ffi::c_void,
        job_object_information_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

/// Create a new, anonymous Job Object — one shell background job's worth of
/// process-group-equivalent tracking. No process is assigned yet; pair with
/// [`assign`] (before resuming a suspended process — see
/// [`crate::process::spawn_suspended`]) and, almost always,
/// [`set_kill_on_close`].
pub fn create() -> Result<RawHandle, Win32Error> {
    // SAFETY: both arguments are documented-valid NULLs (default security
    // attributes, anonymous/unnamed job).
    let job = unsafe { CreateJobObjectW(core::ptr::null(), core::ptr::null()) };
    if job.is_null() {
        Err(Win32Error::last())
    } else {
        Ok(job)
    }
}

/// Set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` on `job`: every process still
/// assigned to it terminates when its last handle closes, e.g. via
/// [`crate::handle::close`] on shell exit — the Windows analog of the
/// process-group-wide cleanup rush's Unix `job.rs` gets from signals.
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn set_kill_on_close(job: RawHandle) -> Result<(), Win32Error> {
    let info = JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation {
            limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: `job` is caller-supplied per this function's own safety
    // contract; `info` is a valid, correctly-sized, initialized struct of
    // exactly the type `JobObjectExtendedLimitInformation` names.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&info as *const JobObjectExtendedLimitInformation).cast(),
            core::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Assign `process` to `job`. Any child `process` itself later spawns
/// inherits job membership automatically. Must happen before a
/// `CREATE_SUSPENDED`-started `process` is resumed (see
/// [`crate::process::spawn_suspended`]/[`crate::process::resume`]) for job
/// membership to be guaranteed before anything in the process tree runs.
///
/// # Safety
///
/// `job` and `process` must both be currently-open, valid handles.
pub unsafe fn assign(job: RawHandle, process: RawHandle) -> Result<(), Win32Error> {
    // SAFETY: both handles are caller-supplied per this function's own
    // safety contract.
    let ok = unsafe { AssignProcessToJobObject(job, process) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// Terminate every process currently assigned to `job` with `exit_code` —
/// backs `kill %n`. Unlike closing the job handle with
/// [`set_kill_on_close`] set, this doesn't require giving up the handle.
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn terminate(job: RawHandle, exit_code: u32) -> Result<(), Win32Error> {
    // SAFETY: `job` is caller-supplied per this function's own safety
    // contract.
    let ok = unsafe { TerminateJobObject(job, exit_code) };
    if ok == 0 {
        Err(Win32Error::last())
    } else {
        Ok(())
    }
}

/// The pids of every process currently assigned to `job` — one way to poll
/// "is this job still running" (an empty result) for `wait`/`jobs`, in the
/// absence of a blocking wait-for-job-completion primitive as simple as
/// Unix `SIGCHLD` (see rush's `docs/WINDOWS_JOB_CONTROL.md` for the
/// polling-vs-completion-port discussion).
///
/// # Safety
///
/// `job` must be a currently-open, valid Job Object handle.
pub unsafe fn process_ids(job: RawHandle) -> Result<Vec<usize>, Win32Error> {
    const HEADER_LEN: usize = 8; // NumberOfAssignedProcesses + NumberOfProcessIdsInList
    let mut capacity: u32 = 32;
    loop {
        let buf_len = HEADER_LEN + capacity as usize * core::mem::size_of::<usize>();
        let mut buf: Vec<u8> = alloc::vec![0u8; buf_len];
        let mut returned_len: u32 = 0;
        // SAFETY: `job` is caller-supplied per this function's own safety
        // contract; `buf` is a valid, zeroed, `buf_len`-byte buffer;
        // `returned_len` is a valid out-pointer.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JOB_OBJECT_BASIC_PROCESS_ID_LIST_CLASS,
                buf.as_mut_ptr().cast(),
                buf_len as u32,
                &mut returned_len,
            )
        };
        if ok == 0 {
            let err = Win32Error::last();
            if err == Win32Error::ERROR_MORE_DATA {
                // NumberOfAssignedProcesses is documented to be filled in
                // correctly even when the list itself didn't fit.
                // SAFETY: `buf` holds at least `HEADER_LEN` initialized
                // bytes even on this "too small" failure path.
                let assigned = unsafe { core::ptr::read_unaligned(buf.as_ptr().cast::<u32>()) };
                capacity = assigned.max(capacity * 2);
                continue;
            }
            return Err(err);
        }
        // SAFETY: a successful call guarantees the header and
        // `number_of_process_ids_in_list` pid-sized entries after it are
        // initialized.
        let in_list =
            unsafe { core::ptr::read_unaligned(buf.as_ptr().add(4).cast::<u32>()) } as usize;
        let pids_ptr = unsafe { buf.as_ptr().add(HEADER_LEN).cast::<usize>() };
        let pids = unsafe { core::slice::from_raw_parts(pids_ptr, in_list) };
        return Ok(pids.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process;

    #[test]
    fn create_terminate_and_kill_on_close_round_trip() {
        let job = create().expect("CreateJobObjectW should succeed");
        // SAFETY: `job` is freshly created and valid.
        unsafe { set_kill_on_close(job).expect("SetInformationJobObject should succeed") };

        // SAFETY: a hand-built, correctly quoted command line for a
        // well-known long-running system command — this test terminates it
        // via the job before it would exit on its own.
        let spawned =
            unsafe { process::spawn_suspended("cmd.exe /c ping -n 30 127.0.0.1 >nul", true) }
                .expect("CreateProcessW should succeed");

        // SAFETY: `job`/`spawned.process` are both freshly created, valid
        // handles; assignment happens before `resume`, so job membership is
        // guaranteed before the process runs.
        unsafe { assign(job, spawned.process).expect("AssignProcessToJobObject should succeed") };
        // SAFETY: `spawned.thread` is a freshly created, valid,
        // not-yet-resumed thread handle.
        unsafe { process::resume(spawned.thread).expect("ResumeThread should succeed") };

        // SAFETY: `job` is a valid handle with exactly one assigned process.
        let ids_before =
            unsafe { process_ids(job) }.expect("QueryInformationJobObject should succeed");
        assert_eq!(ids_before, alloc::vec![spawned.process_id as usize]);

        // SAFETY: `job` is a valid handle; this is the operation under
        // test.
        unsafe { terminate(job, 1).expect("TerminateJobObject should succeed") };

        // SAFETY: `spawned.process` is still a valid handle — terminating
        // via the job doesn't invalidate the handle, only the process.
        let exit = unsafe { process::wait(spawned.process, Some(5_000)) }.unwrap();
        assert_eq!(exit, Some(1));

        // SAFETY: both handles are valid and each closed exactly once.
        unsafe {
            crate::handle::close(spawned.process).unwrap();
            crate::handle::close(spawned.thread).unwrap();
            crate::handle::close(job).unwrap();
        }
    }
}
