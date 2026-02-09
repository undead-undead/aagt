#[cfg(feature = "swarm-p2p")]
use std::time::Duration;
#[cfg(feature = "swarm-p2p")]
use libp2p::{
    gossipsub,
    kad,
    identify,
    noise,
    tcp,
    yamux,
    swarm::{Swarm, SwarmEvent},
    identity,
    PeerId,
    Multiaddr,
    futures::StreamExt,
};
#[cfg(feature = "swarm-p2p")]
use tokio::sync::{mpsc, broadcast};
#[cfg(feature = "swarm-p2p")]
use crate::error::{Error, Result};
#[cfg(feature = "swarm-p2p")]
use crate::agent::swarm::protocol::SwarmMessage;
#[cfg(feature = "swarm-p2p")]
use crate::agent::swarm::p2p::behavior::AagtBehavior;
#[cfg(feature = "swarm-p2p")]
use tracing::{info, warn};

#[cfg(feature = "swarm-p2p")]
pub struct P2pService {
    swarm: Swarm<AagtBehavior>,
    bus_tx: broadcast::Sender<SwarmMessage>,
    service_rx: mpsc::Receiver<P2pCommand>,
}

#[cfg(feature = "swarm-p2p")]
pub enum P2pCommand {
    Broadcast(SwarmMessage),
    Dial(Multiaddr),
    Register(String), // id
}

#[cfg(feature = "swarm-p2p")]
impl P2pService {
    pub fn new(
        keypair: identity::Keypair,
        bus_tx: broadcast::Sender<SwarmMessage>,
        service_rx: mpsc::Receiver<P2pCommand>,
    ) -> Result<Self> {
        let peer_id = PeerId::from(keypair.public());
        info!("P2P: Initialization with PeerId: {}", peer_id);

        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            ).map_err(|e| Error::Internal(format!("P2P: Failed to build transport: {}", e)))?
            .with_behaviour(|key| {
                // Gossipsub
                let gossip_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(|e| Error::Internal(e.to_string()))?;
                
                let gossipsub = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossip_config,
                ).map_err(|e| Error::Internal(e.to_string()))?;

                // Kademlia
                let store = kad::store::MemoryStore::new(key.public().to_peer_id());
                let kad_config = kad::Config::default();
                let kademlia = kad::Behaviour::with_config(key.public().to_peer_id(), store, kad_config);

                // Identify
                let identify = identify::Behaviour::new(identify::Config::new(
                    "/aagt/1.0.0".into(),
                    key.public(),
                ));

                Ok(AagtBehavior {
                    gossipsub,
                    kademlia,
                    identify,
                    ping: libp2p::ping::Behaviour::default(),
                })
            }).map_err(|e| Error::Internal(e.to_string()))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(Self { swarm, bus_tx, service_rx })
    }

    pub async fn run(mut self) -> Result<()> {
        info!("P2P: Service starting...");
        
        // Subscribe to a global topic for AAGT swarm
        let topic = gossipsub::IdentTopic::new("aagt-swarm");
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)
            .map_err(|e| Error::Internal(format!("P2P: Failed to subscribe: {}", e)))?;

        loop {
            tokio::select! {
                // Handle libp2p swarm events
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!("P2P: Listening on {}", address);
                    }
                    SwarmEvent::Behaviour(event) => {
                        self.handle_behaviour_event(event).await;
                    }
                    _ => {}
                },
                // Handle commands from the local agent
                Some(cmd) = self.service_rx.recv() => match cmd {
                    P2pCommand::Broadcast(msg) => {
                        let json = serde_json::to_vec(&msg).unwrap();
                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic.clone(), json) {
                            warn!("P2P: Publish failed: {}", e);
                        }
                    }
                    P2pCommand::Dial(addr) => {
                        let _ = self.swarm.dial(addr);
                    }
                    P2pCommand::Register(id) => {
                         // Add record to Kademlia
                         let key = kad::RecordKey::new(&id);
                         let record = kad::Record {
                             key,
                             value: vec![1], // simple presence marker
                             publisher: None,
                             expires: None,
                         };
                         let _ = self.swarm.behaviour_mut().kademlia.put_record(record, kad::Quorum::One);
                    }
                }
            }
        }
    }

    async fn handle_behaviour_event(&mut self, event: crate::agent::swarm::p2p::behavior::AagtBehaviorEvent) {
        match event {
            crate::agent::swarm::p2p::behavior::AagtBehaviorEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                if let Ok(msg) = serde_json::from_slice::<SwarmMessage>(&message.data) {
                    let _ = self.bus_tx.send(msg);
                }
            }
            crate::agent::swarm::p2p::behavior::AagtBehaviorEvent::Kademlia(kad::Event::OutboundQueryProgressed { result, .. }) => {
                // Handle Kademlia query results
            }
            _ => {}
        }
    }
}
