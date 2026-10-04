//! Authenticated cloud generation. Only validated cumulative output crosses into native delivery.
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};

const MAX_EVENT: usize = 128 * 1024;
const MAX_TEXT: usize = 32 * 1024;
// Poll beyond the server lease (30s) and hard generation deadline (120s).
const RECOVERY_DELAYS: [u64; 7] = [0, 2, 5, 15, 15, 30, 60];
#[derive(Default)]
pub(crate) struct Decoder {
    sequence: Option<u64>,
    pub action: Option<String>,
    pub text: String,
    pub generated: bool,
    accepted: bool,
    dispatched: bool,
    failure_usage: Option<Value>,
    latest_usage: Option<Value>,
    reserved_credits: Value,
    terminal: bool,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
    None,
    Snapshot,
    Complete,
    Usage(Value),
    Done,
    Error(String),
}
impl Decoder {
    pub fn decode(&mut self, raw: &str, request: &str) -> Result<Event, String> {
        if raw.len() > MAX_EVENT {
            return Err("Assistant event exceeds the size limit".into());
        }
        let v: Value = serde_json::from_str(raw).map_err(|_| "Invalid Assistant event")?;
        if v["requestId"] != request || self.terminal {
            return Err("Unexpected Assistant request or terminal event".into());
        }
        let seq = v["sequence"].as_u64().ok_or("Missing Assistant sequence")?;
        if self.sequence.is_some_and(|s| seq != s + 1) {
            return Err("Assistant event sequence interrupted".into());
        }
        self.sequence = Some(seq);
        match v["type"].as_str() {
            Some("accepted") if !self.accepted => {
                self.accepted = true;
                self.reserved_credits = v["reservedCredits"].clone();
                let mut usage = merge_usage(None, &v, &self.reserved_credits);
                usage["billingStatus"] = json!("reserved");
                Ok(Event::Usage(usage))
            }
            Some("action") if self.accepted && self.action.is_none() && !self.generated => {
                let action = v["action"]
                    .as_str()
                    .filter(|a| matches!(*a, "text" | "speak" | "read-selection" | "clarify"))
                    .ok_or("Invalid Assistant action")?;
                self.action = Some(action.into());
                Ok(Event::None)
            }
            Some("text_delta") if self.action.is_some() && !self.generated => {
                let delta = v["text"].as_str().ok_or("Invalid Assistant text")?;
                if self.text.len() + delta.len() > MAX_TEXT
                    || self.action.as_deref() == Some("read-selection")
                {
                    return Err("Invalid Assistant output size or action".into());
                }
                self.text.push_str(delta);
                Ok(Event::Snapshot)
            }
            Some("generation_done") if self.action.is_some() && !self.generated => {
                if self.text.trim().is_empty() && self.action.as_deref() != Some("read-selection") {
                    return Err("Assistant returned empty output".into());
                }
                self.generated = true;
                Ok(Event::Complete)
            }
            Some("usage") if self.accepted => {
                let usage = merge_usage(self.latest_usage.as_ref(), &v, &self.reserved_credits);
                self.latest_usage = Some(usage.clone());
                Ok(Event::Usage(usage))
            }
            Some("done") if self.generated => {
                self.terminal = true;
                Ok(Event::Done)
            }
            Some("error") => {
                self.terminal = true;
                if matches!(v["billingStatus"].as_str(), Some("released" | "settled")) {
                    let prior_terminal = self
                        .latest_usage
                        .as_ref()
                        .filter(|usage| complete_terminal_usage(usage));
                    let mut usage = merge_usage(prior_terminal, &v, &self.reserved_credits);
                    // A rejection before admission created no hold. After admission,
                    // absent amounts must never be guessed, even on a terminal error.
                    if !self.accepted && v["billingStatus"] == "released" {
                        let values = usage.as_object_mut().unwrap();
                        values.entry("chargedCredits").or_insert(json!(0));
                        values.entry("refundedCredits").or_insert(json!(0));
                    }
                    if complete_terminal_usage(&usage) {
                        self.failure_usage = Some(usage);
                    }
                }
                Ok(Event::Error(server_error(&v)))
            }
            _ => Err("Unexpected Assistant event".into()),
        }
    }
}

