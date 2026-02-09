#[cfg(feature = "swarm-p2p")]
use std::sync::Arc;
#[cfg(feature = "swarm-p2p")]
use std::collections::HashMap;
#[cfg(feature = "swarm-p2p")]
use async_trait::async_trait;
#[cfg(feature = "swarm-p2p")]
use parking_lot::RwLock;
#[cfg(feature = "swarm-p2p")]
use crate::error::{Error, Result};
#[cfg(feature = "swarm-p2p")]
use crate::agent::swarm::manifest::AgentManifest;
#[cfg(feature = "swarm-p2p")]
use crate::agent::swarm::discovery::Discovery;
#[cfg(feature = "swarm-p2p")]
use libp2p::identity;

/// Discovery implementation using libp2p
#[cfg(feature = "swarm-p2p")]
pub struct P2pDiscovery {
    registry: Arc<RwLock<HashMap<String, AgentManifest>>>,
    // In a real implementation, we would have a handle to the libp2p swarm task here
}

#[cfg(feature = "swarm-p2p")]
impl P2pDiscovery {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Derive a PeerId and Noise keypair from a passphrase
    pub fn derive_keys_from_passphrase(name: &str, passphrase: &str) -> Result<libp2p::identity::Keypair> {
        #[cfg(feature = "swarm-p2p")]
        {
            use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
            
            // Combine name and passphrase for the salt
            let salt = SaltString::from_b64(&base64::encode(name)).map_err(|e| Error::Internal(e.to_string()))?;
            let argon2 = Argon2::default();
            let hash_res = argon2.hash_password(passphrase.as_bytes(), &salt)
                .map_err(|e| Error::Internal(e.to_string()))?;
            
            let hash = hash_res.hash().expect("hash exists");
            let hash_bytes = hash.as_bytes();
            
            // Generate Ed25519 secret key from the derived hash
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&hash_bytes[0..32]);
            
            let keypair = libp2p::identity::Keypair::ed25519_from_bytes(secret)
                .map_err(|e| Error::Internal(format!("Failed to create keypair: {}", e)))?;
                
            Ok(keypair)
        }
        #[cfg(not(feature = "swarm-p2p"))]
        {
            Err(Error::Internal("P2P feature not enabled".to_string()))
        }
    }
}

#[cfg(feature = "swarm-p2p")]
#[async_trait]
impl Discovery for P2pDiscovery {
    async fn register(&self, manifest: AgentManifest) -> Result<()> {
        self.registry.write().insert(manifest.id.clone(), manifest);
        // Logic to announce to Kademlia DHT would go here
        Ok(())
    }

    async fn unregister(&self, agent_id: &str) -> Result<()> {
        self.registry.write().remove(agent_id);
        Ok(())
    }

    async fn get(&self, agent_id: &str) -> Result<Option<AgentManifest>> {
        Ok(self.registry.read().get(agent_id).cloned())
    }

    async fn list(&self) -> Result<Vec<AgentManifest>> {
        Ok(self.registry.read().values().cloned().collect())
    }

    async fn find_by_capability(&self, capability: &str) -> Result<Vec<AgentManifest>> {
        let registry = self.registry.read();
        Ok(registry.values().filter(|m| m.capabilities.contains(capability)).cloned().collect())
    }
}
