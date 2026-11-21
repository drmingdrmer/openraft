use std::collections::VecDeque;
use std::future::poll_fn;
use std::pin::pin;
use std::task::Poll;

use crate::AsyncRuntime;
use crate::Mpsc;
use crate::MpscReceiver;
use crate::MpscSender;
use crate::OptionalSend;

pub(super) struct BufferingTask<R, T>
where
    R: MpscReceiver<T>,
    T: OptionalSend,
{
    source: Option<R>,
    queued: VecDeque<T>,
}

/// What completed in one turn of [`BufferingTask::run`].
enum Event<R, T> {
    /// The send of the oldest queued command finished; `false` when the output receiver is gone.
    Sent(bool),
    /// The source receive finished, with the source returned and its command, `None` once closed.
    Received(R, Option<T>),
}

impl<R, T> BufferingTask<R, T>
where
    R: MpscReceiver<T>,
    T: OptionalSend,
{
    pub(super) fn new(source: R) -> Self {
        Self {
            source: Some(source),
            queued: VecDeque::new(),
        }
    }

    /// Forward commands from the source to `sender`, buffering them while the output is full.
    ///
    /// The send of the oldest queued command and the receive of the next source command stay in
    /// flight until each completes: dropping a receive that is already pending could lose a
    /// command it has taken from the source. The send is polled first when both are ready.
    pub(super) async fn run<Rt>(self, sender: <Rt::Mpsc as Mpsc>::Sender<T>)
    where Rt: AsyncRuntime {
        let Self { source, mut queued } = self;
        let send = |command: T| sender.send(command);

        let mut sending = pin!(None.map(send));
        let mut receiving = pin!(source.map(Self::receive));

        loop {
            if sending.is_none() {
                match queued.pop_front() {
                    Some(command) => sending.set(Some(send(command))),
                    None if receiving.is_none() => return,
                    None => {}
                }
            }

            let event = poll_fn(|cx| {
                if let Some(send) = sending.as_mut().as_pin_mut()
                    && let Poll::Ready(sent) = send.poll(cx)
                {
                    return Poll::Ready(Event::Sent(sent.is_ok()));
                }
                if let Some(receive) = receiving.as_mut().as_pin_mut()
                    && let Poll::Ready((source, command)) = receive.poll(cx)
                {
                    return Poll::Ready(Event::Received(source, command));
                }
                Poll::Pending
            })
            .await;

            match event {
                Event::Sent(true) => sending.set(None),
                Event::Sent(false) => return,
                Event::Received(source, Some(command)) => {
                    queued.push_back(command);
                    receiving.set(Some(Self::receive(source)));
                }
                Event::Received(_closed, None) => receiving.set(None),
            }
        }
    }

    /// Receive the next command, and give the source back with it.
    async fn receive(mut source: R) -> (R, Option<T>) {
        let command = source.recv().await;
        (source, command)
    }
}
