use super::{Event, Message, Result};
use tokio::sync::{broadcast, mpsc};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    TokioMpscSend(#[from] tokio::sync::mpsc::error::SendError<Request>),
}

#[derive(Clone, Debug)]
pub enum Request {
    Broadcast(Message),
}

#[derive(Clone, Debug)]
pub struct Api {
    request_ch: mpsc::UnboundedSender<Request>,
    event_emitter: broadcast::Sender<Event>,
}

impl Api {
    /// Create a new API handle
    pub fn new(
        request_ch: mpsc::UnboundedSender<Request>,
        event_emitter: broadcast::Sender<Event>,
    ) -> Api {
        Api {
            request_ch,
            event_emitter,
        }
    }

    /// Subscribe to P2P events
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.event_emitter.subscribe()
    }

    /// Broadcast data to the peer network
    pub fn broadcast(&mut self, msg: Message) -> Result<()> {
        Ok(self
            .request_ch
            .send(Request::Broadcast(msg))
            .map_err(Error::from)?)
    }
}
