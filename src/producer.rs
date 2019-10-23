//! A producer type which will send a copy of each published message to all subscribers.

use futures::{
    sync::mpsc::{
        unbounded,
        UnboundedReceiver,
        UnboundedSender,
    }
};

/// A producer type which will send a copy of each published message to all subscribers.
pub struct Producer<T: Clone> {
    subscribers: Vec<UnboundedSender<T>>,
}

impl<T: Clone> Producer<T> {
    /// Publish a value to all subscribers.
    pub fn publish(&mut self, msg: T) {
        // Update this to use https://doc.rust-lang.org/std/vec/struct.Vec.html#method.drain_filter when available on stable.
        let len = self.subscribers.len();
        let mut msg = Some(msg);
        self.subscribers = self.subscribers
            .drain(..)
            .enumerate()
            .filter(move |(idx, sub)| {
                let val = if (idx + 1) == len {
                    msg.take()
                } else {
                    msg.clone()
                };
                if let Some(inner) = val {
                    sub.unbounded_send(inner).is_ok()
                } else {
                    true
                }
            })
            .map(|(_, sub)| sub)
            .collect();
    }

    /// Add a new subscriber.
    pub fn subscribe(&mut self) -> UnboundedReceiver<T> {
        let (tx, rx) = unbounded();
        self.subscribers.push(tx);
        rx
    }
}

impl<T: Clone> Default for Producer<T> {
    fn default() -> Self {
        Producer{subscribers: vec![]}
    }
}
