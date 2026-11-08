//! Graceful signalling through bubblewrap's monitor and PID namespace reaper.

use std::{fs, io};

/// Send SIGTERM to the native PID 2 below the owned bubblewrap `monitor` process.
///
/// The monitor and PID 1 reaper stay alive so `--die-with-parent` cannot interrupt
/// native cleanup. Called on Pi's dedicated shutdown thread; failure leaves forced
/// termination to its existing deadline. Returns an error if the child has exited,
/// the expected namespace hierarchy is unavailable, or signal delivery fails.
pub(crate) fn terminate_native(monitor: u32) -> io::Result<()> {
    let reaper = namespace_child(monitor, 1)?;
    let native = namespace_child(reaper, 2)?;
    let native = i32::try_from(native)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid native pid"))?;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(native),
        nix::sys::signal::Signal::SIGTERM,
    )
    .map_err(|error| io::Error::from_raw_os_error(error as i32))
}

fn namespace_child(parent: u32, expected: u32) -> io::Result<u32> {
    let children = fs::read_to_string(format!("/proc/{parent}/task/{parent}/children"))?;
    children
        .split_whitespace()
        .filter_map(|pid| pid.parse::<u32>().ok())
        .find(|pid| {
            fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|status| {
                    status.lines().find_map(|line| {
                        line.strip_prefix("NSpid:")?
                            .split_whitespace()
                            .last()?
                            .parse::<u32>()
                            .ok()
                    })
                })
                == Some(expected)
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "native sandbox process unavailable",
            )
        })
}
