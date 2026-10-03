use serde_json::Value;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn generation_history(app: AppHandle, id: Option<String>) -> Result<Value, String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("generation-history");
    if let Some(id) = id {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("Invalid generation record".into());
        }
        return serde_json::from_slice(&std::fs::read(root.join(format!("{id}.json"))).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string());
    }
    if !root.exists() { return Ok(serde_json::json!([])); }
    let mut paths = std::fs::read_dir(root).map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path())).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    paths.sort();
    let mut entries = Vec::new();
    for path in paths.into_iter().rev().take(50) {
        let record: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        entries.push(serde_json::json!({"id":record["id"], "createdAt":record["createdAt"],
            "request":record["request"], "status":record["status"], "source":record["source"]}));
    }
    Ok(Value::Array(entries))
}
