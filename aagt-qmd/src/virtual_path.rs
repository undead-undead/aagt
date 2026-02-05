use crate::error::{QmdError, Result};
use serde::{Deserialize, Serialize};

/// Virtual path: aagt://collection/path/to/file.md
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualPath {
    pub collection: String,
    pub path: String,
}

impl VirtualPath {
    /// Parse virtual path from string
    ///
    /// Supports formats:
    /// - `aagt://collection/path.md`
    /// - `//collection/path.md` (missing prefix)
    /// - `aagt:////collection/path.md` (extra slashes)
    ///
    /// # Examples
    ///
    /// ```
    /// # use aagt_qmd::virtual_path::VirtualPath;
    /// let vpath = VirtualPath::parse("aagt://trading/strategies/sol.md").unwrap();
    /// assert_eq!(vpath.collection, "trading");
    /// assert_eq!(vpath.path, "strategies/sol.md");
    /// ```
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();

        // Normalize: aagt:// with any number of slashes
        let normalized = if let Some(rest) = trimmed.strip_prefix("aagt:") {
            format!("aagt://{}", rest.trim_start_matches('/'))
        } else if let Some(rest) = trimmed.strip_prefix("//") {
            format!("aagt://{}", rest)
        } else {
            trimmed.to_string()
        };

        // Security check: Prevent path traversal by checking components
        if normalized
            .split('/')
            .any(|part| part == ".." || part == ".")
        {
            return Err(QmdError::InvalidVirtualPath(format!(
                "Path traversal detected in virtual path: {}",
                input
            )));
        }

        // Parse: aagt://collection/path
        if let Some(rest) = normalized.strip_prefix("aagt://") {
            let parts: Vec<&str> = rest.splitn(2, '/').collect();

            if parts.is_empty() || parts[0].is_empty() {
                return Err(QmdError::InvalidVirtualPath(
                    "Empty collection name".to_string(),
                ));
            }

            Ok(VirtualPath {
                collection: parts[0].to_string(),
                path: parts.get(1).unwrap_or(&"").to_string(),
            })
        } else {
            Err(QmdError::InvalidVirtualPath(format!(
                "Invalid virtual path format: {}",
                input
            )))
        }
    }

    /// Build virtual path from components
    ///
    /// # Examples
    ///
    /// ```
    /// # use aagt_qmd::virtual_path::VirtualPath;
    /// let vpath = VirtualPath::build("trading", "strategies/sol.md");
    /// assert_eq!(vpath, "aagt://trading/strategies/sol.md");
    /// ```
    pub fn build(collection: &str, path: &str) -> String {
        if path.is_empty() {
            format!("aagt://{}", collection)
        } else {
            format!("aagt://{}/{}", collection, path)
        }
    }

    /// Check if a string is a virtual path
    ///
    /// # Examples
    ///
    /// ```
    /// # use aagt_qmd::virtual_path::VirtualPath;
    /// assert!(VirtualPath::is_virtual("aagt://trading/sol.md"));
    /// assert!(VirtualPath::is_virtual("//trading/sol.md"));
    /// assert!(!VirtualPath::is_virtual("trading/sol.md"));
    /// assert!(!VirtualPath::is_virtual("/absolute/path.md"));
    /// ```
    pub fn is_virtual(path: &str) -> bool {
        let trimmed = path.trim();
        trimmed.starts_with("aagt:") || trimmed.starts_with("//")
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        Self::build(&self.collection, &self.path)
    }

    /// Get display path (collection/path)
    pub fn display_path(&self) -> String {
        if self.path.is_empty() {
            self.collection.clone()
        } else {
            format!("{}/{}", self.collection, self.path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard() {
        let vpath = VirtualPath::parse("aagt://trading/strategies/sol.md").unwrap();
        assert_eq!(vpath.collection, "trading");
        assert_eq!(vpath.path, "strategies/sol.md");
    }

    #[test]
    fn test_parse_missing_prefix() {
        let vpath = VirtualPath::parse("//trading/strategies/sol.md").unwrap();
        assert_eq!(vpath.collection, "trading");
        assert_eq!(vpath.path, "strategies/sol.md");
    }

    #[test]
    fn test_parse_extra_slashes() {
        let vpath = VirtualPath::parse("aagt:////trading/strategies/sol.md").unwrap();
        assert_eq!(vpath.collection, "trading");
        assert_eq!(vpath.path, "strategies/sol.md");
    }

    #[test]
    fn test_parse_collection_only() {
        let vpath = VirtualPath::parse("aagt://trading").unwrap();
        assert_eq!(vpath.collection, "trading");
        assert_eq!(vpath.path, "");
    }

    #[test]
    fn test_parse_invalid() {
        assert!(VirtualPath::parse("trading/sol.md").is_err());
        assert!(VirtualPath::parse("/absolute/path.md").is_err());
        assert!(VirtualPath::parse("aagt://").is_err());
    }

    #[test]
    fn test_parse_traversal_attack() {
        // Test key security fix
        assert!(VirtualPath::parse("aagt://../etc/passwd").is_err());
        assert!(VirtualPath::parse("aagt://collection/../secret.txt").is_err());
        assert!(VirtualPath::parse("aagt://collection/subdir/../secret.txt").is_err());
        // Normal paths should still work
        assert!(VirtualPath::parse("aagt://collection/file.md").is_ok());
    }

    #[test]
    fn test_build() {
        assert_eq!(
            VirtualPath::build("trading", "strategies/sol.md"),
            "aagt://trading/strategies/sol.md"
        );
        assert_eq!(VirtualPath::build("trading", ""), "aagt://trading");
    }

    #[test]
    fn test_is_virtual() {
        assert!(VirtualPath::is_virtual("aagt://trading/sol.md"));
        assert!(VirtualPath::is_virtual("//trading/sol.md"));
        assert!(!VirtualPath::is_virtual("trading/sol.md"));
        assert!(!VirtualPath::is_virtual("/absolute/path.md"));
    }

    #[test]
    fn test_display_path() {
        let vpath = VirtualPath {
            collection: "trading".to_string(),
            path: "strategies/sol.md".to_string(),
        };
        assert_eq!(vpath.display_path(), "trading/strategies/sol.md");

        let vpath_root = VirtualPath {
            collection: "trading".to_string(),
            path: "".to_string(),
        };
        assert_eq!(vpath_root.display_path(), "trading");
    }
}
