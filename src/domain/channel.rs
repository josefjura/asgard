//! A broadcast channel that never makes the sender wait, knowing nothing of what travels on it,
//! so a domain service can emit onto the app-wide channel without depending on it.

use std::fmt;
use std::sync::Arc;

use tokio::sync::broadcast;

/// How many items a subscriber may fall behind before it misses the oldest.
const CAPACITY: usize = 64;

/// Broadcasts `E`s to whoever subscribed.
///
/// Emitting never waits: with no subscriber an item is dropped, and a subscriber that falls
/// more than [`CAPACITY`] items behind misses the oldest.
#[derive(Debug, Clone)]
pub struct Channel<E> {
    sender: broadcast::Sender<E>,
}

impl<E: Clone + Send + 'static> Channel<E> {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self { sender }
    }

    /// Every item emitted from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<E> {
        self.sender.subscribe()
    }

    /// An [`Emitter`] of `T`s, each sent as the `E` that `wrap` makes of it.
    #[must_use]
    pub fn emitter<T: 'static>(&self, wrap: fn(T) -> E) -> Emitter<T> {
        let sender = self.sender.clone();
        Emitter {
            emit: Arc::new(move |item| {
                // Sending fails only when no one subscribed, and then no one needs the item.
                let _ = sender.send(wrap(item));
            }),
        }
    }
}

impl<E: Clone + Send + 'static> Default for Channel<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Emits one service's `T`s onto a [`Channel`] without the service knowing what else travels
/// on it.
pub struct Emitter<T> {
    emit: Arc<dyn Fn(T) + Send + Sync>,
}

impl<T> Emitter<T> {
    pub(crate) fn emit(&self, item: T) {
        (self.emit)(item);
    }
}

impl<T> Clone for Emitter<T> {
    fn clone(&self) -> Self {
        Self {
            emit: Arc::clone(&self.emit),
        }
    }
}

impl<T> fmt::Debug for Emitter<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emitter").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::convert::identity;

    use tokio::sync::broadcast::error::TryRecvError;

    use super::*;

    #[test]
    fn an_emitter_wraps_what_it_sends() {
        let channel = Channel::<Option<i64>>::new();
        let mut subscription = channel.subscribe();

        channel.emitter(Some).emit(7);

        assert_eq!(subscription.try_recv(), Ok(Some(7)));
    }

    #[test]
    fn emitting_with_no_subscriber_drops_the_item() {
        let channel = Channel::<i64>::new();

        channel.emitter(identity).emit(1);

        let mut late = channel.subscribe();
        assert_eq!(late.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn a_subscriber_that_never_reads_does_not_hold_up_emitting() {
        let channel = Channel::<i64>::new();
        let emitter = channel.emitter(identity);
        let mut idle = channel.subscribe();

        for item in 0..=i64::try_from(CAPACITY).unwrap() {
            emitter.emit(item);
        }

        assert_eq!(idle.try_recv(), Err(TryRecvError::Lagged(1)));
        assert_eq!(idle.try_recv(), Ok(1));
    }
}
