//! Workspace-scoped cancellation and native process ownership during archive.

use super::{Child, Duration, HashSet, Inner, Instant, Weak, WorkspaceAutomationError};
use super::{POLL_INTERVAL, lock, setup_io, terminate_child, thread};

pub(super) fn close(inner: &Inner, ids: &[String]) -> Result<(), WorkspaceAutomationError> {
    close_until(inner, ids, Instant::now() + Duration::from_secs(5))
}

fn close_until(
    inner: &Inner,
    ids: &[String],
    deadline: Instant,
) -> Result<(), WorkspaceAutomationError> {
    let selected: HashSet<_> = ids.iter().map(String::as_str).collect();
    {
        let mut state = lock(&inner.state);
        for id in ids {
            if state.setup_running.contains(id) {
                state.setup_cancelled.insert(id.clone());
            }
        }
        for ((workspace, _), process) in &mut state.scripts {
            if selected.contains(workspace.as_str())
                && let Some(child) = process.child.as_mut()
            {
                let status = terminate_child(child).map_err(|error| setup_io(&error))?;
                process.exit_code = status.code();
                process.child = None;
            }
        }
    }
    loop {
        let mut state = lock(&inner.state);
        for id in ids {
            if let Some(child) = state.setup_cleanup.get_mut(id) {
                terminate_child(child).map_err(|error| setup_io(&error))?;
                state.setup_cleanup.remove(id);
            }
        }
        if ids.iter().all(|id| !state.setup_running.contains(id)) {
            for id in ids {
                state.setup_cancelled.remove(id);
            }
            return Ok(());
        }
        drop(state);
        if Instant::now() >= deadline {
            return Err(WorkspaceAutomationError::Io(
                "Workspace setup cleanup is still pending; retry archive".to_owned(),
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests;

pub(super) fn cancelled(inner: &Weak<Inner>, id: &str) -> bool {
    inner
        .upgrade()
        .is_none_or(|inner| lock(&inner.state).setup_cancelled.contains(id))
}

pub(super) fn terminate_setup(
    inner: &Weak<Inner>,
    id: &str,
    mut child: Child,
) -> Result<(), WorkspaceAutomationError> {
    match terminate_child(&mut child) {
        Ok(_) => Ok(()),
        Err(error) => {
            if let Some(inner) = inner.upgrade() {
                lock(&inner.state)
                    .setup_cleanup
                    .insert(id.to_owned(), child);
            }
            Err(setup_io(&error))
        }
    }
}
