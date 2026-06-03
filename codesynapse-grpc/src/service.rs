use std::pin::Pin;
use std::sync::{Arc, RwLock};

use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};

use crate::event_bus::{Event, EventBus};
use crate::proto::graph_service_server::GraphService;
use crate::proto::{
    Edge, GetGraphRequest, GetGraphResponse, GetNodeRequest, GetNodeResponse, GraphEvent, Node,
    SearchNodesRequest, SearchNodesResponse, ShortestPathRequest, ShortestPathResponse,
    WatchGraphRequest,
};
use crate::state::{GraphEdge, GraphNode, GraphState};

pub struct GraphServiceImpl {
    state: Arc<RwLock<GraphState>>,
    bus: Arc<EventBus>,
}

impl GraphServiceImpl {
    pub fn new(state: Arc<RwLock<GraphState>>, bus: Arc<EventBus>) -> Self {
        Self { state, bus }
    }

    pub fn add_node(&self, node: GraphNode) {
        let ev = Event::NodeAdded {
            id: node.id.clone(),
            label: node.label.clone(),
            source_file: node.source_file.clone(),
        };
        self.state.write().unwrap().add_node(node);
        self.bus.emit(ev);
    }

    pub fn add_edge(&self, edge: GraphEdge) {
        let ev = Event::EdgeAdded {
            source: edge.source.clone(),
            target: edge.target.clone(),
            relation: edge.relation.clone(),
        };
        self.state.write().unwrap().add_edge(edge);
        self.bus.emit(ev);
    }
}

fn node_to_proto(n: &GraphNode) -> Node {
    Node {
        id: n.id.clone(),
        label: n.label.clone(),
        source_file: n.source_file.clone(),
        source_location: n.source_location.clone(),
        community: n.community,
    }
}

fn edge_to_proto(e: &GraphEdge) -> Edge {
    Edge {
        source: e.source.clone(),
        target: e.target.clone(),
        relation: e.relation.clone(),
        confidence: e.confidence.clone(),
    }
}

fn event_to_proto(ev: Event) -> Option<GraphEvent> {
    use crate::proto::graph_event::Event as PEvent;
    let inner = match ev {
        Event::NodeAdded {
            id,
            label,
            source_file,
        } => PEvent::NodeAdded(Node {
            id,
            label,
            source_file,
            source_location: String::new(),
            community: 0,
        }),
        Event::EdgeAdded {
            source,
            target,
            relation,
        } => PEvent::EdgeAdded(Edge {
            source,
            target,
            relation,
            confidence: String::new(),
        }),
        Event::NodeRemoved { id } => PEvent::NodeRemoved(id),
        Event::GraphReset => PEvent::GraphReset("reset".to_string()),
    };
    Some(GraphEvent { event: Some(inner) })
}

