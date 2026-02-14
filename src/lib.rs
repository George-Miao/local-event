//! A single-threaded (unsync) version of [`event-listener`](https://crates.io/crates/event-listener).

#![no_std]

extern crate alloc;

use alloc::{collections::BTreeMap, rc::Rc};
use core::{
    cell::RefCell,
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll, Waker},
};

/// A synchronization primitive for notifying tasks in a single-threaded
/// context.
///
/// This is similar to `event_listener::Event` but uses `Rc`/`RefCell` instead
/// of thread-safe primitives, making it suitable only for single-threaded use.
#[derive(Clone)]
pub struct Event {
    inner: Rc<RefCell<Inner>>,
}

impl Debug for Event {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let guard = self.inner.try_borrow();
        match guard {
            Ok(inner) => f.debug_tuple("Event").field(&inner).finish(),
            Err(_) => f.debug_tuple("Event").field(&"<locked>").finish(),
        }
    }
}

#[derive(Debug)]
struct Inner {
    /// List of listeners waiting for notification.
    listeners: BTreeMap<usize, ListenerEntry>,

    /// Counter for generating unique listener IDs.
    next_id: usize,

    /// Number of notified listeners that haven't been woken yet.
    notified: usize,
}

#[derive(Debug, Default)]
struct ListenerEntry {
    waker: Option<Waker>,
    notified: bool,
}

impl Event {
    /// Creates a new `Event`.
    ///
    /// # Examples
    ///
    /// ```
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    /// ```
    pub fn new() -> Self {
        Event {
            inner: Rc::new(RefCell::new(Inner {
                listeners: BTreeMap::new(),
                next_id: 0,
                notified: 0,
            })),
        }
    }

    /// Returns a guard listening for a notification. Returned listener can be
    /// `.await`-ed directly.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    /// event.listen().await;
    ///
    /// // Do something after the event is received.
    /// ```
    pub fn listen(&self) -> EventListener {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;

        inner.listeners.insert(id, ListenerEntry::default());

        EventListener {
            event: Rc::clone(&self.inner),
            id,
        }
    }

    /// Notifies a number of active listeners.
    ///
    /// The number of notified listeners is determined by `n`:
    /// - If `n` is `usize::MAX`, all active listeners are notified.
    /// - Otherwise, `n` active listeners are notified.
    ///
    /// # Examples
    ///
    /// ```
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    ///
    /// // Notify all listeners.
    /// event.notify(usize::MAX);
    ///
    /// // Notify exactly 5 listeners.
    /// event.notify(5);
    /// ```
    pub fn notify(&self, n: usize) {
        let mut inner = self.inner.borrow_mut();

        let count = if n == usize::MAX {
            inner.listeners.len()
        } else {
            n.saturating_sub(inner.notified)
        };

        let mut notified = 0;
        for entry in inner.listeners.values_mut() {
            if notified >= count {
                break;
            }
            if entry.notified {
                continue;
            }
            entry.notified = true;
            if let Some(waker) = entry.waker.take() {
                waker.wake();
            }
            notified += 1;
        }

        inner.notified += notified;
    }

    /// Notifies a number of active and still waiting listeners.
    ///
    /// Unlike `notify()`, this method only notifies listeners that haven't been
    /// notified yet and are still registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    /// event.notify_additional(2);
    /// ```
    pub fn notify_additional(&self, n: usize) {
        let mut inner = self.inner.borrow_mut();

        let count = if n == usize::MAX {
            inner.listeners.len()
        } else {
            n.min(inner.listeners.len())
        };

        let mut notified = 0;
        for entry in inner.listeners.values_mut() {
            if notified >= count {
                break;
            }
            if entry.notified {
                continue;
            }
            entry.notified = true;
            if let Some(waker) = entry.waker.take() {
                waker.wake();
            }
            notified += 1;
        }

        inner.notified += notified;
    }

    /// Notifies all active listeners.
    ///
    /// This is equivalent to calling `notify(usize::MAX)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    /// let listener1 = event.listen();
    /// let listener2 = event.listen();
    ///
    /// // Notify all listeners.
    /// event.notify_all();
    ///
    /// assert!(listener1.is_notified());
    /// assert!(listener2.is_notified());
    /// ```
    pub fn notify_all(&self) {
        self.notify(usize::MAX);
    }
}