pub(crate) fn resolve(
    provider: &str,
    consent: Option<u32>,
    local: bool,
    configured: bool,
    signed_in: bool,
) -> Result<&'static str, String> {
    if provider == "local" || (provider == "automatic" && local) {
        return Ok("local");
    }
    if !matches!(provider, "cloud" | "automatic") {
        return Err("Unknown Assistant provider".into());
    }
    if consent != Some(1) {
        return Err("Review and enable cloud Assistant in Settings first.".into());
    }
    if !configured {
        return Err("Cloud Assistant is not configured in this build.".into());
    }
    if !signed_in {
        return Err("Sign in to use cloud Assistant.".into());
    }
    Ok("cloud")
}

pub(crate) async fn run(
    app: AppHandle,
    id: u64,
    request_id: String,
    input: Value,
    mut cancel: watch::Receiver<bool>,
) {
    let audit_id = input["auditId"].as_str().map(str::to_owned);
    crate::generation_history::save_cloud_usage(
        &app,
        audit_id.as_deref(),
        &request_id,
        &json!({"billingStatus":"pending"}),
    );
    let mut decoder = Decoder::default();
    let socket_cancel = cancel.clone();
    let result = tokio::select! {
        _ = cancel.changed() => Err("Assistant canceled".to_string()),
        result = tokio::time::timeout(Duration::from_secs(120), stream(&app, id, &request_id, input, audit_id.as_deref(), &mut decoder, socket_cancel)) => result.unwrap_or_else(|_| Err("Cloud Assistant timed out".into())),
    };
    if result.is_err() {
        if !decoder.generated {
            crate::assistant::hosted_delivery(
                &app,
                id,
                &request_id,
                "hosted_error",
                decoder.action.as_deref().unwrap_or("text"),
                &decoder.text,
                result.as_ref().err().map(String::as_str),
            );
        }
        if let Some(usage) = definitive_failure_usage(&decoder) {
            crate::generation_history::save_cloud_usage(
                &app,
                audit_id.as_deref(),
                &request_id,
                &usage,
            );
            crate::assistant::hosted_usage(&app, id, &request_id, usage);
            return;
        }
        // Cancellation is idempotent and status reconciliation never replays output.
        let _ = crate::hosted::json_request(
            reqwest::Method::POST,
            &format!("/v1/assistant/requests/{request_id}/cancel"),
            None,
        )
        .await;
        reconcile(&app, id, &request_id, audit_id.as_deref()).await;
    }
}
async fn stream(
    app: &AppHandle,
    id: u64,
    request_id: &str,
    input: Value,
    audit_id: Option<&str>,
    decoder: &mut Decoder,
    cancel: watch::Receiver<bool>,
) -> Result<(), String> {
    let capabilities =
        crate::hosted::json_request(reqwest::Method::GET, "/v1/assistant/capabilities", None)
            .await?;
    if capabilities["enabled"] != true {
        return Err("Cloud Assistant is unavailable on this server.".into());
    }
    let token = crate::hosted::access_token(false).await?;
    let origin = crate::hosted::api_origin().ok_or("Cloud Assistant is not configured")?;
    let mut url = url::Url::parse(&origin).map_err(|_| "Invalid service URL")?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| "Invalid service URL")?;
    url.set_path("/v1/assistant/stream");
    let input = start_message(input, request_id, &token, &capabilities)?;
    socket_stream(
        url.as_str(),
        input,
        request_id,
        decoder,
        cancel,
        |event, state| match event {
            Event::Snapshot | Event::Complete => {
                let command = if state.generated {
                    "hosted_complete"
                } else {
                    "hosted_snapshot"
                };
                crate::assistant::hosted_delivery(
                    app,
                    id,
                    request_id,
                    command,
                    state.action.as_deref().unwrap(),
                    &state.text,
                    None,
                );
            }
            Event::Usage(usage) => {
                crate::generation_history::save_cloud_usage(app, audit_id, request_id, &usage);
                crate::assistant::hosted_usage(app, id, request_id, usage);
            }
            _ => (),
        },
    )
    .await
}
fn start_message(
    mut input: Value,
    request_id: &str,
    token: &str,
    capabilities: &Value,
) -> Result<Value, String> {
    input
        .as_object_mut()
        .ok_or("Invalid generation input")?
        .retain(|key, _| {
            matches!(
                key.as_str(),
                "request"
                    | "preferences"
                    | "selection"
                    | "source"
                    | "previousOutput"
                    | "screenText"
            )
        });
    input["type"] = json!("start");
    input["token"] = json!(token);
    input["requestId"] = json!(request_id);
    input["interactionId"] = json!(request_id);
    input["modelAlias"] = capabilities["modelAlias"].clone();
    input["rateVersion"] = capabilities["rateVersion"].clone();
    input["maxCredits"] = capabilities["maxRequestCredits"].clone();
    Ok(input)
}
async fn socket_stream(
    url: &str,
    input: Value,
    request_id: &str,
    decoder: &mut Decoder,
    mut cancel: watch::Receiver<bool>,
    mut deliver: impl FnMut(Event, &Decoder),
) -> Result<(), String> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_EVENT))
        .max_frame_size(Some(MAX_EVENT));
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(20),
        tokio_tungstenite::connect_async_with_config(url, Some(config), false),
    )
    .await
    .map_err(|_| "Cloud connection timed out")?
    .map_err(|_| "Could not connect to cloud Assistant")?;
    if *cancel.borrow() {
        return Err("Assistant canceled".into());
    }
    // Even a failed write can have reached admission; never infer a refund after this point.
    decoder.dispatched = true;
    socket
        .send(Message::Text(input.to_string().into()))
        .await
        .map_err(|_| "Cloud connection interrupted")?;
    loop {
        if *cancel.borrow() {
            return Err("Assistant canceled".into());
        }
        let message = tokio::select! {
            biased;
            _ = cancel.changed() => return Err("Assistant canceled".into()),
            message = tokio::time::timeout(Duration::from_secs(90), socket.next()) => message.map_err(|_| "Cloud Assistant timed out")?.ok_or("Cloud connection interrupted")?.map_err(|_| "Cloud connection interrupted")?,
        };
        let Message::Text(raw) = message else {
            if matches!(message, Message::Close(_)) {
                return Err("Cloud connection interrupted".into());
            }
            continue;
        };
        let event = decoder.decode(&raw, request_id)?;
        match event {
            Event::Done => return Ok(()),
            Event::Error(message) => return Err(message),
            event => deliver(event, decoder),
        }
    }
}
fn merge_usage(previous: Option<&Value>, incoming: &Value, reserved: &Value) -> Value {
    let mut usage = previous
        .cloned()
        .unwrap_or_else(|| json!({"reservedCredits":reserved}));
    for key in [
        "billingStatus",
        "reservedCredits",
        "chargedCredits",
        "refundedCredits",
        "availableCredits",
        "outputLimited",
        "maxOutputTokens",
    ] {
        if let Some(value) = incoming.get(key).filter(|value| !value.is_null()) {
            usage[key] = value.clone();
        }
    }
    usage
}
fn complete_terminal_usage(usage: &Value) -> bool {
    matches!(
        usage["billingStatus"].as_str(),
        Some("settled" | "released")
    ) && usage["chargedCredits"].is_number()
        && usage["refundedCredits"].is_number()
}

