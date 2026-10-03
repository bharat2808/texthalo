use serde_json::Value;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub fn generation_history(app: AppHandle, id: Option<String>) -> Result<Value, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("generation-history");
    if let Some(id) = id {
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err("Invalid generation record".into());
        }
        let mut record: Value = serde_json::from_slice(
            &std::fs::read(root.join(format!("{id}.json"))).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        merge_cloud_usage(&root, &mut record);
        return Ok(record);
    }
    if !root.exists() {
        return Ok(serde_json::json!([]));
    }
    let mut paths = std::fs::read_dir(&root)
        .map_err(|e| e.to_string())?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    paths.retain(|p| p.extension().is_some_and(|e| e == "json"));
    paths.sort();
    let mut entries = Vec::new();
    for path in paths.into_iter().rev().take(50) {
        let mut record: Value =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        merge_cloud_usage(&root, &mut record);
        entries.push(serde_json::json!({"id":record["id"], "createdAt":record["createdAt"],
            "request":record["request"], "status":record["status"], "source":record["source"], "provider":record["provider"], "usage":record["usage"]}));
    }
    Ok(Value::Array(entries))
}

// Billing metadata has its own atomic file so late settlement cannot race native audit writes.
pub(crate) fn save_cloud_usage(
    app: &AppHandle,
    audit_id: Option<&str>,
    request_id: &str,
    usage: &Value,
) {
    let Some(id) = Some(audit_id.unwrap_or(request_id))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    else {
        return;
    };
    let Ok(root) = app
        .path()
        .app_data_dir()
        .map(|p| p.join("generation-history"))
    else {
        return;
    };
    static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Ok(_guard) = WRITES.lock() else {
        return;
    };
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&root)?;
        if matches!(
            usage["billingStatus"].as_str(),
            None | Some("pending" | "reserved")
        ) {
            if let Ok(bytes) = std::fs::read(root.join(format!("{id}.usage"))) {
                if let Ok(previous) = serde_json::from_slice::<Value>(&bytes) {
                    if matches!(
                        previous["usage"]["billingStatus"].as_str(),
                        Some("settled" | "released")
                    ) {
                        return Ok(());
                    }
                }
            }
        }
        let temp = root.join(format!("{id}.usage.tmp"));
        std::fs::write(
            &temp,
            serde_json::json!({"provider":"cloud","requestId":request_id,"usage":usage})
                .to_string(),
        )?;
        std::fs::rename(temp, root.join(format!("{id}.usage")))
    })();
    if let Err(error) = result {
        eprintln!("Could not persist Assistant billing metadata: {error}");
    }
}
fn merge_cloud_usage(root: &std::path::Path, record: &mut Value) {
    let Some(id) = record["id"].as_str() else {
        return;
    };
    if !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return;
    }
    if let Ok(bytes) = std::fs::read(root.join(format!("{id}.usage"))) {
        if let Ok(metadata) = serde_json::from_slice::<Value>(&bytes) {
            record["provider"] = metadata["provider"].clone();
            record["usage"] = metadata["usage"].clone();
            record["requestId"] = metadata["requestId"].clone();
        }
    }
}
