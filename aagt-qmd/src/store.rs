use crate::content_hash::{get_docid, hash_content, normalize_docid, validate_docid};
use crate::error::{QmdError, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info};

/// Document metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Option<i64>,
    pub collection: String,
    pub path: String,
    pub title: String,
    pub hash: String,
    pub docid: String,
    pub body: Option<String>,
    pub summary: Option<String>,
    pub created_at: String,
    pub modified_at: String,
    pub active: bool,
}

/// Search result with score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document: Document,
    pub score: f64,
    pub snippet: Option<String>,
}

/// Collection metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    pub description: Option<String>,
    pub glob_pattern: String,
    pub root_path: Option<PathBuf>,
}

use std::sync::Mutex;

/// QMD Store - Core storage engine
pub struct QmdStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

const MAX_CONTENT_SIZE: usize = 10 * 1024 * 1024; // 10MB limit

impl QmdStore {
    /// Create or open a QMD store at the given path
    pub fn new(db_path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = db_path.into();
        info!("Opening QMD store at: {:?}", db_path);

        // Create parent directory if needed
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        let store = Self {
            conn: Mutex::new(conn),
            db_path,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        debug!("Initializing QMD schema");
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        // Enable WAL mode for better concurrency
        conn.execute_batch("PRAGMA journal_mode = WAL")?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;

        // Content-addressable storage (source of truth)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS content (
                hash TEXT PRIMARY KEY,
                doc TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // Documents table (filesystem layer)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                collection TEXT NOT NULL,
                path TEXT NOT NULL,
                title TEXT NOT NULL,
                hash TEXT NOT NULL,
                summary TEXT,
                created_at TEXT NOT NULL,
                modified_at TEXT NOT NULL,
                active INTEGER NOT NULL DEFAULT 1,
                FOREIGN KEY (hash) REFERENCES content(hash) ON DELETE CASCADE,
                UNIQUE(collection, path)
            )",
            [],
        )?;

