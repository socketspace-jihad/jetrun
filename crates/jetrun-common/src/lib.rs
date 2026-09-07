pub mod config;
pub mod error;
pub mod models;

pub mod proto {
    tonic::include_proto!("jetrun");
}
