//! Consumer-owned leases, independent of the Store's resource references.

use crate::remote_resource::{RemoteResource, ResourceStatus};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use tokio::sync::watch;

pub(super) struct Lease(pub Box<dyn Fn() + Send + Sync>);

impl Drop for Lease {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// A shared endpoint with explicit consumer ownership. Dropping the last
/// handle or receiver retires its fetch task, regardless of cache references.
#[derive(Clone)]
pub struct ResourceHandle<T: Clone + Send + Sync + 'static> {
    pub(super) resource: Arc<RemoteResource<T>>,
    pub(super) lease: Arc<Lease>,
    pub(super) refetch: Arc<dyn Fn() + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> ResourceHandle<T> {
    pub fn subscribe(&self) -> ResourceSubscription<T> {
        ResourceSubscription {
            receiver: self.resource.subscribe(),
            _lease: self.lease.clone(),
        }
    }

    pub fn refetch(&self) {
        (self.refetch)();
    }
}

/// A watch receiver that keeps its consumer lease when moved or cloned.
#[derive(Clone)]
pub struct ResourceSubscription<T: Clone + Send + Sync + 'static> {
    receiver: watch::Receiver<ResourceStatus<T>>,
    _lease: Arc<Lease>,
}

impl<T: Clone + Send + Sync + 'static> Deref for ResourceSubscription<T> {
    type Target = watch::Receiver<ResourceStatus<T>>;

    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T: Clone + Send + Sync + 'static> DerefMut for ResourceSubscription<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for ResourceHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceHandle").finish_non_exhaustive()
    }
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for ResourceSubscription<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceSubscription")
            .finish_non_exhaustive()
    }
}
