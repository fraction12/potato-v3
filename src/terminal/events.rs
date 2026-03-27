//! Async terminal event stream — wraps crossterm into a tokio Stream.

use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::app::message::Message;

/// Spawn a background task that reads crossterm events and forwards them as
/// [`Message`] values through the returned channel.
pub fn event_stream() -> mpsc::Receiver<Message> {
    let (tx, rx) = mpsc::channel(256);

    tokio::spawn(async move {
        let mut reader = EventStream::new();
        loop {
            match reader.next().await {
                Some(Ok(Event::Key(k))) => {
                    if tx.send(Message::Key(k)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Event::Mouse(m))) => {
                    if tx.send(Message::Mouse(m)).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Event::Resize(w, h))) => {
                    if tx.send(Message::Resize(w, h)).await.is_err() {
                        break;
                    }
                }
                Some(Err(_)) | None => break,
                _ => {}
            }
        }
    });

    rx
}

/// Wraps the event receiver as a [`ReceiverStream`] for `StreamExt` usage.
pub fn event_stream_as_stream(rx: mpsc::Receiver<Message>) -> ReceiverStream<Message> {
    ReceiverStream::new(rx)
}
