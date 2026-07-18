// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Linux file-lease synchronization for deterministic copy-ordering tests.

use std::fs::{
    File,
    OpenOptions,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::marker::PhantomData;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::rc::Rc;
use std::time::{
    Duration,
    Instant,
};

use super::file_owner_ex_tests::FileOwnerEx;

/// Linux `F_SETOWN_EX` command number from `<fcntl.h>`.
const F_SETOWN_EX: libc::c_int = 15;

/// Linux `F_OWNER_TID` owner type from `<fcntl.h>`.
const F_OWNER_TID: libc::c_int = 0;

/// Maximum time to wait for the recursive-copy worker to open its source.
const LEASE_BREAK_TIMEOUT: Duration = Duration::from_secs(5);

/// A Linux write lease that blocks another thread from opening a source file.
///
/// The lease-break `SIGIO` is targeted to and blocked on the creating thread.
/// [`Self::wait_for_break`] can therefore synchronously detect the exact
/// source-open attempt without a process-wide signal handler or filesystem
/// polling.
pub(crate) struct SourceReadLease {
    /// File description that owns the lease.
    file: Option<File>,
    /// Signal set containing the blocked lease-break signal.
    signal_set: libc::sigset_t,
    /// Signal mask restored when the lease is released.
    previous_signal_mask: libc::sigset_t,
    /// Whether the lease and signal-mask state still require cleanup.
    active: bool,
    /// Keeps cleanup on the thread whose signal mask was changed.
    _not_send: PhantomData<Rc<()>>,
}

impl SourceReadLease {
    /// Acquires a write lease that blocks subsequent source-file readers.
    ///
    /// # Parameters
    ///
    /// * `path` - Existing regular file to lease.
    ///
    /// # Errors
    ///
    /// Returns the native I/O error reported while opening the file, blocking
    /// `SIGIO`, targeting the signal, or acquiring the lease.
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        // SAFETY: a zeroed sigset_t is immediately initialized by
        // sigemptyset before it is read by any other libc function.
        let mut signal_set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: `signal_set` is a valid writable sigset_t object.
        if unsafe { libc::sigemptyset(&mut signal_set) } == -1 {
            return Err(Error::last_os_error());
        }
        // SAFETY: `signal_set` was initialized by sigemptyset and SIGIO is a
        // valid signal number on Linux.
        if unsafe { libc::sigaddset(&mut signal_set, libc::SIGIO) } == -1 {
            return Err(Error::last_os_error());
        }
        // SAFETY: a zeroed sigset_t is a valid output buffer for
        // pthread_sigmask, which initializes it before it is read.
        let mut previous_signal_mask: libc::sigset_t =
            unsafe { std::mem::zeroed() };
        // SAFETY: both sigset_t pointers are valid for their documented input
        // and output roles for the duration of the call.
        let mask_result = unsafe {
            libc::pthread_sigmask(
                libc::SIG_BLOCK,
                &signal_set,
                &mut previous_signal_mask,
            )
        };
        if mask_result != 0 {
            return Err(Error::from_raw_os_error(mask_result));
        }

        // SAFETY: SYS_gettid takes no arguments and returns the current Linux
        // thread identifier without retaining any pointers.
        let thread_id = unsafe { libc::syscall(libc::SYS_gettid) };
        let thread_id = match libc::pid_t::try_from(thread_id) {
            Ok(thread_id) => thread_id,
            Err(_) => {
                restore_signal_mask(&previous_signal_mask);
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "thread identifier is invalid",
                ));
            }
        };
        let owner = FileOwnerEx {
            owner_type: F_OWNER_TID,
            pid: thread_id,
        };
        // SAFETY: the file descriptor is live and `owner` has the native
        // layout required by the Linux F_SETOWN_EX fcntl command.
        let owner_result = unsafe {
            libc::fcntl(file.as_raw_fd(), F_SETOWN_EX, &raw const owner)
        };
        if owner_result == -1 {
            let error = Error::last_os_error();
            restore_signal_mask(&previous_signal_mask);
            return Err(error);
        }
        // SAFETY: the file descriptor is a live regular file opened for
        // reading and writing, as required for a Linux write lease.
        let lease_result = unsafe {
            libc::fcntl(file.as_raw_fd(), libc::F_SETLEASE, libc::F_WRLCK)
        };
        if lease_result == -1 {
            let error = Error::last_os_error();
            restore_signal_mask(&previous_signal_mask);
            return Err(error);
        }
        Ok(Self {
            file: Some(file),
            signal_set,
            previous_signal_mask,
            active: true,
            _not_send: PhantomData,
        })
    }

    /// Waits until another thread attempts to open the leased source file.
    ///
    /// # Errors
    ///
    /// Returns the native signal-wait error, including a timeout when the
    /// source-open attempt does not occur within five seconds.
    pub(crate) fn wait_for_break(&self) -> Result<()> {
        let deadline = Instant::now() + LEASE_BREAK_TIMEOUT;
        loop {
            let timeout = duration_to_timespec(
                deadline.saturating_duration_since(Instant::now()),
            );
            // SAFETY: `signal_set` is initialized and remains live, the
            // optional signal-information pointer may be null, and `timeout`
            // is valid.
            let signal = unsafe {
                libc::sigtimedwait(
                    &self.signal_set,
                    std::ptr::null_mut(),
                    &timeout,
                )
            };
            if signal == libc::SIGIO {
                return Ok(());
            }
            if signal == -1 {
                let error = Error::last_os_error();
                if error.kind() == ErrorKind::Interrupted {
                    if Instant::now() >= deadline {
                        return Err(Error::from_raw_os_error(libc::EAGAIN));
                    }
                    continue;
                }
                return Err(error);
            }
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("unexpected lease-break signal: {signal}"),
            ));
        }
    }

    /// Releases the lease and restores the creating thread's signal mask.
    ///
    /// # Errors
    ///
    /// Returns the native error reported while releasing the lease or
    /// restoring the signal mask.
    pub(crate) fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    /// Performs idempotent lease and signal-mask cleanup.
    fn release_inner(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let lease_error = self.file.as_ref().and_then(|file| {
            // SAFETY: the file descriptor remains live and owns the lease
            // being released by the Linux F_SETLEASE command.
            let result = unsafe {
                libc::fcntl(
                    file_descriptor(file),
                    libc::F_SETLEASE,
                    libc::F_UNLCK,
                )
            };
            (result == -1).then(Error::last_os_error)
        });
        // Closing the descriptor also releases the lease if the explicit
        // unlock failed, so no later open can queue another lease-break
        // signal after cleanup starts.
        drop(self.file.take());
        let signal_error = drain_signal(&self.signal_set);
        let mask_error = restore_signal_mask(&self.previous_signal_mask);
        self.active = false;
        match (lease_error, signal_error, mask_error) {
            (Some(error), _, _)
            | (None, Some(error), _)
            | (None, None, Some(error)) => Err(error),
            (None, None, None) => Ok(()),
        }
    }
}

