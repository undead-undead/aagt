#[cfg(feature = "swarm-p2p")]
pub mod behavior;
#[cfg(feature = "swarm-p2p")]
pub mod discovery;
#[cfg(feature = "swarm-p2p")]
pub mod service;

#[cfg(feature = "swarm-p2p")]
pub use discovery::P2pDiscovery;
#[cfg(feature = "swarm-p2p")]
pub use service::{P2pCommand, P2pService};
