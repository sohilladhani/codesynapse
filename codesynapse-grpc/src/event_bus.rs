use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    NodeAdded {
        id: String,
        label: String,
        source_file: String,
    },
    EdgeAdded {
        source: String,
        target: String,
        relation: String,
    },
    NodeRemoved {
        id: String,
    },
    GraphReset,
}

const CHANNEL_CAPACITY: usize = 256;

pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscribe_and_receive_graph_reset() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(Event::GraphReset);
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev, Event::GraphReset);
    }

    #[tokio::test]
    async fn test_node_added_event_roundtrip() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(Event::NodeAdded {
            id: "n1".to_string(),
            label: "Foo".to_string(),
            source_file: "a.rs".to_string(),
        });
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, Event::NodeAdded { ref id, .. } if id == "n1"));
    }

    #[tokio::test]
    async fn test_edge_added_event_roundtrip() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(Event::EdgeAdded {
            source: "a".to_string(),
            target: "b".to_string(),
            relation: "calls".to_string(),
        });
        let ev = rx.recv().await.unwrap();
        assert!(matches!(ev, Event::EdgeAdded { ref source, .. } if source == "a"));
    }

    #[tokio::test]
    async fn test_multiple_subscribers_both_receive() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        bus.emit(Event::GraphReset);
        assert_eq!(rx1.recv().await.unwrap(), Event::GraphReset);
        assert_eq!(rx2.recv().await.unwrap(), Event::GraphReset);
    }

    #[tokio::test]
    async fn test_multiple_events_received_in_order() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(Event::NodeAdded {
            id: "1".to_string(),
            label: "A".to_string(),
            source_file: "f.rs".to_string(),
        });
        bus.emit(Event::NodeAdded {
            id: "2".to_string(),
            label: "B".to_string(),
            source_file: "f.rs".to_string(),
        });
        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        assert!(matches!(first, Event::NodeAdded { ref id, .. } if id == "1"));
        assert!(matches!(second, Event::NodeAdded { ref id, .. } if id == "2"));
    }

    #[test]
    fn test_emit_with_no_subscribers_does_not_panic() {
        let bus = EventBus::new();
        bus.emit(Event::GraphReset);
    }
}
