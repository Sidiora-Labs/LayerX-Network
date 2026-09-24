use std::{future::Future, time::Duration};

use tokio::{task::JoinSet, time::error::Elapsed};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) const DEFAULT_SHUTDOWN_TIMEOUT_SEC: u64 = 15;

/// Graceful shutdown according to https://tokio.rs/tokio/topics/shutdown
#[derive(Clone)]
pub struct GracefulShutdownHandler {
    pub shutdown_token: CancellationToken,
    pub task_tracker: TaskTracker,
}

impl Default for GracefulShutdownHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl GracefulShutdownHandler {
    pub fn new() -> Self {
        Self {
            shutdown_token: CancellationToken::new(),
            task_tracker: TaskTracker::new(),
        }
    }

    pub fn from_token(token: CancellationToken) -> Self {
        Self {
            shutdown_token: token,
            task_tracker: TaskTracker::new(),
        }
    }

    /// Close the tasks with shutdown token and wait for their completion
    /// with provided (or default) timeout (`duration`).
    pub async fn cancel_wait_timeout(&self, duration: Option<Duration>) -> Result<(), Elapsed> {
        self.shutdown_token.cancel();
        self.task_tracker.close();
        let duration = duration.unwrap_or(Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SEC));
        tokio::time::timeout(duration, self.task_tracker.wait()).await
    }
}

/// `GracefulShutdownHandler` but with local task tracker always included.
/// See [`TaskTrackers`] for details
#[derive(Clone)]
pub struct LocalGracefulShutdownHandler {
    pub shutdown_token: CancellationToken,
    pub task_trackers: TaskTrackers,
}

impl From<GracefulShutdownHandler> for LocalGracefulShutdownHandler {
    fn from(value: GracefulShutdownHandler) -> Self {
        Self {
            shutdown_token: value.shutdown_token,
            task_trackers: TaskTrackers::new(value.task_tracker),
        }
    }
}

impl LocalGracefulShutdownHandler {
    pub async fn spawn_and_track<F>(
        &self,
        futures: &mut JoinSet<F::Output>,
        future: F,
    ) -> tokio::task::AbortHandle
    where
        F: Future,
        F: Send + 'static,
        F::Output: Send,
    {
        futures.spawn(
            self.task_trackers
                .local
                .track_future(self.task_trackers.external.track_future(future)),
        )
    }

    /// Close the tasks with shutdown token and wait for the local tasks to complete
    /// with provided (or default) timeout (`duration`).
    pub async fn local_cancel_wait_timeout(
        &self,
        duration: Option<Duration>,
    ) -> Result<(), Elapsed> {
        self.shutdown_token.cancel();
        self.task_trackers.close_local();
        let duration = duration.unwrap_or(Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SEC));
        tokio::time::timeout(duration, self.task_trackers.wait_local()).await
    }
}

/// Just like a regular task tracker, but it also tracks
/// the tasks to an external tracker. We would like to
/// allow the dependent crate to track tasks created in the launcher.
///
/// So, it behaves just like a normal task tracker, but it adds the
/// task to an external tracker when tracking.
#[derive(Clone)]
pub(crate) struct TaskTrackers {
    // we don't use `JoinSet` here because we wish to
    // share this tracker with many tasks
    /// tracker for tasks created within this crate
    pub local: TaskTracker,
    /// tracker provided by some dependant crate
    pub external: TaskTracker,
}

impl TaskTrackers {
    pub fn new(external: TaskTracker) -> Self {
        Self {
            local: TaskTracker::new(),
            external,
        }
    }

    /// See [TaskTracker::close]
    pub fn close_local(&self) {
        self.local.close();
    }

    /// See [TaskTracker::wait]
    pub async fn wait_local(&self) {
        self.local.wait().await;
    }

    /// Tracks the task in both local and external trackers.
    ///
    /// See [TaskTracker::track_future]
    pub fn track_future<F>(&self, future: F) -> impl Future<Output = F::Output>
    where
        F: Future,
    {
        self.external.track_future(self.local.track_future(future))
    }
}
