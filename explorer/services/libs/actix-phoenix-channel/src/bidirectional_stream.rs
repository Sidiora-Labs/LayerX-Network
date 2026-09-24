use futures_lite::Stream;
use std::{
    fmt::Debug,
    pin::Pin,
    task::{Context, Poll},
};

pin_project_lite::pin_project! {
    pub struct BidirectionalStream<I, O> {
        #[pin]
        pub inbound: Option<I>,
        #[pin]
        pub outbound: O,
    }
}

#[derive(Debug)]
pub enum Direction<I, O> {
    Inbound(I),
    Outbound(O),
}

impl<I, O> Stream for BidirectionalStream<I, O>
where
    I: Stream,
    O: Stream,
{
    type Item = Direction<I::Item, O::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        macro_rules! poll_inbound {
            () => {
                if let Some(inbound) = this.inbound.as_pin_mut() {
                    match inbound.poll_next(cx) {
                        Poll::Ready(Some(t)) => return Poll::Ready(Some(Direction::Inbound(t))),
                        Poll::Ready(None) => return Poll::Ready(None),
                        _ => (),
                    }
                }
            };
        }
        macro_rules! poll_outbound {
            () => {
                match this.outbound.poll_next(cx) {
                    Poll::Ready(Some(t)) => return Poll::Ready(Some(Direction::Outbound(t))),
                    Poll::Ready(None) => return Poll::Ready(None),
                    _ => (),
                }
            };
        }

        if fastrand::bool() {
            poll_inbound!();
            poll_outbound!();
        } else {
            poll_outbound!();
            poll_inbound!();
        }

        Poll::Pending
    }
}
