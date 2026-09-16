use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use lux_project::ProjectPaths;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(100);

/// Shared watch/debounce infrastructure used by both `dev` and
/// `check --watch`. A burst becomes one sorted, deduplicated path batch.
pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<notify::Result<Event>>,
    paths: ProjectPaths,
    pending: BTreeSet<PathBuf>,
    deadline: Option<Instant>,
}

impl ProjectWatcher {
    pub fn new(paths: ProjectPaths) -> notify::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(&paths.root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            receiver,
            paths,
            pending: BTreeSet::new(),
            deadline: None,
        })
    }

    /// Non-blocking. Returns a batch only after no relevant event has been
    /// seen for the debounce interval.
    pub fn poll(&mut self, now: Instant) -> notify::Result<Option<Vec<PathBuf>>> {
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(event)) => {
                    let mut relevant = false;
                    for path in event.paths {
                        if is_relevant(&self.paths, &path) {
                            self.pending.insert(path);
                            relevant = true;
                        }
                    }
                    if relevant {
                        self.deadline = Some(now + WATCH_DEBOUNCE);
                    }
                }
                Ok(Err(error)) => return Err(error),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    return Err(notify::Error::generic("filesystem watcher disconnected"));
                }
            }
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            self.deadline = None;
            return Ok(Some(
                std::mem::take(&mut self.pending).into_iter().collect(),
            ));
        }
        Ok(None)
    }

    /// Updates the configured path filter after a valid manifest reload. The
    /// OS watcher already observes the whole project root, so no handle needs
    /// to be torn down or recreated.
    pub fn update_paths(&mut self, paths: ProjectPaths) {
        debug_assert_eq!(self.paths.root, paths.root);
        self.paths = paths;
    }
}

pub fn is_relevant(paths: &ProjectPaths, path: &Path) -> bool {
    path == paths.manifest
        || path == paths.entry
        || path == paths.patch
        || path == paths.bindings
        || path.starts_with(&paths.fixtures)
        || (path.starts_with(paths.root.join("src"))
            && path.extension().is_some_and(|extension| extension == "lux"))
}

/// Deterministic debounce primitive used to test event coalescing without
/// depending on OS watcher timing.
#[derive(Debug, Default)]
pub struct DebounceState {
    deadline: Option<Instant>,
    dirty: bool,
}

impl DebounceState {
    pub fn event(&mut self, now: Instant) {
        self.dirty = true;
        self.deadline = Some(now + WATCH_DEBOUNCE);
    }

    pub fn take_due(&mut self, now: Instant) -> bool {
        if self.dirty && self.deadline.is_some_and(|deadline| now >= deadline) {
            self.dirty = false;
            self.deadline = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_editor_events_coalesce_into_one_rebuild() {
        let start = Instant::now();
        let mut debounce = DebounceState::default();
        for millis in [0, 10, 20, 30, 40] {
            debounce.event(start + Duration::from_millis(millis));
        }
        assert!(!debounce.take_due(start + Duration::from_millis(139)));
        assert!(debounce.take_due(start + Duration::from_millis(140)));
        assert!(!debounce.take_due(start + Duration::from_secs(1)));
    }
}
