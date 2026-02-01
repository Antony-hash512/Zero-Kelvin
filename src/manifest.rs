use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Directory,
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
    pub original_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub date: String,
    pub host: String,
    // Optional for backward compatibility with legacy archives
    pub privilege_mode: Option<PrivilegeMode>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub metadata: Metadata,
    pub files: Vec<FileEntry>,
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
}