pub(crate) type BoxStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl GraphService for GraphServiceImpl {
    async fn get_graph(
        &self,
        _req: Request<GetGraphRequest>,
    ) -> Result<Response<GetGraphResponse>, Status> {
        let state = self.state.read().unwrap();
        let nodes = state.get_nodes().iter().map(node_to_proto).collect();
        let edges = state.get_edges().iter().map(edge_to_proto).collect();
        Ok(Response::new(GetGraphResponse { nodes, edges }))
    }

    async fn get_node(
        &self,
        req: Request<GetNodeRequest>,
    ) -> Result<Response<GetNodeResponse>, Status> {
        let id = req.into_inner().id;
        let state = self.state.read().unwrap();
        match state.get_node(&id) {
            Some(n) => Ok(Response::new(GetNodeResponse {
                node: Some(node_to_proto(n)),
            })),
            None => Err(Status::not_found(format!("node {} not found", id))),
        }
    }

    async fn search_nodes(
        &self,
        req: Request<SearchNodesRequest>,
    ) -> Result<Response<SearchNodesResponse>, Status> {
        let inner = req.into_inner();
        let limit = if inner.limit <= 0 {
            20
        } else {
            inner.limit as usize
        };
        let state = self.state.read().unwrap();
        let nodes = state
            .search_nodes(&inner.query, limit)
            .iter()
            .map(node_to_proto)
            .collect();
        Ok(Response::new(SearchNodesResponse { nodes }))
    }

    async fn shortest_path(
        &self,
        req: Request<ShortestPathRequest>,
    ) -> Result<Response<ShortestPathResponse>, Status> {
        let inner = req.into_inner();
        let state = self.state.read().unwrap();
        match state.shortest_path(&inner.source, &inner.target) {
            Some(path) => Ok(Response::new(ShortestPathResponse {
                node_ids: path,
                found: true,
            })),
            None => Ok(Response::new(ShortestPathResponse {
                node_ids: vec![],
                found: false,
            })),
        }
    }

    type WatchGraphStream = BoxStream<GraphEvent>;

    async fn watch_graph(
        &self,
        _req: Request<WatchGraphRequest>,
    ) -> Result<Response<Self::WatchGraphStream>, Status> {
        let (tx, rx) = mpsc::channel(128);
        let mut sub = self.bus.subscribe();
        tokio::spawn(async move {
            while let Ok(ev) = sub.recv().await {
                if let Some(proto_ev) = event_to_proto(ev) {
                    if tx.send(Ok(proto_ev)).await.is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{GetGraphRequest, GetNodeRequest, SearchNodesRequest, ShortestPathRequest};
    use tokio_stream::StreamExt;

    fn make_service() -> GraphServiceImpl {
        let state = Arc::new(RwLock::new(GraphState::new()));
        let bus = Arc::new(EventBus::new());
        GraphServiceImpl::new(state, bus)
    }

    fn node(id: &str, label: &str) -> GraphNode {
        GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            source_file: "test.rs".to_string(),
            source_location: "1:1".to_string(),
            community: 0,
        }
    }

    fn edge(src: &str, tgt: &str) -> GraphEdge {
        GraphEdge {
            source: src.to_string(),
            target: tgt.to_string(),
            relation: "calls".to_string(),
            confidence: "1.0".to_string(),
        }
    }

    #[tokio::test]
    async fn test_get_graph_empty() {
        let svc = make_service();
        let resp = svc
            .get_graph(Request::new(GetGraphRequest {}))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(inner.nodes.is_empty());
        assert!(inner.edges.is_empty());
    }

    #[tokio::test]
    async fn test_get_graph_with_nodes_and_edges() {
        let svc = make_service();
        svc.add_node(node("a", "Alpha"));
        svc.add_node(node("b", "Beta"));
        svc.add_edge(edge("a", "b"));

        let resp = svc
            .get_graph(Request::new(GetGraphRequest {}))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert_eq!(inner.nodes.len(), 2);
        assert_eq!(inner.edges.len(), 1);
    }

    #[tokio::test]
    async fn test_get_node_found() {
        let svc = make_service();
        svc.add_node(node("n1", "Foo"));

        let resp = svc
            .get_node(Request::new(GetNodeRequest {
                id: "n1".to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().node.unwrap().label, "Foo");
    }

    #[tokio::test]
    async fn test_get_node_not_found_returns_status_not_found() {
        let svc = make_service();
        let err = svc
            .get_node(Request::new(GetNodeRequest {
                id: "missing".to_string(),
            }))
            .await
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_search_nodes_matches_substring() {
        let svc = make_service();
        svc.add_node(node("1", "AuthService"));
        svc.add_node(node("2", "AuthController"));
        svc.add_node(node("3", "UserService"));

        let resp = svc
            .search_nodes(Request::new(SearchNodesRequest {
                query: "auth".to_string(),
                limit: 10,
            }))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_search_nodes_limit_respected() {
        let svc = make_service();
        for i in 0..5 {
            svc.add_node(node(&i.to_string(), &format!("Service{}", i)));
        }
        let resp = svc
            .search_nodes(Request::new(SearchNodesRequest {
                query: "service".to_string(),
                limit: 2,
            }))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_search_nodes_zero_limit_defaults_to_20() {
        let svc = make_service();
        for i in 0..25 {
            svc.add_node(node(&i.to_string(), &format!("Node{}", i)));
        }
        let resp = svc
            .search_nodes(Request::new(SearchNodesRequest {
                query: "node".to_string(),
                limit: 0,
            }))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().nodes.len(), 20);
    }

    #[tokio::test]
    async fn test_shortest_path_found() {
        let svc = make_service();
        svc.add_node(node("a", "A"));
        svc.add_node(node("b", "B"));
        svc.add_node(node("c", "C"));
        svc.add_edge(edge("a", "b"));
        svc.add_edge(edge("b", "c"));

        let resp = svc
            .shortest_path(Request::new(ShortestPathRequest {
                source: "a".to_string(),
                target: "c".to_string(),
            }))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(inner.found);
        assert_eq!(inner.node_ids, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_shortest_path_not_found() {
        let svc = make_service();
        svc.add_node(node("x", "X"));
        svc.add_node(node("y", "Y"));

        let resp = svc
            .shortest_path(Request::new(ShortestPathRequest {
                source: "x".to_string(),
                target: "y".to_string(),
            }))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(!inner.found);
        assert!(inner.node_ids.is_empty());
    }

    #[tokio::test]
    async fn test_watch_graph_receives_node_added_event() {
        let svc = make_service();
        let resp = svc
            .watch_graph(Request::new(crate::proto::WatchGraphRequest {}))
            .await
            .unwrap();
        let mut stream = resp.into_inner();

        svc.add_node(node("w1", "Watcher"));

        let event = tokio::time::timeout(std::time::Duration::from_millis(200), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream ended")
            .unwrap();

        assert!(matches!(
            event.event,
            Some(crate::proto::graph_event::Event::NodeAdded(_))
        ));
    }
}
