//! Cooperative shutdown coordination for `SIGINT`, `SIGTERM`, and `SIGHUP`.
//!
//! On Unix, the default action for these signals is to terminate the
//! process *without unwinding the stack*, which means
//! [`Drop`](std::ops::Drop) impls — including
//! [`crate::storage::SqliteStorage::drop`] — never run, and WAL frames
//! that haven't been checkpointed yet are left stranded on disk
//! (issue #270).
//!
//! This module installs a small handler that translates those signals
//! into a single atomic "shutdown requested" flag, then lets the main
//! thread complete its current operation, return from `main`, and run
//! every destructor on the way out. If the user signals again while the
//! main thread is still inside a long operation we escalate to an
//! immediate `_exit`, matching the muscle-memory of "press Ctrl-C
//! twice."
//!
//! On Windows we currently rely on the default Ctrl-C behaviour and the
//! [`Drop`] / `panic = "abort"` interaction; the public surface here is
//! a no-op so callers don't need `cfg(unix)` at every call site.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Set when one of the registered termination signals has been
/// observed. Public callers should use [`is_requested`] /
/// [`exit_code`].
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// `128 + signo` of the signal that triggered the shutdown, encoding
/// the conventional Unix exit code. Stored as `i32` so the relaxed
/// load is wait-free; only the *first* signal wins, which keeps the
/// reported exit code stable when multiple signals race.
static SHUTDOWN_EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Tracks whether [`install`] has already wired the background thread,
/// so callers can invoke it safely from `main` without worrying about
/// double-registration in test harnesses or library re-entry.
static INSTALLED: OnceLock<()> = OnceLock::new();

/// Install signal handlers for `SIGINT`, `SIGTERM`, and `SIGHUP` (Unix
/// only). On non-Unix targets this is a no-op.
///
/// # Behaviour
///
/// * The first signal records the exit code `128 + signo` and flips
///   [`is_requested`]. The main thread is responsible for noticing the
///   flag at a safe checkpoint and returning from `main`.
/// * The second matching signal calls
///   [`signal_hook::low_level::exit`] (an async-signal-safe `_exit`
///   wrapper) immediately so a user can always escape a hung command
///   by hitting Ctrl-C twice.
///
/// Idempotent: subsequent calls return without re-installing.
pub fn install() {
    if INSTALLED.set(()).is_err() {
        return;
    }
    #[cfg(unix)]
    install_unix();
    install_pipe_tolerant_panic_hook();
}

/// Classify a panic payload as "the text output path failed to print
/// because stdout is a closed pipe" (beads_rust-3fna). Such panics are a
/// normal close, not a crash: the requested output was delivered up to the
/// point the reader stopped listening (`br list | head`), and standard CLI
/// convention is to exit 0 rather than abort with a core dump.
#[must_use]
fn is_stdout_broken_pipe_panic(payload: &(dyn std::any::Any + Send)) -> bool {
    let message: Option<&str> = if let Some(static_message) = payload.downcast_ref::<&'static str>()
    {
        Some(static_message)
    } else {
        payload.downcast_ref::<String>().map(String::as_str)
    };
    let Some(message) = message else {
        return false;
    };
    message.contains("failed printing to stdout")
        && (message.contains("Broken pipe")
            || message.contains("broken pipe")
            || message.contains("os error 32"))
}

/// Install a panic-hook shim that turns broken-stdout-print panics into a
/// clean `exit(0)`.
///
/// The Rust runtime ignores `SIGPIPE`, so `println!` on a closed pipe
/// returns `EPIPE`, which `println!` converts into a panic; under
/// `panic = "abort"` that becomes SIGABRT plus a core dump even though the
/// command is read-only and its output was already consumed by the reader
/// (`br list | head`). Every other panic is forwarded to the previously
/// installed hook (or default behavior) unchanged.
pub fn install_pipe_tolerant_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |payload| {
        if is_stdout_broken_pipe_panic(payload.payload()) {
            // Read-only text commands: the reader hung up. Exit like a
            // well-behaved filter instead of aborting (#434 /
            // beads_rust-3fna). exit() skips Drop/WAL flush, but so did the
            // abort it replaces, and these commands hold no write lock.
            std::process::exit(0);
        }
        previous(payload);
    }));
}

/// Returns `true` once any registered signal has been observed.
#[must_use]
pub fn is_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::Acquire)
}

/// Returns the conventional Unix exit code (`128 + signo`) for the
/// signal that triggered shutdown, or `None` if no signal has fired.
#[must_use]
pub fn exit_code() -> Option<i32> {
    let code = SHUTDOWN_EXIT_CODE.load(Ordering::Acquire);
    (code != 0).then_some(code)
}