fn definitive_failure_usage(decoder: &Decoder) -> Option<Value> {
    decoder.failure_usage.clone().or_else(|| decoder.latest_usage.as_ref().filter(|usage| complete_terminal_usage(usage)).cloned()).or_else(|| {
        (!decoder.dispatched).then(|| json!({"billingStatus":"released","reservedCredits":0,"chargedCredits":0,"refundedCredits":0}))
    })
}

fn server_error(value: &Value) -> String {
    match value["error"].as_str().unwrap_or("") {
        "insufficient_credits" => {
            "Not enough cloud Assistant credits for this request. Shorten your request or open Billing to add credits."
        }
        "cloud_disabled" | "feature_disabled" => {
            "Cloud Assistant is unavailable on this server. Choose Local in Assistant settings."
        }
        "rate_changed" => {
            "Cloud Assistant pricing changed. Start a new voice request to use the current rate."
        }
        "credit_ceiling_exceeded" => "This request exceeds its credit limit. Shorten your request and try again.",
        "request_in_progress" => "This cloud Assistant request is already running. Its credit usage will be reconciled.",
        "request_already_completed" => "This cloud Assistant request has already finished. Start a new voice request to try again.",
        "provider_unavailable" => "Cloud Assistant is temporarily unavailable. Try again later.",
        _ => value["message"]
            .as_str()
            .filter(|message| !message.trim().is_empty())
            .unwrap_or("Cloud Assistant could not complete this request. Try again later."),
    }
    .chars()
    .take(600)
    .collect()
}

