//! User-authorized browser relay. The native host holds a private loopback token;
//! websites cannot enqueue commands or retrieve reading content.
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Arc, Condvar, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
#[derive(Default)]
struct Inner {
    relay: Option<String>,
    reader: Option<Value>,
    seen: Option<Instant>,
    queue: VecDeque<Value>,
    pending: HashMap<String, mpsc::Sender<Value>>,
    control_busy: bool,
    control_error: Option<String>,
}
#[derive(Clone)]
pub struct ReaderBridge {
    shared: Arc<(Mutex<Inner>, Condvar)>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub(crate) fn clear_connection_error(error: &mut Option<String>) {
    if matches!(
        error.as_deref(),
        Some(
            "Connect the book from the Chrome extension first."
                | "Connect the extension to TextHalo first."
        )
    ) {
        *error = None;
    }
}
fn valid_reader(reader: &Value) -> bool {
    reader["status"] == "ready"
        && reader["bookId"]
            .as_str()
            .is_some_and(|s| s.len() == 10 && s.chars().all(|c| c.is_ascii_alphanumeric()))
        && reader["url"]
            .as_str()
            .and_then(|s| url::Url::parse(s).ok())
            .is_some_and(|u| {
                u.scheme() == "https"
                    && matches!(u.host_str(), Some("read.amazon.ca" | "read.amazon.com"))
                    && u.query_pairs()
                        .any(|(k, v)| k == "asin" && Some(v.as_ref()) == reader["bookId"].as_str())
            })
}
impl ReaderBridge {
    pub fn disabled() -> Self {
        Self {
            shared: Arc::new((Mutex::new(Inner::default()), Condvar::new())),
        }
    }
    pub fn start(app: AppHandle) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = uuid::Uuid::new_v4().to_string();
        let dir = crate::engine_paths::app_support_dir().ok_or("Cannot locate app support")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join("reader-bridge.json");
        let temporary = dir.join("reader-bridge.tmp");
        use std::fs::OpenOptions;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&temporary).map_err(|e| e.to_string())?;
        output
            .write_all(json!({"port":port,"token":token}).to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &file).map_err(|e| e.to_string())?;
        let bridge = Self {
            shared: Arc::new((Mutex::new(Inner::default()), Condvar::new())),
        };
        let server = bridge.clone();
        std::thread::spawn(move || {
            let concurrent = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for connection in listener.incoming() {
                let Ok(mut stream) = connection else { continue };
                use std::sync::atomic::Ordering;
                if concurrent.fetch_add(1, Ordering::SeqCst) >= 16 {
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let server = server.clone();
                let token = token.clone();
                let app = app.clone();
                let concurrent = concurrent.clone();
                std::thread::spawn(move || {
                    let response = server.http(&mut stream, &token, Some(&app));
                    let (code, body) =
                        response.unwrap_or_else(|error| (400, json!({"error":error})));
                    let body = body.to_string();
                    let _=write!(stream,"HTTP/1.1 {code} Reply\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(bridge)
    }
    fn http(
        &self,
        stream: &mut TcpStream,
        token: &str,
        app: Option<&AppHandle>,
    ) -> Result<(u16, Value), String> {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        let boundary = loop {
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Incomplete request".into());
            }
            bytes.extend_from_slice(&buf[..n]);
            if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                break index + 4;
            }
            if bytes.len() > 8192 {
                return Err("Headers too large".into());
            }
        };
        let header = std::str::from_utf8(&bytes[..boundary]).map_err(|e| e.to_string())?;
        let mut lines = header.lines();
        let first = lines.next().ok_or("Missing request")?;
        let mut auth = false;
        let mut length = None;
        for line in lines {
            if let Some((key, value)) = line.split_once(':') {
                if key.eq_ignore_ascii_case("authorization") {
                    auth = value.trim() == format!("Bearer {token}")
                }
                if key.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().ok();
                }
            }
        }
        if !auth {
            return Ok((401, json!({"error":"Unauthorized"})));
        }
        let length = length
            .filter(|n| *n <= 900_000)
            .ok_or("Invalid body length")?;
        let route = first
            .split_whitespace()
            .nth(1)
            .ok_or("Missing route")?
            .to_string();
        if !first.starts_with("POST ") {
            return Err("POST required".into());
        }
        while bytes.len() - boundary < length {
            let mut buf = [0; 8192];
            let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("Truncated body".into());
            }
            bytes.extend_from_slice(&buf[..n]);
        }
        let body: Value = serde_json::from_slice(&bytes[boundary..boundary + length])
            .map_err(|e| e.to_string())?;
        if matches!(route.as_str(), "/controls" | "/control") {
            self.controls(app.ok_or("App controls unavailable")?, &route, body)
        } else {
            self.route(&route, body)
        }
    }
    fn controls(&self, app: &AppHandle, route: &str, body: Value) -> Result<(u16, Value), String> {
        {
            let inner = self.shared.0.lock().unwrap();
            if inner.relay.as_deref() != body["relayId"].as_str()
                || inner.relay.is_none()
                || !inner
                    .seen
                    .is_some_and(|s| s.elapsed() < Duration::from_secs(10))
            {
                return Err("Connect the extension to TextHalo first.".into());
            }
        }
        let state = crate::audiobooks::audiobook_state(app.clone());
        let context = &state["readerContext"];
        if context.is_null() || context["reader"]["bookId"] != body["bookId"] {
            return Err("The connected book changed. Reconnect the extension.".into());
        }
        if route == "/controls" {
            let mut inner = self.shared.0.lock().unwrap();
            // The checks above confirm this connection; an older failure no longer applies.
            clear_connection_error(&mut inner.control_error);
            let mut context = context.clone();
            // Only the connected book's controls are sent to Chrome; no saved passage text.
            if let Some(book) = context["book"].as_object_mut() {
                book.remove("passage");
            }
            return Ok((
                200,
                json!({"context":context,"playback":state["playback"],
                "generating":state["generating"],"busy":inner.control_busy,"error":inner.control_error,
                "readerWarning":state["readerWarning"]}),
            ));
        }
        let action = body["action"]
            .as_str()
            .ok_or("Missing reader action")?
            .to_owned();
        if !matches!(action.as_str(), "full" | "page") {
            return Err("Unsupported reader action".into());
        }
        let id = context["book"]["id"].as_str().map(str::to_owned);
        let fingerprint = body["fingerprint"].as_str().unwrap_or("").to_owned();
        if action == "page" && context["reader"]["fingerprint"] != fingerprint {
            return Err("The open page changed. Select it again.".into());
        }
        {
            let mut inner = self.shared.0.lock().unwrap();
            if inner.control_busy {
                return Err("A reader action is already running.".into());
            }
            inner.control_busy = true;
            inner.control_error = None;
        }
        let app = app.clone();
        let bridge = self.clone();
        // Return immediately so the native host remains available for browser capture commands.
        tauri::async_runtime::spawn(async move {
            let result = if action == "full" {
                crate::audiobooks::audiobook_generate(app, id, true)
                    .await
                    .map(|_| ())
            } else if let Some(id) = id {
                crate::audiobooks::audiobook_listen_page(app, id, fingerprint)
                    .await
                    .map(|_| ())
            } else {
                crate::audiobooks::audiobook_generate(app, None, false)
                    .await
                    .map(|_| ())
            };
            let mut inner = bridge.shared.0.lock().unwrap();
            inner.control_busy = false;
            inner.control_error = result.err();
        });
        Ok((200, json!({"accepted":true})))
    }
    fn route(&self, route: &str, body: Value) -> Result<(u16, Value), String> {
        let (lock, wake) = &*self.shared;
        let mut inner = lock.lock().unwrap();
        let relay = body["relayId"]
            .as_str()
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            .ok_or("Invalid relay identity")?;
        if route == "/register" {
            if !valid_reader(&body["reader"]) {
                return Err("Invalid reader".into());
            }
            if inner.relay.as_deref() != Some(relay) {
                inner.queue.clear();
                inner.pending.clear();
            }
            inner.relay = Some(relay.to_owned());
            inner.reader = Some(body["reader"].clone());
            inner.seen = Some(Instant::now());
            return Ok((200, json!({"registered":true})));
        }
        if inner.relay.as_deref() != Some(relay) {
            return Ok((409, json!({"error":"Reader relay changed"})));
        }
        inner.seen = Some(Instant::now());
        match route {
            "/poll" => {
                if body.get("reader").is_some() {
                    inner.reader = if valid_reader(&body["reader"]) {
                        Some(body["reader"].clone())
                    } else {
                        None
                    };
                }
                if inner.queue.is_empty() {
                    inner = wake.wait_timeout(inner, Duration::from_secs(1)).unwrap().0;
                }
                let command = loop {
                    match inner.queue.pop_front() {
                        Some(command) if command["expiresAt"].as_u64().unwrap_or(0) > now() => {
                            break Some(command)
                        }
                        Some(_) => continue,
                        None => break None,
                    }
                };
                Ok((200, json!({"command":command})))
            }
            "/reply" => {
                let id = body["id"].as_str().ok_or("Missing command ID")?;
                if let Some(sender) = inner.pending.remove(id) {
                    let result = body["result"].clone();
                    if result["ok"] == true {
                        let reader = if result["data"]["reader"].is_object() {
                            &result["data"]["reader"]
                        } else {
                            &result["data"]
                        };
                        if valid_reader(reader) {
                            inner.reader = Some(reader.clone());
                        }
                    }
                    let _ = sender.send(result);
                }
                Ok((200, json!({"accepted":true})))
            }
            "/disconnect" => {
                inner.relay = None;
                inner.reader = None;
                inner.queue.clear();
                inner.pending.clear();
                Ok((200, json!({"disconnected":true})))
            }
            _ => Err("Unknown relay route".into()),
        }
    }
    pub(crate) fn status(&self) -> Value {
        let inner = self.shared.0.lock().unwrap();
        json!({"connected":inner.relay.is_some()&&inner.seen.is_some_and(|seen|seen.elapsed()<Duration::from_secs(10)),"reader":inner.reader})
    }
    // Wait before enqueuing, so minimized time does not consume the relay timeout.
    pub(crate) fn command_while(
        &self,
        command: Value,
        active: impl Fn() -> bool,
    ) -> Result<Value, String> {
        loop {
            if !active() {
                return Err("Preparation stopped".into());
            }
            match self.command(command.clone()) {
                Err(error) if error == "CHROME_MINIMIZED" => {
                    let (lock, wake) = &*self.shared;
                    let inner = lock.lock().unwrap();
                    drop(
                        wake.wait_timeout(inner, Duration::from_millis(200))
                            .unwrap(),
                    );
                }
                result => return result,
            }
        }
    }
    pub(crate) fn command(&self, mut command: Value) -> Result<Value, String> {
        if !matches!(
            command["type"].as_str(),
            Some("inspect" | "ocr" | "navigate")
        ) {
            return Err("Unsupported browser command".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::channel();
        let (lock, wake) = &*self.shared;
        {
            let mut inner = lock.lock().unwrap();
            if inner.relay.is_none()
                || !inner
                    .seen
                    .is_some_and(|seen| seen.elapsed() < Duration::from_secs(10))
            {
                return Err("Connect the book from the Chrome extension first.".into());
            }
            if command["bookId"].is_string()
                && inner
                    .reader
                    .as_ref()
                    .is_some_and(|r| r["bookId"] != command["bookId"])
            {
                return Err("Connected book changed".into());
            }
            if matches!(command["type"].as_str(), Some("navigate" | "ocr"))
                && inner
                    .reader
                    .as_ref()
                    .is_some_and(|r| r["windowState"] == "minimized")
            {
                return Err("CHROME_MINIMIZED".into());
            }
            if inner.pending.len() >= 8 {
                return Err("Too many pending reader commands".into());
            }
            command["id"] = json!(id);
            command["relayId"] = json!(inner.relay);
            command["expiresAt"] = json!(now() + 15_000);
            inner.pending.insert(id.clone(), sender);
            inner.queue.push_back(command);
            wake.notify_one();
        }
        let result = receiver.recv_timeout(Duration::from_secs(15));
        let mut inner = lock.lock().unwrap();
        inner.pending.remove(&id);
        inner.queue.retain(|v| v["id"] != id);
        drop(inner);
        let result =
            result.map_err(|_| "Browser relay did not respond. Reconnect the book.".to_string())?;
        if result["ok"] != true {
            if result["error"] == "CHROME_MINIMIZED" {
                let mut inner = lock.lock().unwrap();
                if let Some(reader) = inner.reader.as_mut() {
                    reader["windowState"] = json!("minimized");
                }
            }
            return Err(result["error"]
                .as_str()
                .unwrap_or("Browser command failed")
                .into());
        }
        Ok(result["data"].clone())
    }
}
#[tauri::command]
pub fn reader_bridge_status(app: AppHandle) -> Value {
    app.state::<ReaderBridge>().status()
}
#[tauri::command]
pub async fn reader_bridge_command(app: AppHandle, command: Value) -> Result<Value, String> {
    let bridge = app.state::<ReaderBridge>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || bridge.command(command))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmed_connection_clears_only_connection_required_errors() {
        for message in [
            "Connect the book from the Chrome extension first.",
            "Connect the extension to TextHalo first.",
        ] {
            let mut error = Some(message.to_owned());
            clear_connection_error(&mut error);
            assert_eq!(error, None);
        }
        let mut error = Some("Local OCR failed.".to_owned());
        clear_connection_error(&mut error);
        assert_eq!(error.as_deref(), Some("Local OCR failed."));
    }
    #[test]
    fn reader_identity_is_domain_and_book_bound() {
        assert!(valid_reader(
            &json!({"status":"ready","url":"https://read.amazon.ca/?asin=B0GY3RKVRZ","bookId":"B0GY3RKVRZ"})
        ));
        assert!(!valid_reader(
            &json!({"status":"ready","url":"https://example.com/?asin=B0GY3RKVRZ","bookId":"B0GY3RKVRZ"})
        ));
        assert!(!valid_reader(
            &json!({"status":"ready","url":"https://read.amazon.ca/?asin=AAAAAAAAAA","bookId":"B0GY3RKVRZ"})
        ));
    }
}

#[cfg(test)]
mod relay_tests {
    use super::*;
    fn reader() -> Value {
        json!({"status":"ready","url":"https://read.amazon.ca/?asin=B0GY3RKVRZ","bookId":"B0GY3RKVRZ"})
    }
    #[test]
    fn minimized_capture_is_a_recoverable_wait_not_an_enqueued_timeout() {
        let bridge = ReaderBridge::disabled();
        let relay = uuid::Uuid::new_v4().to_string();
        let mut r = reader();
        r["windowState"] = json!("minimized");
        bridge
            .route("/register", json!({"relayId":relay,"reader":r}))
            .unwrap();
        let before = Instant::now();
        assert_eq!(
            bridge.command(json!({"type":"ocr"})).unwrap_err(),
            "CHROME_MINIMIZED"
        );
        assert!(before.elapsed() < Duration::from_secs(1));
        assert!(bridge.shared.0.lock().unwrap().queue.is_empty());
    }
    #[test]
    fn minimized_wait_can_be_cancelled_without_queuing_a_page_turn() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let bridge = ReaderBridge::disabled();
        let relay = uuid::Uuid::new_v4().to_string();
        let mut r = reader();
        r["windowState"] = json!("minimized");
        bridge
            .route("/register", json!({"relayId":relay,"reader":r}))
            .unwrap();
        let active = Arc::new(AtomicBool::new(true));
        let worker = bridge.clone();
        let flag = active.clone();
        let task = std::thread::spawn(move || {
            worker.command_while(json!({"type":"navigate"}), || flag.load(Ordering::SeqCst))
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(bridge.shared.0.lock().unwrap().queue.is_empty());
        active.store(false, Ordering::SeqCst);
        assert_eq!(task.join().unwrap().unwrap_err(), "Preparation stopped");
    }
    #[test]
    fn restoring_the_window_resumes_the_same_command() {
        let bridge = ReaderBridge::disabled();
        let relay = uuid::Uuid::new_v4().to_string();
        let mut r = reader();
        r["windowState"] = json!("minimized");
        bridge
            .route("/register", json!({"relayId":relay,"reader":r}))
            .unwrap();
        let worker = bridge.clone();
        let task = std::thread::spawn(move || {
            worker.command_while(
                json!({"type":"navigate","fingerprint":"saved-view"}),
                || true,
            )
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(bridge.shared.0.lock().unwrap().queue.is_empty());
        let mut r = reader();
        r["windowState"] = json!("normal");
        bridge
            .route("/register", json!({"relayId":relay,"reader":r.clone()}))
            .unwrap();
        let command = bridge.route("/poll", json!({"relayId":relay})).unwrap().1["command"].clone();
        assert_eq!(command["fingerprint"], "saved-view");
        bridge
            .route(
                "/reply",
                json!({"relayId":relay,"id":command["id"],"result":{"ok":true,"data":r}}),
            )
            .unwrap();
        assert_eq!(task.join().unwrap().unwrap()["windowState"], "normal");
    }
    #[test]
    fn commands_require_the_registered_relay_and_return_their_reply() {
        let bridge = ReaderBridge::disabled();
        let relay = uuid::Uuid::new_v4().to_string();
        bridge
            .route("/register", json!({"relayId":relay,"reader":reader()}))
            .unwrap();
        let other = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            bridge.route("/poll", json!({"relayId":other})).unwrap().0,
            409
        );
        let worker = bridge.clone();
        let task = std::thread::spawn(move || worker.command(json!({"type":"inspect"})));
        let command = bridge.route("/poll", json!({"relayId":relay})).unwrap().1["command"].clone();
        assert_eq!(command["type"], "inspect");
        bridge
            .route(
                "/reply",
                json!({"relayId":relay,"id":command["id"],"result":{"ok":true,"data":reader()}}),
            )
            .unwrap();
        assert_eq!(task.join().unwrap().unwrap()["bookId"], "B0GY3RKVRZ");
    }
    #[test]
    fn expired_commands_are_not_dispatched() {
        let bridge = ReaderBridge::disabled();
        let relay = uuid::Uuid::new_v4().to_string();
        bridge
            .route("/register", json!({"relayId":relay,"reader":reader()}))
            .unwrap();
        bridge
            .shared
            .0
            .lock()
            .unwrap()
            .queue
            .push_back(json!({"type":"navigate","expiresAt":0}));
        assert!(bridge.route("/poll", json!({"relayId":relay})).unwrap().1["command"].is_null());
    }
    #[test]
    fn missing_authentication_is_rejected_before_reading_content() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let task = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            ReaderBridge::disabled()
                .http(&mut stream, "test-token", None)
                .unwrap()
                .0
        });
        let mut client = TcpStream::connect(address).unwrap();
        client
            .write_all(b"POST /register HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}")
            .unwrap();
        assert_eq!(task.join().unwrap(), 401);
    }
}

#[tauri::command]
pub fn reader_extension_download(path: String) -> Result<(), String> {
    std::fs::write(
        path,
        include_bytes!(concat!(env!("OUT_DIR"), "/texthalo-reader.zip")),
    )
    .map_err(|e| e.to_string())
}
