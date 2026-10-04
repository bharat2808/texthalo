//! Bounded local transcript archive. Audio and screen context are never stored here.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub created_at: u64,
    pub text: String,
    pub source: String,
}
pub fn list(path: &Path) -> Result<Vec<Entry>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| "Could not read transcription history".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("Could not read transcription history".into()),
    }
}
fn write(path: &Path, entries: &[Entry]) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid history path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(entries).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}
pub fn append(path: &Path, entry: Entry) -> Result<(), String> {
    let mut entries = list(path)?;
    entries.retain(|e| e.id != entry.id);
    entries.insert(0, entry);
    entries.truncate(50);
    write(path, &entries)
}
pub fn delete(path: &Path, id: Option<&str>) -> Result<(), String> {
    let mut entries = if id.is_some() {
        list(path)?
    } else {
        Vec::new()
    };
    entries.retain(|e| Some(e.id.as_str()) != id);
    write(path, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_newest_first_limits_and_deletes_only_requested_entry() {
        let root =
            std::env::temp_dir().join(format!("texthalo-history-test-{}", std::process::id()));
        let path = root.join("history.json");
        let _ = std::fs::remove_dir_all(&root);
        for n in 0..55 {
            append(
                &path,
                Entry {
                    id: n.to_string(),
                    created_at: n,
                    text: format!("Synthetic transcript {n}"),
                    source: "Test editor".into(),
                },
            )
            .unwrap();
        }
        let entries = list(&path).unwrap();
        assert_eq!(entries.len(), 50);
        assert_eq!(entries[0].text, "Synthetic transcript 54");
        delete(&path, Some("54")).unwrap();
        assert_eq!(list(&path).unwrap()[0].id, "53");
        delete(&path, Some("../../other")).unwrap();
        assert_eq!(list(&path).unwrap().len(), 49);
        delete(&path, None).unwrap();
        assert!(list(&path).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn corrupt_history_is_reported_without_overwriting_it() {
        let path = std::env::temp_dir().join(format!(
            "texthalo-corrupt-history-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, "broken").unwrap();
        assert!(append(
            &path,
            Entry {
                id: "1".into(),
                created_at: 1,
                text: "Synthetic".into(),
                source: String::new()
            }
        )
        .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
        std::fs::remove_file(path).unwrap();
    }
}
