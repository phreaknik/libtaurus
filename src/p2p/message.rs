use libp2p::gossipsub::{self, Sha256Topic, TopicHash};
use strum_macros::{AsRefStr, EnumIter};

/// List of topics to subscribe to
#[derive(Clone, Debug, EnumIter, AsRefStr)]
pub enum Message {
    Hello(Vec<u8>),
}

impl Message {
    /// Convert from ['gossipsub::Message']
    pub fn from_gossipsub(m: gossipsub::Message) -> Message {
        Message::Hello(m.data)
    }

    /// Get the associated ['gossipsub::Topic'] for this message
    pub fn topic(&self) -> Sha256Topic {
        self.into()
    }

    /// Get the message data
    pub fn data(&self) -> &[u8] {
        match self {
            Message::Hello(s) => s,
        }
    }
}

impl<H: gossipsub::Hasher> From<&Message> for gossipsub::Topic<H> {
    fn from(m: &Message) -> Self {
        gossipsub::Topic::new(m.as_ref())
    }
}

impl From<Message> for TopicHash {
    fn from(m: Message) -> Self {
        m.topic().hash()
    }
}
