//! Signals for native process groups owned by local adapters, without spawning helpers.

/// Termination request delivered to every process in an owned group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Ask native handlers to clean up and exit (`SIGTERM`).
    Terminate,
    /// Stop the group immediately (`SIGKILL`).
    Kill,
}

/// Send `signal` to process group `group` when one is owned.
///
/// A group that has already exited, or delivery that the OS refuses, is ignored: callers
/// still reap their direct child afterwards. This is a single non-blocking system call,
/// so it is safe in async shutdown paths and `Drop`. Platforms without process groups
/// do nothing.
pub fn signal(group: Option<u32>, signal: Signal) {
    #[cfg(unix)]
    if let Some(group) = group
        .and_then(|group| i32::try_from(group).ok())
        .filter(|group| *group > 0)
    {
        let signal = match signal {
            Signal::Terminate => nix::sys::signal::Signal::SIGTERM,
            Signal::Kill => nix::sys::signal::Signal::SIGKILL,
        };
        let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(group), signal);
    }
    #[cfg(not(unix))]
    let _ = (group, signal);
}

#[cfg(all(test, unix))]
mod tests;