        // Migration: Add summary column if it doesn't exist (for existing DBs)
        let has_summary: bool = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('documents') WHERE name='summary'",
            [],
            |row| row.get::<_, i64>(0).map(|c| c > 0),
        )?;

        if !has_summary {
            debug!("Migrating: Adding 'summary' column to 'documents' table");
            conn.execute("ALTER TABLE documents ADD COLUMN summary TEXT", [])?;
        }

        // Indexes for fast lookup
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash)",
            [],
        )?;

        // Collections table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS collections (
                name TEXT PRIMARY KEY,
                description TEXT,
                glob_pattern TEXT NOT NULL DEFAULT '**/*.md',
                root_path TEXT,
                created_at TEXT NOT NULL
            )",
            [],
        )?;

        // FTS5 full-text index
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
                filepath, title, body,
                tokenize='porter unicode61'
            )",
        )?;

        // Triggers to keep FTS in sync with documents
        self.create_fts_triggers_internal(&conn)?;

        // Sessions table for agent state persistence
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        info!("QMD schema initialized successfully");
        Ok(())
    }

    /// Create FTS sync triggers
    fn create_fts_triggers_internal(&self, conn: &Connection) -> Result<()> {
        // Insert trigger
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents
            WHEN new.active = 1
            BEGIN
                INSERT INTO documents_fts(rowid, filepath, title, body)
                SELECT new.id, new.collection || '/' || new.path, new.title,
                       (SELECT doc FROM content WHERE hash = new.hash)
                WHERE new.active = 1;
            END",
        )?;

        // Update trigger
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents
            BEGIN
                DELETE FROM documents_fts WHERE rowid = old.id AND new.active = 0;
                
                INSERT OR REPLACE INTO documents_fts(rowid, filepath, title, body)
                SELECT new.id, new.collection || '/' || new.path, new.title,
                       (SELECT doc FROM content WHERE hash = new.hash)
                WHERE new.active = 1;
            END",
        )?;

        // Delete trigger
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents
            BEGIN
                DELETE FROM documents_fts WHERE rowid = old.id;
            END",
        )?;

        Ok(())
    }

    /// Store a document with content-addressable storage
    pub fn store_document(
        &self,
        collection: &str,
        path: &str,
        title: &str,
        body: &str,
    ) -> Result<Document> {
        if body.len() > MAX_CONTENT_SIZE {
            return Err(QmdError::Custom(format!(
                "Document too large: {} bytes (max {} bytes)",
                body.len(),
                MAX_CONTENT_SIZE
            )));
        }

        let hash = hash_content(body);
        let docid = get_docid(&hash);
        let now = Utc::now().to_rfc3339();

        debug!(
            "Storing document: {}/{} (docid: #{})",
            collection, path, docid
        );

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        // Begin transaction
        let tx = conn.transaction()?;

        // 1. Store content (content-addressable, auto-dedup)
        tx.execute(
            "INSERT OR IGNORE INTO content (hash, doc, created_at) VALUES (?, ?, ?)",
            params![hash, body, now],
        )?;

        // 2. Check if document exists
        let existing: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT id, hash, modified_at FROM documents 
                 WHERE collection = ? AND path = ?",
                params![collection, path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let doc_id = if let Some((id, old_hash, _old_modified)) = existing {
            if old_hash == hash {
                // Content unchanged, just update modified_at and title
                debug!("Content unchanged, updating metadata only");
                tx.execute(
                    "UPDATE documents SET title = ?, modified_at = ? WHERE id = ?",
                    params![title, now, id],
                )?;
            } else {
                // Content changed, update document
                debug!("Content changed, updating document");
                tx.execute(
                    "UPDATE documents SET title = ?, hash = ?, modified_at = ?, summary = NULL WHERE id = ?",
                    params![title, hash, now, id],
                )?;
            }
            id
        } else {
            // New document, insert
            debug!("New document, inserting");
            tx.execute(
                "INSERT INTO documents (collection, path, title, hash, created_at, modified_at, active)
                 VALUES (?, ?, ?, ?, ?, ?, 1)",
                params![collection, path, title, hash, now, now],
            )?;
            tx.last_insert_rowid()
        };

        tx.commit()?;

        Ok(Document {
            id: Some(doc_id),
            collection: collection.to_string(),
            path: path.to_string(),
            title: title.to_string(),
            hash: hash.clone(),
            docid,
            body: Some(body.to_string()),
            summary: None, // Summary is generated asynchronously
            created_at: now.clone(),
            modified_at: now,
            active: true,
        })
    }

    /// Get document by virtual path
    pub fn get_by_path(&self, collection: &str, path: &str) -> Result<Option<Document>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let row = conn
            .query_row(
                "SELECT d.id, d.collection, d.path, d.title, d.hash, d.created_at, d.modified_at,
                        d.active, c.doc, d.summary
                 FROM documents d
                 JOIN content c ON d.hash = c.hash
                 WHERE d.collection = ? AND d.path = ? AND d.active = 1",
                params![collection, path],
                |row| {
                    Ok(Document {
                        id: Some(row.get(0)?),
                        collection: row.get(1)?,
                        path: row.get(2)?,
                        title: row.get(3)?,
                        hash: row.get(4)?,
                        docid: get_docid(&row.get::<_, String>(4)?),
                        created_at: row.get(5)?,
                        modified_at: row.get(6)?,
                        active: row.get(7)?,
                        body: Some(row.get(8)?),
                        summary: row.get(9)?,
                    })
                },
            )
            .optional()?;

        Ok(row)
    }

    /// Get document by docid (short hash)
    pub fn get_by_docid(&self, docid: &str) -> Result<Option<Document>> {
        let normalized = normalize_docid(docid);

        if !validate_docid(&normalized) {
            return Err(QmdError::InvalidDocid(docid.to_string()));
        }

        let pattern = format!("{}%", normalized);
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        let row = conn
            .query_row(
                "SELECT d.id, d.collection, d.path, d.title, d.hash, d.created_at, d.modified_at,
                        d.active, c.doc, d.summary
                 FROM documents d
                 JOIN content c ON d.hash = c.hash
                 WHERE d.hash LIKE ? AND d.active = 1
                 LIMIT 1",
                params![pattern],
                |row| {
                    Ok(Document {
                        id: Some(row.get(0)?),
                        collection: row.get(1)?,
                        path: row.get(2)?,
                        title: row.get(3)?,
                        hash: row.get(4)?,
                        docid: get_docid(&row.get::<_, String>(4)?),
                        created_at: row.get(5)?,
                        modified_at: row.get(6)?,
                        active: row.get(7)?,
                        body: Some(row.get(8)?),
                        summary: row.get(9)?,
                    })
                },
            )
            .optional()?;

        Ok(row)
    }

    /// BM25 full-text search
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT d.id, d.collection, d.path, d.title, d.hash, d.created_at, d.modified_at,
                    d.active, bm25(documents_fts) as score,
                    snippet(documents_fts, 2, '<mark>', '</mark>', '...', 32) as snippet,
                    d.summary
             FROM documents d
             JOIN documents_fts ON documents_fts.rowid = d.id
             WHERE documents_fts MATCH ? AND d.active = 1
             ORDER BY score
             LIMIT ?",
        )?;

        let results = stmt
            .query_map(params![query, limit], |row| {
                let hash: String = row.get(4)?;
                Ok(SearchResult {
                    document: Document {
                        id: Some(row.get(0)?),
                        collection: row.get(1)?,
                        path: row.get(2)?,
                        title: row.get(3)?,
                        hash: hash.clone(),
                        docid: get_docid(&hash),
                        created_at: row.get(5)?,
                        modified_at: row.get(6)?,
                        active: row.get(7)?,
                        body: None, // Don't load body in search results
                        summary: row.get(10)?,
                    },
                    score: row.get::<_, f64>(8)?.abs(), // BM25 score (absolute value)
                    snippet: Some(row.get(9)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Search within a specific collection
    pub fn search_fts_in_collection(
        &self,
        query: &str,
        collection: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT d.id, d.collection, d.path, d.title, d.hash, d.created_at, d.modified_at,
                    d.active, bm25(documents_fts) as score,
                    snippet(documents_fts, 2, '<mark>', '</mark>', '...', 32) as snippet,
                    d.summary
             FROM documents d
             JOIN documents_fts ON documents_fts.rowid = d.id
             WHERE documents_fts MATCH ? AND d.collection = ? AND d.active = 1
             ORDER BY score
             LIMIT ?",
        )?;

        let results = stmt
            .query_map(params![query, collection, limit], |row| {
                let hash: String = row.get(4)?;
                Ok(SearchResult {
                    document: Document {
                        id: Some(row.get(0)?),
                        collection: row.get(1)?,
                        path: row.get(2)?,
                        title: row.get(3)?,
                        hash: hash.clone(),
                        docid: get_docid(&hash),
                        created_at: row.get(5)?,
                        modified_at: row.get(6)?,
                        active: row.get(7)?,
                        body: None,
                        summary: row.get(10)?,
                    },
                    score: row.get::<_, f64>(8)?.abs(),
                    snippet: Some(row.get(9)?),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    /// Create a collection
    pub fn create_collection(&self, collection: Collection) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        conn.execute(
            "INSERT OR REPLACE INTO collections (name, description, glob_pattern, root_path, created_at)
             VALUES (?, ?, ?, ?, ?)",
            params![
                collection.name,
                collection.description,
                collection.glob_pattern,
                collection.root_path.map(|p| p.to_string_lossy().to_string()),
                now
            ],
        )?;

        info!("Created collection: {}", collection.name);
        Ok(())
    }

    /// List all collections
    pub fn list_collections(&self) -> Result<Vec<Collection>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let mut stmt =
            conn.prepare("SELECT name, description, glob_pattern, root_path FROM collections")?;

        let collections = stmt
            .query_map([], |row| {
                Ok(Collection {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    glob_pattern: row.get(2)?,
                    root_path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(collections)
    }

    /// Get index statistics
    pub fn get_stats(&self) -> Result<StoreStats> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let total_docs: i64 = conn.query_row(
            "SELECT COUNT(*) FROM documents WHERE active = 1",
            [],
            |row| row.get(0),
        )?;

        let total_collections: i64 =
            conn.query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))?;

        let total_content: i64 =
            conn.query_row("SELECT COUNT(*) FROM content", [], |row| row.get(0))?;

        let db_size = std::fs::metadata(&self.db_path)?.len();

        Ok(StoreStats {
            total_documents: total_docs as usize,
            total_collections: total_collections as usize,
            total_unique_content: total_content as usize,
            database_size_bytes: db_size,
        })
    }

    /// Vacuum database (reclaim space)
    pub fn vacuum(&self) -> Result<()> {
        info!("Vacuuming database");
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    /// Garbage collect orphaned content
    ///
    /// Deletes content blobs that are no longer referenced by any document.
    /// This should be called periodically to free disk space.
    pub fn vacuum_content(&self) -> Result<usize> {
        info!("Vacuuming orphaned content");

        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let tx = conn.unchecked_transaction()?;

        // Find and delete orphaned content
        // sqlite doesn't support DELETE ... JOIN properly in all versions,
        // using subquery is safer standard SQL
        let deleted_count = tx.execute(
            "DELETE FROM content 
             WHERE hash NOT IN (SELECT hash FROM documents)",
            [],
        )?;

        tx.commit()?;

        if deleted_count > 0 {
            info!("Deleted {} orphaned content blobs", deleted_count);
        }

        Ok(deleted_count)
    }

    /// Update the summary for a document
    pub fn update_summary(&self, collection: &str, path: &str, summary: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        conn.execute(
            "UPDATE documents SET summary = ? WHERE collection = ? AND path = ?",
            params![summary, collection, path],
        )?;

        Ok(())
    }

    /// Store an agent session (JSON blob)
    pub fn store_session(&self, id: &str, data: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, data, updated_at) VALUES (?, ?, ?)",
            params![id, data, now],
        )?;

        Ok(())
    }

    /// Load an agent session
    pub fn load_session(&self, id: &str) -> Result<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        let data: Option<String> = conn
            .query_row(
                "SELECT data FROM sessions WHERE id = ?",
                params![id],
                |row| row.get(0),
            )
            .optional()?;

        Ok(data)
    }

    /// Delete a session
    pub fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| QmdError::Custom("Lock poisoned".to_string()))?;

        conn.execute("DELETE FROM sessions WHERE id = ?", params![id])?;

        Ok(())
    }
}

/// Store statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoreStats {
    pub total_documents: usize,
    pub total_collections: usize,
    pub total_unique_content: usize,
    pub database_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (QmdStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let store = QmdStore::new(db_path).unwrap();
        (store, temp_dir)
    }

    #[test]
    fn test_store_and_retrieve() {
        let (store, _temp) = create_test_store();

        // Store a document
        let doc = store
            .store_document(
                "trading",
                "strategies/sol.md",
                "SOL Trading Strategy",
                "Buy SOL when RSI < 30",
            )
            .unwrap();

        assert_eq!(doc.collection, "trading");
        assert_eq!(doc.path, "strategies/sol.md");
        assert_eq!(doc.title, "SOL Trading Strategy");
        assert_eq!(doc.docid.len(), 6);

        // Retrieve by path
        let retrieved = store
            .get_by_path("trading", "strategies/sol.md")
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.title, "SOL Trading Strategy");
        assert_eq!(retrieved.body.unwrap(), "Buy SOL when RSI < 30");
        assert!(retrieved.summary.is_none());

        // Update summary
        store
            .update_summary("trading", "strategies/sol.md", "Short summary")
            .unwrap();
        let updated = store
            .get_by_path("trading", "strategies/sol.md")
            .unwrap()
            .unwrap();
        assert_eq!(updated.summary.unwrap(), "Short summary");

        // Retrieve by docid
        let by_docid = store.get_by_docid(&doc.docid).unwrap().unwrap();
        assert_eq!(by_docid.title, "SOL Trading Strategy");
    }

    #[test]
    fn test_content_deduplication() {
        let (store, _temp) = create_test_store();

        // Store same content twice
        let doc1 = store
            .store_document("trading", "doc1.md", "Title 1", "Same content")
            .unwrap();

        let doc2 = store
            .store_document("trading", "doc2.md", "Title 2", "Same content")
            .unwrap();

        // Same content hash
        assert_eq!(doc1.hash, doc2.hash);

        // But different docids (different paths)
        let stats = store.get_stats().unwrap();
        assert_eq!(stats.total_documents, 2);
        assert_eq!(stats.total_unique_content, 1); // ← Deduplication!
    }

    #[test]
    fn test_fts_search() {
        let (store, _temp) = create_test_store();

        // Index some documents
        store
            .store_document("trading", "sol.md", "SOL Strategy", "Buy SOL when RSI < 30")
            .unwrap();
        store
            .store_document("trading", "eth.md", "ETH Strategy", "Buy ETH on dips")
            .unwrap();
        store
            .store_document(
                "notes",
                "meeting.md",
                "Meeting Notes",
                "Discuss SOL integration",
            )
            .unwrap();

        // Search for "SOL"
        let results = store.search_fts("SOL", 10).unwrap();
        assert_eq!(results.len(), 2); // sol.md and meeting.md

        // Search within collection
        let trading_only = store
            .search_fts_in_collection("SOL", "trading", 10)
            .unwrap();
        assert_eq!(trading_only.len(), 1); // Only sol.md
        assert!(trading_only[0].document.path.contains("sol.md"));
    }

    #[test]
    fn test_store_document_too_large() {
        let (mut store, _temp) = create_test_store();

        let large_content = "a".repeat(10 * 1024 * 1024 + 1);

        let result = store.store_document("test", "large.md", "Large", &large_content);

        assert!(result.is_err());
        match result {
            Err(QmdError::Custom(msg)) => assert!(msg.contains("Document too large")),
            _ => panic!("Expected Custom error for large document"),
        }
    }
}
