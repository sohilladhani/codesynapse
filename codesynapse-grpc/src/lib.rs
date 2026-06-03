pub mod proto {
    tonic::include_proto!("codesynapse");
}

pub mod event_bus;
pub mod service;
pub mod state;
