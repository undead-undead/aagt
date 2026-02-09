#[cfg(feature = "swarm-p2p")]
use libp2p::{gossipsub, identify, kad, ping, swarm::NetworkBehaviour};

/// Custom libp2p NetworkBehaviour for AAGT swarm
#[cfg(feature = "swarm-p2p")]
#[derive(NetworkBehaviour)]
pub struct AagtBehavior {
    pub gossipsub: gossipsub::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
}
