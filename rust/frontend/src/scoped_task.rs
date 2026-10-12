//! Cancellation ownership for a screen's asynchronous fill. Tickets still
//! reject queued UI completions; dropping a model also stops its network wait.

use std::sync::Arc;
use tokio::task::{AbortHandle, JoinHandle};

#[derive(Debug)]
struct AbortOnDrop(AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScopedTask(Option<Arc<AbortOnDrop>>);

impl ScopedTask {
    pub fn replace(&mut self, task: JoinHandle<()>) {
        self.cancel();
        self.0 = Some(Arc::new(AbortOnDrop(task.abort_handle())));
        drop(task);
    }

    /// A reticketed model must cancel even if navigation retained its clone.
    pub fn cancel(&mut self) {
        if let Some(task) = self.0.take() {
            task.0.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replacement_and_last_owner_drop_abort_tasks() {
        let first = tokio::spawn(std::future::pending());
        let first_abort = first.abort_handle();
        let mut owner = ScopedTask::default();
        owner.replace(first);
        let second = tokio::spawn(std::future::pending());
        let second_abort = second.abort_handle();
        owner.replace(second);
        tokio::task::yield_now().await;
        assert!(first_abort.is_finished());
        let retained = owner.clone();
        drop(owner);
        assert!(!second_abort.is_finished());
        drop(retained);
        tokio::task::yield_now().await;
        assert!(second_abort.is_finished());
    }

    #[tokio::test]
    async fn cancellation_retires_a_task_even_with_a_retained_model() {
        let task = tokio::spawn(std::future::pending());
        let abort = task.abort_handle();
        let mut owner = ScopedTask::default();
        owner.replace(task);
        let _retained = owner.clone();
        owner.cancel();
        tokio::task::yield_now().await;
        assert!(abort.is_finished());
    }
}
