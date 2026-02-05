use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::{Result, Context, anyhow};
use std::fs;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrivilegeMode {
    User,
    Root,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: u32,

    #[serde(rename = "type")]
    pub entry_type: EntryType,

    // New format
    pub name: Option<String>,
    pub restore_path: Option<String>,

    // Legacy format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
}

impl FileEntry {
    pub fn from_path(id: u32, path: &Path, follow_links: bool) -> Result<Self> {
        // Use symlink_metadata to detect symlinks (don't follow them yet)
        let metadata = if follow_links {
            fs::metadata(path).context(format!("Failed to get metadata for {:?}", path))?
        } else {
            fs::symlink_metadata(path).context(format!("Failed to get metadata for {:?}", path))?
        };
        
        let entry_type = if metadata.is_symlink() {
            EntryType::Symlink
        } else if metadata.is_dir() {
            EntryType::Directory
        } else {
            EntryType::File
        };

        let name = path.file_name()
            .ok_or_else(|| anyhow!("Path {:?} terminates in ..", path))?
            .to_string_lossy()
            .into_owned();

        let restore_path = path.parent()
            .ok_or_else(|| anyhow!("Path {:?} has no parent", path))?
            .to_string_lossy()
            .into_owned();

        Ok(FileEntry {
            id,
            entry_type,
            name: Some(name),
            restore_path: Some(restore_path),
            original_path: None,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(name) = &self.name {
            if name.contains("..") || name.contains('/') {
                return Err(anyhow!("Invalid name contains '..' or '/': {}", name));
            }
        }

        if let Some(path) = &self.restore_path {
            if path.split('/').any(|part| part == "..") {
                return Err(anyhow!("Invalid restore_path contains '..': {}", path));
            }
        }

        if let Some(path) = &self.original_path {
            if path.split('/').any(|part| part == "..") {
                return Err(anyhow!("Invalid original_path contains '..': {}", path));
            }
        }
        
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub date: String,
    pub host: String,
    // Optional for backward compatibility with legacy archives
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privilege_mode: Option<PrivilegeMode>,
}

impl Metadata {
    pub fn new(host: String, privilege_mode: PrivilegeMode) -> Self {
        // Use system date command to match legacy behavior and avoid extra dependencies
        let date_str = std::process::Command::new("date")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown Date".to_string());

        Metadata {
            date: date_str,
            host,
            privilege_mode: Some(privilege_mode),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub metadata: Metadata,
    pub files: Vec<FileEntry>,
}

impl Manifest {
    pub fn new(metadata: Metadata, files: Vec<FileEntry>) -> Self {
        Manifest {
            metadata,
            files,
        }
    }

    pub fn validate(&self) -> Result<()> {
        for entry in &self.files {
            entry.validate().context(format!("Validation failed for file ID {}", entry.id))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_legacy_manifest() {
        let yaml = r#"
metadata:
  date: "Tue Jan 27 08:09:58 PM +04 2026"
  host: "katana"
files:
  - id: 1
    original_path: "/home/user/data"
    type: directory
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.metadata.host, "katana");
        assert_eq!(manifest.files[0].id, 1);
        
        // Ensure privilege_mode is None for legacy
        assert_eq!(manifest.metadata.privilege_mode, None);

        if let Some(path) = &manifest.files[0].original_path {
            assert_eq!(path, "/home/user/data");
        } else {
            panic!("Legacy path not found");
        }

        assert_eq!(manifest.files[0].entry_type, EntryType::Directory);
    }

    #[test]
    fn test_deserialize_new_manifest() {
        let yaml = r#"
metadata:
  date: "Tue Jan 27 08:09:58 PM +04 2026"
  host: "katana"
  privilege_mode: "user"
files:
  - id: 2
    name: "docs"
    restore_path: "/home/user/docs"
    type: file
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.metadata.privilege_mode, Some(PrivilegeMode::User));
        assert_eq!(manifest.files[0].id, 2);
        assert_eq!(manifest.files[0].name.as_ref().unwrap(), "docs");
        assert_eq!(manifest.files[0].entry_type, EntryType::File);
    }

    #[test]
    fn test_deserialize_root_privilege_mode() {
        let yaml = r#"
metadata:
    date: "Tue Jan 27 08:09:58 PM +04 2026"
    host: "katana"
    privilege_mode: "root"
files: []
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.metadata.privilege_mode, Some(PrivilegeMode::Root));
    }

    #[test]
    fn test_file_entry_from_file() {
        let temp = tempfile::tempdir().unwrap();
        let file_path = temp.path().join("my_file.txt");
        std::fs::File::create(&file_path).unwrap();

        let entry = FileEntry::from_path(1, &file_path, false).unwrap();
        assert_eq!(entry.id, 1);
        assert_eq!(entry.entry_type, EntryType::File);
        assert_eq!(entry.name.unwrap(), "my_file.txt");
        // restore_path should be absolute path of parent
        assert_eq!(entry.restore_path.unwrap(), temp.path().to_string_lossy());
    }

    #[test]
    fn test_file_entry_from_dir() {
        let temp = tempfile::tempdir().unwrap();
        let dir_path = temp.path().join("my_dir");
        std::fs::create_dir(&dir_path).unwrap();

        let entry = FileEntry::from_path(2, &dir_path, false).unwrap();
        assert_eq!(entry.id, 2);
        assert_eq!(entry.entry_type, EntryType::Directory);
        assert_eq!(entry.name.unwrap(), "my_dir");
        assert_eq!(entry.restore_path.unwrap(), temp.path().to_string_lossy());
    }

    #[test]
    fn test_file_entry_validation() {
        // Valid case
        let entry = FileEntry {
            id: 1,
            entry_type: EntryType::File,
            name: Some("valid.txt".to_string()),
            restore_path: Some("/home/user".to_string()),
            original_path: None,
        };
        assert!(entry.validate().is_ok());

        // Invalid: .. in name
        let bad_name = FileEntry {
            id: 2,
            entry_type: EntryType::File,
            name: Some("../bad.txt".to_string()),
            restore_path: Some("/home".to_string()),
            original_path: None,
        };
        assert!(bad_name.validate().is_err());

        // Invalid: .. in restore_path
        let bad_path = FileEntry {
            id: 3,
            entry_type: EntryType::File,
            name: Some("ok.txt".to_string()),
            restore_path: Some("/home/../etc".to_string()),
            original_path: None,
        };
        assert!(bad_path.validate().is_err());
    }

    #[test]
    fn test_manifest_validation() {
        let entry_ok = FileEntry {
            id: 1,
            entry_type: EntryType::File,
            name: Some("ok".to_string()),
            restore_path: Some("/ok".to_string()),
            original_path: None,
        };

        let manifest_ok = Manifest::new(
            Metadata::new("host".to_string(), PrivilegeMode::User),
            vec![entry_ok]
        );
        assert!(manifest_ok.validate().is_ok());

        let entry_bad = FileEntry {
            id: 2,
            entry_type: EntryType::File,
            name: Some("../bad".to_string()),
            restore_path: Some("/ok".to_string()),
            original_path: None,
        };

        let manifest_bad = Manifest::new(
             Metadata::new("host".to_string(), PrivilegeMode::User),
             vec![entry_bad]
        );
        assert!(manifest_bad.validate().is_err());
    }
}

