use crate::hybrid_search::HybridSearchEngine;
use crate::error::Result;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, error};

/// Watcher for Active Indexing
pub struct FileWatcher {
    engine: Arc<HybridSearchEngine>,
    watcher: Option<notify::RecommendedWatcher>,
}

impl FileWatcher {
    /// Create a new watcher linked to a search engine
    pub fn new(engine: Arc<HybridSearchEngine>) -> Self {
        Self {
            engine,
            watcher: None,
        }
    }

    /// Start watching a directory for changes
    pub async fn watch(&mut self, path: impl AsRef<Path>, collection: String) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        let (tx, mut rx) = mpsc::channel(100);
        let engine = self.engine.clone();
        let collection_name = collection.clone();

        // Initial crawl to ensure everything is indexed
        self.crawl(&path, &collection_name).await?;

        // Setup notification channel
        let mut watcher = notify::RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            notify::Config::default(),
        ).map_err(|e| crate::error::QmdError::Custom(format!("Failed to create watcher: {}", e)))?;

        watcher.watch(&path, RecursiveMode::Recursive)
            .map_err(|e| crate::error::QmdError::Custom(format!("Failed to start watch: {}", e)))?;

        self.watcher = Some(watcher);

        // Background task to process events
        tokio::spawn(async move {
            info!("Watcher started for {:?} (Collection: {})", path, collection_name);
            while let Some(event) = rx.recv().await {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) => {
                        for path in event.paths {
                            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                                info!("Detected change in {:?}, re-indexing...", path);
                                if let Err(e) = Self::index_file(&engine, &collection_name, &path).await {
                                    error!("Failed to re-index {:?}: {}", path, e);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    /// Crawl the directory and index all .md files
    async fn crawl(&self, root: &Path, collection: &str) -> Result<()> {
        info!("Crawling directory for initial indexing: {:?}", root);
        let pattern = format!("{}/**/*.md", root.to_string_lossy());
        for entry in glob::glob(&pattern).map_err(|e| crate::error::QmdError::Custom(e.to_string()))? {
            if let Ok(path_buf) = entry {
                Self::index_file(&self.engine, collection, &path_buf).await?;
            }
        }
        Ok(())
    }

    /// Helper to index a single file
    async fn index_file(engine: &HybridSearchEngine, collection: &str, path: &Path) -> Result<()> {
        let content = tokio::fs::read_to_string(path).await
            .map_err(|e| crate::error::QmdError::Io(e))?;
        
        let title = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
            
        let relative_path = path.to_string_lossy().to_string();

        engine.index_document(collection, &relative_path, &title, &content)?;
        Ok(())
    }
}
