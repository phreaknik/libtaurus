use crate::{Block, Header, ValidatorTicket};
use libp2p::gossipsub::{self, Sha256Topic, TopicHash};
use serde::{Deserialize, Serialize};
use strum_macros::{AsRefStr, EnumIter};

/// List of gossip messages
#[derive(Clone, Debug, Serialize, Deserialize, EnumIter, AsRefStr)]
pub enum Message {
    Block(Block),
    Header(Header),
    Ticket(ValidatorTicket),
}

impl<H: gossipsub::Hasher> From<&Message> for gossipsub::Topic<H> {
    fn from(m: &Message) -> Self {
        gossipsub::Topic::new(m.as_ref())
    }
}

impl From<Message> for TopicHash {
    fn from(m: Message) -> Self {
        Sha256Topic::from(&m).hash()
    }
}