async fn reconcile(app: &AppHandle, id: u64, request: &str, audit: Option<&str>) {
    for delay in RECOVERY_DELAYS {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        let Ok(status) = crate::hosted::json_request(
            reqwest::Method::GET,
            &format!("/v1/assistant/requests/{request}"),
            None,
        )
        .await
        else {
            continue;
        };
        let usage = status.get("usage").cloned().unwrap_or(status);
        crate::generation_history::save_cloud_usage(app, audit, request, &usage);
        let pending = matches!(
            usage["billingStatus"].as_str(),
            None | Some("pending" | "reserved")
        );
        crate::assistant::hosted_usage(app, id, request, usage);
        if !pending {
            break;
        }
    }
}
pub(crate) async fn recover(app: AppHandle) {
    static RECOVERY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let Ok(_guard) = RECOVERY.try_lock() else {
        return;
    };
    use tauri::Manager;
    let Ok(root) = app
        .path()
        .app_data_dir()
        .map(|p| p.join("generation-history"))
    else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "usage") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if !matches!(
            value["usage"]["billingStatus"].as_str(),
            None | Some("pending" | "reserved")
        ) {
            continue;
        }
        let Some(request) = value["requestId"]
            .as_str()
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        else {
            continue;
        };
        reconcile(&app, 0, request, path.file_stem().and_then(|s| s.to_str())).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_error_preserves_prior_authoritative_settlement_amounts() {
        let mut decoder = Decoder {
            dispatched: true,
            ..Decoder::default()
        };
        event(&mut decoder, 1, "accepted", json!({"reservedCredits":1000})).unwrap();
        event(&mut decoder, 2, "usage", json!({"billingStatus":"settled","chargedCredits":1,"refundedCredits":999,"availableCredits":42})).unwrap();
        event(
            &mut decoder,
            3,
            "error",
            json!({"billingStatus":"settled","error":"canceled"}),
        )
        .unwrap();
        let usage = definitive_failure_usage(&decoder).unwrap();
        assert_eq!(usage["chargedCredits"], 1);
        assert_eq!(usage["refundedCredits"], 999);
        assert_eq!(usage["availableCredits"], 42);
    }
    #[test]
    fn incomplete_terminal_error_after_admission_requires_reconciliation() {
        let mut decoder = Decoder {
            dispatched: true,
            ..Decoder::default()
        };
        event(&mut decoder, 1, "accepted", json!({"reservedCredits":1000})).unwrap();
        event(
            &mut decoder,
            2,
            "error",
            json!({"billingStatus":"settled","error":"canceled"}),
        )
        .unwrap();
        assert!(definitive_failure_usage(&decoder).is_none());
    }
    #[test]
    fn provisional_zero_amounts_do_not_make_terminal_error_authoritative() {
        let mut decoder = Decoder {
            dispatched: true,
            ..Decoder::default()
        };
        event(&mut decoder, 1, "accepted", json!({"reservedCredits":1000})).unwrap();
        event(
            &mut decoder,
            2,
            "usage",
            json!({"billingStatus":"pending","chargedCredits":0,"refundedCredits":0}),
        )
        .unwrap();
        event(
            &mut decoder,
            3,
            "error",
            json!({"billingStatus":"settled","error":"canceled"}),
        )
        .unwrap();
        assert!(definitive_failure_usage(&decoder).is_none());
    }
    #[test]
    fn admission_rejection_preserves_released_usage_without_recovery() {
        let mut decoder = Decoder {
            dispatched: true,
            ..Decoder::default()
        };
        let error = event(&mut decoder, 1, "error", json!({"error":"insufficient_credits","billingStatus":"released","chargedCredits":0,"refundedCredits":0,"availableCredits":7})).unwrap();
        assert!(matches!(error, Event::Error(_)));
        let usage = definitive_failure_usage(&decoder).unwrap();
        assert_eq!(usage["billingStatus"], "released");
        assert_eq!(usage["chargedCredits"], 0);
        assert_eq!(usage["availableCredits"], 7);
    }
    #[test]
    fn only_pre_dispatch_failure_is_definitively_uncharged() {
        let mut decoder = Decoder::default();
        assert_eq!(
            definitive_failure_usage(&decoder).unwrap()["billingStatus"],
            "released"
        );
        decoder.dispatched = true;
        assert!(
            definitive_failure_usage(&decoder).is_none(),
            "A failed write may have reached the server"
        );
        event(&mut decoder, 1, "accepted", json!({"reservedCredits":1000})).unwrap();
        assert!(definitive_failure_usage(&decoder).is_none());
    }
    #[test]
    fn acceptance_surfaces_reserved_credit_hold() {
        let mut decoder = Decoder::default();
        assert_eq!(
            event(&mut decoder, 1, "accepted", json!({"reservedCredits":1000})).unwrap(),
            Event::Usage(json!({"billingStatus":"reserved","reservedCredits":1000}))
        );
    }
    #[test]
    fn recovery_schedule_outlives_server_deadline() {
        assert!(RECOVERY_DELAYS.iter().sum::<u64>() > 120);
        assert!(RECOVERY_DELAYS
            .iter()
            .scan(0, |sum, delay| {
                *sum += delay;
                Some(*sum)
            })
            .any(|elapsed| elapsed > 30 && elapsed < 60));
    }
    #[test]
    fn server_error_codes_are_actionable() {
        for (code, expected) in [
            ("insufficient_credits", "credits"),
            ("cloud_disabled", "unavailable"),
            ("rate_changed", "changed"),
            ("provider_unavailable", "temporarily"),
        ] {
            let mut decoder = Decoder::default();
            let Event::Error(message) =
                event(&mut decoder, 1, "error", json!({"error":code})).unwrap()
            else {
                panic!("Expected error");
            };
            assert!(
                message.contains(expected) && !message.contains('_'),
                "{message}"
            );
        }
    }
    #[tokio::test]
    async fn mock_socket_checks_start_and_cumulative_native_delivery() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "ws://{}/v1/assistant/stream",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let start: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(start["type"], "start");
            assert_eq!(start["token"], "mock-token");
            assert_eq!(start["requestId"], "r");
            assert_eq!(start["interactionId"], "r");
            assert_eq!(start["maxCredits"], 4);
            assert_eq!(start["rateVersion"], "v1");
            assert_eq!(start["modelAlias"], "assistant-standard");
            assert_eq!(start["request"], "Hello");
            assert!(start.get("auditId").is_none());
            assert!(start.get("id").is_none());
            for value in [
                json!({"type":"accepted"}),
                json!({"type":"action","action":"text"}),
                json!({"type":"text_delta","text":"hé"}),
                json!({"type":"text_delta","text":"🙂"}),
                json!({"type":"generation_done"}),
                json!({"type":"usage","billingStatus":"settled","chargedCredits":1}),
                json!({"type":"done"}),
            ]
            .into_iter()
            .enumerate()
            {
                let (seq, mut value) = value;
                value["requestId"] = json!("r");
                value["sequence"] = json!(seq + 1);
                socket
                    .send(Message::Text(value.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let input = start_message(json!({"request":"Hello","preferences":"","selection":"","source":{},"previousOutput":"","screenText":"","auditId":"private","id":23}), "r", "mock-token", &json!({"modelAlias":"assistant-standard","rateVersion":"v1","maxRequestCredits":4})).unwrap();
        let (_tx, cancel) = watch::channel(false);
        let mut decoder = Decoder::default();
        let mut snapshots = Vec::new();
        socket_stream(&url, input, "r", &mut decoder, cancel, |event, state| {
            if matches!(event, Event::Snapshot | Event::Complete) {
                snapshots.push((state.text.clone(), state.generated));
            }
        })
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(
            snapshots,
            vec![
                ("hé".into(), false),
                ("hé🙂".into(), false),
                ("hé🙂".into(), true)
            ]
        );
    }
    #[tokio::test]
    async fn mock_socket_cancel_discards_queued_completion() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            socket.next().await.unwrap().unwrap();
            for (seq, mut value) in [
                json!({"type":"accepted"}),
                json!({"type":"action","action":"text"}),
                json!({"type":"text_delta","text":"partial"}),
                json!({"type":"generation_done"}),
            ]
            .into_iter()
            .enumerate()
            {
                value["requestId"] = json!("r");
                value["sequence"] = json!(seq + 1);
                let _ = socket.send(Message::Text(value.to_string().into())).await;
            }
        });
        let (tx, cancel) = watch::channel(false);
        let mut decoder = Decoder::default();
        let mut completions = 0;
        let result = socket_stream(
            &url,
            json!({"type":"start"}),
            "r",
            &mut decoder,
            cancel,
            |event, _| {
                if event == Event::Snapshot {
                    tx.send_replace(true);
                }
                if event == Event::Complete {
                    completions += 1;
                }
            },
        )
        .await;
        server.await.unwrap();
        assert!(result.is_err());
        assert_eq!(completions, 0);
        assert!(!decoder.generated);
    }
    fn event(d: &mut Decoder, seq: u64, kind: &str, extra: Value) -> Result<Event, String> {
        let mut v = json!({"requestId":"r","sequence":seq,"type":kind});
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        d.decode(&v.to_string(), "r")
    }
    #[test]
    fn cumulative_unicode_and_completion() {
        let mut d = Decoder::default();
        event(&mut d, 1, "accepted", json!({})).unwrap();
        event(&mut d, 2, "action", json!({"action":"text"})).unwrap();
        event(&mut d, 3, "text_delta", json!({"text":"hé"})).unwrap();
        event(&mut d, 4, "text_delta", json!({"text":"🙂"})).unwrap();
        assert_eq!(d.text, "hé🙂");
        assert_eq!(
            event(&mut d, 5, "generation_done", json!({})).unwrap(),
            Event::Complete
        );
        assert!(event(&mut d, 6, "text_delta", json!({"text":"late"})).is_err());
    }
    #[test]
    fn rejects_changed_action_missing_sequence_wrong_request_and_bounds() {
        let mut d = Decoder::default();
        event(&mut d, 1, "accepted", json!({})).unwrap();
        event(&mut d, 2, "action", json!({"action":"speak"})).unwrap();
        assert!(event(&mut d, 3, "action", json!({"action":"text"})).is_err());
        assert!(d.decode("{}", "r").is_err());
        assert!(d.decode(&"x".repeat(MAX_EVENT + 1), "r").is_err());
        let mut d = Decoder::default();
        event(&mut d, 1, "accepted", json!({})).unwrap();
        assert!(event(&mut d, 3, "action", json!({"action":"text"})).is_err());
    }
    #[test]
    fn read_selection_needs_no_body() {
        let mut d = Decoder::default();
        event(&mut d, 0, "accepted", json!({})).unwrap();
        event(&mut d, 1, "action", json!({"action":"read-selection"})).unwrap();
        assert_eq!(
            event(&mut d, 2, "generation_done", json!({})).unwrap(),
            Event::Complete
        );
    }
    #[test]
    fn bounded_output_and_terminal_stream_reject_late_text() {
        let mut d = Decoder::default();
        event(&mut d, 1, "accepted", json!({})).unwrap();
        event(&mut d, 2, "action", json!({"action":"text"})).unwrap();
        assert!(event(
            &mut d,
            3,
            "text_delta",
            json!({"text":"x".repeat(MAX_TEXT + 1)})
        )
        .is_err());
        let mut d = Decoder::default();
        event(&mut d, 1, "accepted", json!({})).unwrap();
        event(&mut d, 2, "action", json!({"action":"read-selection"})).unwrap();
        event(&mut d, 3, "generation_done", json!({})).unwrap();
        event(&mut d, 4, "done", json!({})).unwrap();
        assert!(event(&mut d, 5, "text_delta", json!({"text":"late"})).is_err());
    }
    #[test]
    fn routing_requires_consent_and_auth_only_for_cloud() {
        assert_eq!(
            resolve("local", None, false, false, false).unwrap(),
            "local"
        );
        assert_eq!(
            resolve("automatic", None, true, false, false).unwrap(),
            "local"
        );
        assert!(resolve("automatic", None, false, true, true).is_err());
        assert!(resolve("cloud", Some(1), true, true, false).is_err());
        assert_eq!(
            resolve("automatic", Some(1), false, true, true).unwrap(),
            "cloud"
        );
    }
}
