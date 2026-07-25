//! SSE relay trait — unifies streaming response relay logic (Module C2 — v2.0).
//!
//! Abstract the common pattern of transforming upstream provider byte streams
//! into OpenAI-compatible SSE streams. All provider stream implementations
//! implement this trait, eliminating duplicated relay logic.

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::error::AppError;

pub trait SseRelay: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static {}

impl<T> SseRelay for T where T: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static {}

pub fn stream_to_body<S>(stream: S) -> axum::body::Body
where
    S: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static,
{
    let mapped = stream.map(|item| match item {
        Ok(bytes) => Ok::<_, std::convert::Infallible>(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "stream error during relay");
            Ok(Bytes::new())
        }
    });
    axum::body::Body::from_stream(mapped)
}

pub struct StreamWithInspector<S> {
    inner: S,
    tx: mpsc::Sender<Bytes>,
}

impl<S> StreamWithInspector<S>
where
    S: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static,
{
    pub fn new(inner: S, tx: mpsc::Sender<Bytes>) -> Self {
        Self { inner, tx }
    }
}

impl<S> Stream for StreamWithInspector<S>
where
    S: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static,
{
    type Item = Result<Bytes, AppError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                let _ = self.tx.try_send(bytes.clone());
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub fn relay_with_inspector<S>(source: S, tx: mpsc::Sender<Bytes>) -> StreamWithInspector<S>
where
    S: Stream<Item = Result<Bytes, AppError>> + Unpin + Send + 'static,
{
    StreamWithInspector::new(source, tx)
}