impl Drop for SourceReadLease {
    /// Best-effort releases the lease and restores the signal mask.
    fn drop(&mut self) {
        drop(self.release_inner());
    }
}

/// Returns the raw descriptor for a leased file.
fn file_descriptor(file: &File) -> libc::c_int {
    file.as_raw_fd()
}

/// Converts a relative Rust duration to the native signal-wait timeout.
fn duration_to_timespec(duration: Duration) -> libc::timespec {
    libc::timespec {
        tv_sec: libc::time_t::try_from(duration.as_secs())
            .unwrap_or(libc::time_t::MAX),
        tv_nsec: duration.subsec_nanos().into(),
    }
}

/// Reads CPU time consumed by the current Linux thread.
///
/// # Returns
///
/// CPU time charged to the calling thread.
///
/// # Errors
///
/// Returns the native clock error, or [`ErrorKind::InvalidData`] when the
/// returned native time cannot be represented as a Rust [`Duration`].
pub(crate) fn current_thread_cpu_time() -> Result<Duration> {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: `time` points to writable storage for one timespec and is read
    // only after clock_gettime reports success.
    let result = unsafe {
        libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, time.as_mut_ptr())
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    // SAFETY: a successful clock_gettime call initialized the complete value.
    let time = unsafe { time.assume_init() };
    let seconds = u64::try_from(time.tv_sec).map_err(|_| {
        Error::new(ErrorKind::InvalidData, "thread CPU seconds are negative")
    })?;
    let nanoseconds = u32::try_from(time.tv_nsec).map_err(|_| {
        Error::new(
            ErrorKind::InvalidData,
            "thread CPU nanoseconds are negative",
        )
    })?;
    if nanoseconds >= 1_000_000_000 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "thread CPU nanoseconds exceed one second",
        ));
    }
    Ok(Duration::new(seconds, nanoseconds))
}

/// Consumes all pending lease-break signals before they are unblocked.
fn drain_signal(signal_set: &libc::sigset_t) -> Option<Error> {
    let timeout = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    loop {
        // SAFETY: `signal_set` is initialized and remains live, the optional
        // signal-information pointer may be null, and `timeout` is valid.
        let signal = unsafe {
            libc::sigtimedwait(signal_set, std::ptr::null_mut(), &timeout)
        };
        if signal == libc::SIGIO {
            continue;
        }
        if signal == -1 {
            let error = Error::last_os_error();
            return match error.kind() {
                ErrorKind::Interrupted => continue,
                ErrorKind::WouldBlock => None,
                _ => Some(error),
            };
        }
        return Some(Error::new(
            ErrorKind::InvalidData,
            format!("unexpected pending lease-break signal: {signal}"),
        ));
    }
}

/// Restores a previously saved pthread signal mask.
fn restore_signal_mask(previous_signal_mask: &libc::sigset_t) -> Option<Error> {
    // SAFETY: `previous_signal_mask` was initialized by pthread_sigmask and
    // remains live for the duration of this call; no output mask is needed.
    let result = unsafe {
        libc::pthread_sigmask(
            libc::SIG_SETMASK,
            previous_signal_mask,
            std::ptr::null_mut(),
        )
    };
    (result != 0).then(|| Error::from_raw_os_error(result))
}