impl Default for Event {
    fn default() -> Self {
        Self::new()
    }
}

/// A guard waiting for a notification from an [`Event`].
///
/// This listener can be polled or `await`-ed as a [`Future`].
pub struct EventListener {
    event: Rc<RefCell<Inner>>,
    id: usize,
}

impl EventListener {
    /// Returns `true` if this listener has been notified.
    ///
    /// # Examples
    ///
    /// ```
    /// use local_event::Event;
    ///
    /// let event = Event::new();
    /// let listener = event.listen();
    ///
    /// assert!(!listener.is_notified());
    /// event.notify(1);
    /// assert!(listener.is_notified());
    /// ```
    pub fn is_notified(&self) -> bool {
        self.event
            .borrow()
            .listeners
            .get(&self.id)
            .map(|e| e.notified)
            .unwrap_or(false)
    }
}

impl Drop for EventListener {
    fn drop(&mut self) {
        let mut inner = self.event.borrow_mut();

        // Find and remove this listener
        let Some(entry) = inner.listeners.remove(&self.id) else {
            return;
        };

        if !entry.notified || inner.notified == 0 {
            return;
        }

        inner.notified -= 1;

        let Some(next) = inner.listeners.values_mut().find(|e| !e.notified) else {
            return;
        };

        next.notified = true;

        if let Some(waker) = next.waker.take() {
            waker.wake();
        }

        inner.notified += 1;
    }
}

impl core::future::Future for EventListener {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.event.borrow_mut();

        let Some(entry) = inner.listeners.get_mut(&self.id) else {
            unreachable!("Entry shouldn't be removed")
        };

        if entry.notified {
            return Poll::Ready(());
        }

        // Store the waker for later notification
        entry.waker = Some(cx.waker().clone());

        Poll::Pending
    }
}

/// Create an event listener for an [`Event`].
///
/// This macro exists **solely for API compatibility** with `event-listener`,
/// making it easier to swap or mix `local-event` and `event-listener` in the
/// same codebase. Unlike the `event-listener` version, this macro provides no
/// stack-allocation benefit — it is a trivial wrapper around [`Event::listen`].
///
/// **Prefer calling [`Event::listen`] directly.** Only reach for this macro
/// when you need source-level compatibility with code that already uses
/// `event-listener`'s `listener!`.
///
/// # Example
///
/// ```
/// use local_event::{Event, listener};
///
/// let event = Event::new();
/// listener!(event => listener);
/// // equivalent to: let mut listener = event.listen();
/// ```
#[macro_export]
macro_rules! listener {
    ($event:expr => $listener:ident) => {
        #[allow(unused_mut)]
        let mut $listener = $event.listen();
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify() {
        let event = Event::new();
        let listener = event.listen();

        assert!(!listener.is_notified());
        event.notify(1);
        assert!(listener.is_notified());
    }

    #[test]
    fn test_notify_multiple() {
        let event = Event::new();
        let listener1 = event.listen();
        let listener2 = event.listen();
        let listener3 = event.listen();

        event.notify(2);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(!listener3.is_notified());

        event.notify(2);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(!listener3.is_notified());
    }

    #[test]
    fn test_notify_additional() {
        let event = Event::new();
        let listener1 = event.listen();
        let listener2 = event.listen();
        let listener3 = event.listen();
        let listener4 = event.listen();

        event.notify(2);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(!listener3.is_notified());
        assert!(!listener4.is_notified());

        event.notify_additional(2);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(listener3.is_notified());
        assert!(listener4.is_notified());
    }

    #[test]
    fn test_notify_all() {
        let event = Event::new();
        let listener1 = event.listen();
        let listener2 = event.listen();
        let listener3 = event.listen();

        event.notify(usize::MAX);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(listener3.is_notified());
    }

    #[test]
    fn test_notify_drop() {
        let event = Event::new();
        let listener1 = event.listen();
        let listener2 = event.listen();
        let listener3 = event.listen();

        event.notify(2);

        assert!(listener1.is_notified());
        assert!(listener2.is_notified());
        assert!(!listener3.is_notified());

        drop(listener2);

        assert!(listener3.is_notified());
    }

    #[pollster::test]
    async fn test_listen_async() {
        let event = Event::new();
        let listener = event.listen();

        event.notify(1);

        listener.await
    }
}