#[cfg(unix)]
fn install_unix() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let mut signals = match Signals::new([SIGINT, SIGTERM, SIGHUP]) {
        Ok(signals) => signals,
        Err(err) => {
            // If we can't install a handler we fall back to default
            // signal action (process termination). Logging here keeps
            // the failure visible without aborting startup, since the
            // user's command is already in flight.
            tracing::warn!(
                error = %err,
                "failed to install shutdown signal handler; SIGTERM/SIGINT/SIGHUP \
                 will skip Drop and may strand WAL frames"
            );
            return;
        }
    };

    std::thread::Builder::new()
        .name("br-shutdown".to_string())
        .spawn(move || {
            for signo in signals.forever() {
                let exit = 128 + signo;
                // Publish in this exact order:
                //   1. Reserve the exit code via `compare_exchange`
                //      from 0 → `exit`. Only the first writer wins, so
                //      a re-entrant signal cannot overwrite the value
                //      a `main` thread reader is about to consume.
                //   2. Set the "requested" flag with `Release`
                //      ordering. Any reader that observes the flag set
                //      via an `Acquire` load is therefore guaranteed
                //      to also see the matching exit code (Step 1
                //      happens-before Step 2 by program order, and
                //      the Release on Step 2 publishes both writes
                //      together).
                //
                // Reversing this order would let `is_requested()`
                // return true while `exit_code()` still saw the
                // initial 0, which would cause a racing main thread
                // to silently miss the signal.
                let was_first = SHUTDOWN_EXIT_CODE
                    .compare_exchange(0, exit, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok();
                SHUTDOWN_REQUESTED.store(true, Ordering::Release);
                if !was_first {
                    // Second strike: bypass main, accept that any
                    // remaining WAL frames are forfeit — the user
                    // explicitly asked to bail out now.
                    // `signal_hook::low_level::exit` wraps `_exit`
                    // and is async-signal-safe for exactly this case.
                    signal_hook::low_level::exit(exit);
                }
            }
        })
        .map(drop)
        .unwrap_or_else(|err| {
            tracing::warn!(
                error = %err,
                "failed to spawn br-shutdown thread; falling back to default signal action"
            );
        });
}

/// Terminate the process with `code`, guaranteeing the exit code survives
/// teardown. This is the single exit funnel for every deliberate process
/// exit; call it only after the caller has dropped (or deliberately
/// forfeited) any [`crate::storage::SqliteStorage`] whose WAL should be
/// checkpointed, exactly as with [`std::process::exit`].
///
/// # Why not `std::process::exit` on Windows (GitHub #439)
///
/// On Windows, `std::process::exit` reaches the CRT `exit()` path, which
/// eventually calls `ExitProcess`. `ExitProcess` first terminates every
/// other thread in the process and only then runs atexit callbacks and
/// TLS/FLS destructors on the calling thread. Any of those destructors that
/// joins one of the just-terminated threads finds a thread that never ran
/// its Rust epilogue, which trips std's `JoinInner::join` integrity check
/// ("threads should not terminate unexpectedly") and aborts with
/// `0xC0000409` — corrupting the exit code of otherwise-successful
/// commands. `TerminateProcess` on our own handle sets the exit code and
/// skips that teardown entirely. Rust destructors were never going to run
/// on this path anyway (they don't under `std::process::exit` either), so
/// the only cleanup we owe here is flushing the std stream buffers, which
/// this function does on every platform before exiting.
// The single `unsafe` block below is the `#439` Windows `TerminateProcess`
// carve-out sanctioned in Cargo.toml's `[lints.rust]` note (alongside
// `sync::db_inode_lock` and `restore_default_sigpipe_unix`); without this
// attribute the crate-level `#![deny(unsafe_code)]` fails every
// `x86_64-pc-windows-msvc` build.
#[allow(unsafe_code)]
pub fn exit_process(code: i32) -> ! {
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    #[cfg(windows)]
    {
        // SAFETY: `GetCurrentProcess` returns the pseudo-handle for this
        // process, which is always valid to pass to `TerminateProcess`.
        // Terminating our own process is the entire point; buffers were
        // flushed above and no Rust invariants outlive the process.
        unsafe {
            windows_sys::Win32::System::Threading::TerminateProcess(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                code as u32,
            );
        }
        // TerminateProcess only returns on failure; fall through to the
        // standard path rather than looping.
    }
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// `install` must be safe to call repeatedly without leaking
    /// background threads or panicking on the second invocation.
    #[test]
    fn install_is_idempotent() {
        install();
        install();
        install();
        // The flag itself is process-global; clearing it here keeps
        // other tests in the same binary unaffected by an accidental
        // earlier install. We only touch it when no signal has been
        // observed, which is the common case in unit tests.
        if !is_requested() {
            SHUTDOWN_REQUESTED.store(false, Ordering::Release);
            SHUTDOWN_EXIT_CODE.store(0, Ordering::Release);
        }
    }

    #[test]
    fn exit_code_is_none_until_signal_fires() {
        // We don't fire a real signal in unit tests because that
        // would race with cargo's own Ctrl-C handling and make
        // assertions order-dependent across the rest of the test
        // binary. Asserting the unsignalled invariant here pins down
        // the API contract, while the signalled path is verified
        // implicitly by the binary's exit-code behaviour at runtime.
        if !is_requested() {
            assert_eq!(exit_code(), None);
        }
    }
}

#[cfg(test)]
mod pipe_tolerant_panic_hook_tests {
    use super::*;

    /// beads_rust-3fna: the exact println! EPIPE payload must classify as a
    /// normal close.
    #[test]
    fn stdout_broken_pipe_print_panic_classifies_as_normal_close() {
        let payload = String::from("failed printing to stdout: Broken pipe (os error 32)");
        assert!(is_stdout_broken_pipe_panic(&payload));
    }

    #[test]
    fn static_str_variant_classifies_as_normal_close() {
        static PAYLOAD: &str = "failed printing to stdout: broken pipe";
        assert!(is_stdout_broken_pipe_panic(&PAYLOAD));
    }

    #[test]
    fn unrelated_panics_do_not_classify_as_pipe_close() {
        assert!(!is_stdout_broken_pipe_panic(&String::from(
            "index out of bounds"
        )));
        assert!(!is_stdout_broken_pipe_panic(&String::from(
            "failed printing to stderr: Broken pipe (os error 32)"
        )));
        assert!(!is_stdout_broken_pipe_panic(&String::from(
            "failed printing to stdout: No such device"
        )));
    }
}
