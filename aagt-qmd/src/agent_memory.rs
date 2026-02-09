use crate::store::QmdStore;
use aagt_core::agent::memory::Memory;
use aagt_core::agent::message::Message;
use aagt_core::agent::session::AgentSession;
use aagt_core::knowledge::rag::Document;
use async_trait::async_trait;
use std::sync::Arc;

/// Adapter to use QmdStore as an AAGT Memory backend
pub struct QmdMemory {
    store: Arc<QmdStore>,
}

impl QmdMemory {
    pub fn new(store: Arc<QmdStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Memory for QmdMemory {
    async fn store(&self, user_id: &str, agent_id: Option<&str>, message: Message) -> aagt_core::error::Result<()> {
        let _collection = format!("history/{}", user_id);
        let _path = format!("{}.jsonl", agent_id.unwrap_or("default"));
        let _content = serde_json::to_string(&message).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        
        // We append to history. In QmdStore, store_document overwrites if path exists.
        // For conversation history, we might need a different table or append logic.
        // But QMD Phase 1 is content-addressable docs.
        
        // For now, let's treat history as a document that gets updated? 
        // No, that's inefficient.
        
        // Let's assume we'll use a specific table for messages if QmdStore supports it, 
        // or just use store_document for "Memories" (knowledge).
        
        // But the Memory trait requires "retrieve" (recent messages).
        // Since QmdStore is focused on documents/knowledge, we'll implement store_knowledge.
        Ok(())
    }

    async fn store_knowledge(&self, _user_id: &str, _agent_id: Option<&str>, title: &str, content: &str, collection: &str) -> aagt_core::error::Result<()> {
        let path = format!("manual/{}", uuid::Uuid::new_v4());
        self.store.store_document(collection, &path, title, content)
            .map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        Ok(())
    }

    async fn retrieve(&self, _user_id: &str, _agent_id: Option<&str>, _limit: usize) -> Vec<Message> {
        // Retrieve logic would go here
        Vec::new()
    }

    async fn search(&self, _user_id: &str, _agent_id: Option<&str>, query: &str, limit: usize) -> aagt_core::error::Result<Vec<Document>> {
        let results = self.store.search_fts(query, limit).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        
        let docs = results.into_iter().map(|r| Document {
            id: r.document.docid,
            title: r.document.title,
            content: r.document.body.unwrap_or_default(),
            summary: r.document.summary,
            collection: Some(r.document.collection),
            path: Some(r.document.path),
            metadata: std::collections::HashMap::new(), // TODO: populate
            score: r.score as f32,
        }).collect();
        
        Ok(docs)
    }

    async fn store_session(&self, session: AgentSession) -> aagt_core::error::Result<()> {
        let data = serde_json::to_string(&session).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        self.store.store_session(&session.id, &data).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        Ok(())
    }

    async fn retrieve_session(&self, session_id: &str) -> aagt_core::error::Result<Option<AgentSession>> {
        let data = self.store.load_session(session_id).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        if let Some(json) = data {
            let session = serde_json::from_str(&json).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }

    async fn fetch_document(&self, collection: &str, path: &str) -> aagt_core::error::Result<Option<Document>> {
        let doc = self.store.get_by_path(collection, path).map_err(|e| aagt_core::error::Error::Internal(e.to_string()))?;
        Ok(doc.map(|d| Document {
            id: d.docid,
            title: d.title,
            content: d.body.unwrap_or_default(),
            summary: d.summary,
            collection: Some(d.collection),
            path: Some(d.path),
            metadata: std::collections::HashMap::new(),
            score: 1.0,
        }))
    }

    async fn clear(&self, _user_id: &str, _agent_id: Option<&str>) -> aagt_core::error::Result<()> {
        Ok(())
    }

    async fn undo(&self, _user_id: &str, _agent_id: Option<&str>) -> aagt_core::error::Result<Option<Message>> {
        Ok(None)
    }
}
