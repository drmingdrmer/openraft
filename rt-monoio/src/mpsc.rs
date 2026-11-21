//! MPSC channel is implemented with tokio MPSC channels.
//!
//! Tokio MPSC channel are runtime independent.

use std::future::Future;

use futures_util::TryFutureExt;
use openraft_rt::Mpsc;
use openraft_rt::MpscReceiver;
use openraft_rt::MpscSender;
use openraft_rt::MpscWeakSender;
use openraft_rt::OptionalSend;
use openraft_rt::SendError;
use openraft_rt::TryRecvError;
use tokio::sync::mpsc as tokio_mpsc;

pub struct MonoioMpsc;

pub struct MonoioMpscSender<T>(tokio_mpsc::Sender<T>);

impl<T> Clone for MonoioMpscSender<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct MonoioMpscReceiver<T>(tokio_mpsc::Receiver<T>);

pub struct MonoioMpscWeakSender<T>(tokio_mpsc::WeakSender<T>);

impl<T> Clone for MonoioMpscWeakSender<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Mpsc for MonoioMpsc {
    type Sender<T: OptionalSend> = MonoioMpscSender<T>;
    type Receiver<T: OptionalSend> = MonoioMpscReceiver<T>;
    type WeakSender<T: OptionalSend> = MonoioMpscWeakSender<T>;

    #[inline]
    fn channel<T: OptionalSend>(buffer: usize) -> (Self::Sender<T>, Self::Receiver<T>) {
        let (tx, rx) = tokio_mpsc::channel(buffer);
        let tx_wrapper = MonoioMpscSender(tx);
        let rx_wrapper = MonoioMpscReceiver(rx);

        (tx_wrapper, rx_wrapper)
    }

    /// Rejects a `buffer` of 0, or one greater than `tokio::sync::Semaphore::MAX_PERMITS`: the
    /// Tokio channel that this adapter uses panics on both.
    fn check_buffer(buffer: usize) -> Result<(), String> {
        if buffer == 0 {
            return Err("a Tokio mpsc channel needs a buffer of at least 1".to_string());
        }
        let max = tokio::sync::Semaphore::MAX_PERMITS;
        if buffer > max {
            return Err(format!("a Tokio mpsc channel buffer must be at most {max}"));
        }
        Ok(())
    }
}

impl<T> MpscSender<MonoioMpsc, T> for MonoioMpscSender<T>
where T: OptionalSend
{
    #[inline]
    fn send(&self, msg: T) -> impl Future<Output = Result<(), SendError<T>>> {
        self.0.send(msg).map_err(|e| SendError(e.0))
    }

    #[inline]
    fn downgrade(&self) -> <MonoioMpsc as Mpsc>::WeakSender<T> {
        let inner = self.0.downgrade();
        MonoioMpscWeakSender(inner)
    }
}

impl<T> MpscReceiver<T> for MonoioMpscReceiver<T> {
    #[inline]
    fn recv(&mut self) -> impl Future<Output = Option<T>> {
        self.0.recv()
    }

    #[inline]
    fn try_recv(&mut self) -> Result<T, TryRecvError> {
        self.0.try_recv().map_err(|e| match e {
            tokio_mpsc::error::TryRecvError::Empty => TryRecvError::Empty,
            tokio_mpsc::error::TryRecvError::Disconnected => TryRecvError::Disconnected,
        })
    }
}

impl<T> MpscWeakSender<MonoioMpsc, T> for MonoioMpscWeakSender<T>
where T: OptionalSend
{
    #[inline]
    fn upgrade(&self) -> Option<<MonoioMpsc as Mpsc>::Sender<T>> {
        self.0.upgrade().map(MonoioMpscSender)
    }
}

#[cfg(test)]
mod tests {
    use openraft_rt::Mpsc;

    use super::MonoioMpsc;

    #[test]
    fn test_check_buffer_rejects_what_a_tokio_channel_panics_on() {
        let max = tokio::sync::Semaphore::MAX_PERMITS;

        assert!(MonoioMpsc::check_buffer(0).is_err());
        assert_eq!(Ok(()), MonoioMpsc::check_buffer(1));
        assert_eq!(Ok(()), MonoioMpsc::check_buffer(max));
        assert!(MonoioMpsc::check_buffer(max + 1).is_err());
    }
}
