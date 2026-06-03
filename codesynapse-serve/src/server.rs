use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use codesynapse_core::error::{CodeSynapseError, Result};
use codesynapse_grpc::event_bus::EventBus;
use codesynapse_grpc::proto::graph_service_server::GraphServiceServer;
use codesynapse_grpc::service::GraphServiceImpl;
use codesynapse_grpc::state::GraphState;

pub async fn start_grpc(addr: SocketAddr) -> std::result::Result<(), tonic::transport::Error> {
    let state = Arc::new(RwLock::new(GraphState::new()));
    let bus = Arc::new(EventBus::new());
    let svc = GraphServiceImpl::new(state, bus);

    tonic::transport::Server::builder()
        .add_service(GraphServiceServer::new(svc))
        .serve(addr)
        .await
}

pub fn start_grpc_blocking(addr: SocketAddr) -> Result<()> {
    let rt = tokio::runtime::Runtime::new().map_err(CodeSynapseError::Io)?;
    rt.block_on(start_grpc(addr))
        .map_err(|e| CodeSynapseError::Other(e.to_string()))
}
