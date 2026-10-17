const $ = (id) => document.getElementById(id);
const setText = (id, text) => {
  const element = $(id);
  if (element.textContent !== text) element.textContent = text;
};
// Mutable release artifact: always the latest published Mac app.
const DOWNLOAD_URL = "https://storage.texthalo.app/TextHalo-macOS-aarch64.dmg";
async function request(type, payload = {}) {
  let timer;
  const response = await Promise.race([
    chrome.runtime.sendMessage({ protocolVersion: 1, type, ...payload }),
    new Promise((_, reject) => {
      timer = setTimeout(
        () =>
          reject(
            Error(
              "Reader relay timed out. Reload the extension and try again.",
            ),
          ),
        5000,
      );
    }),
  ]).finally(() => clearTimeout(timer));
  if (!response?.ok)
    throw Error(response?.error || "Cannot reach the reader relay.");
  return response.data;
}
let controls = null, pending = false, refreshing = false, actionError = "", connectionError = "";
// The app registers the native host on launch. Its hello probe also checks
// the app executable, because the helper can survive an uninstall.
// Recheck periodically so an uninstall or reinstall is reflected in an open popup.
let bridgeCheckedAt = 0, bridgeAbsent = false;
async function bridgeMissing() {
  if (Date.now() - bridgeCheckedAt < 5000) return bridgeAbsent;
  bridgeCheckedAt = Date.now();
  try {
    const status = await request("bridge-status");
    bridgeAbsent = status?.appInstalled === false;
  } catch (error) {
    bridgeAbsent = /not found|native host has exited|failed to start native messaging host/i.test(error.message);
  }
  return bridgeAbsent;
}
function showDownload(show) {
  $("download").hidden = !show;
  document.querySelector(".support").hidden = show;
  $("status").hidden = show;
  if (show) {
    $("book").hidden = true;
    $("controls").hidden = true;
    $("connect").hidden = true;
  }
}
function renderControls(data) {
  controls = data;
  const book = data.context.book;
  const generating = !!book && data.generating === book.id;
  const busy = pending || data.busy;
  $("page").disabled = busy;
  $("full").disabled = busy || !!data.generating || !!book?.complete;
  setText("preparation", data.readerWarning ? "Restore Chrome to prepare audio."
    : generating ? (book.fullPreparation ? "Building complete audiobook…" : "Preparing audio from your page…")
    : book?.complete ? "Complete audiobook ready. Listen in TextHalo."
    : "Listen from the open page, or prepare the whole book.");
  setText("control-error", actionError || data.error || book?.message || "");
}
async function refresh() {
  if (refreshing) return;
  refreshing = true;
  try {
    if (await bridgeMissing()) {
      showDownload(true);
      return;
    }
    showDownload(false);
    const state = await request("reader-status");
    setText("supported", state.supported ? "Yes" : "No");
    $("supported").className = state.supported ? "yes" : "no";
    $("book").hidden = !state.supported;
    setText("title", state.title || "Open a book in Kindle");
    setText("position", state.progress || "Page number unavailable");
    $("connect").hidden = !state.supported || state.status !== "ready" || state.connected;
    if (!state.connected || state.status !== "ready") $("controls").hidden = true;
    setText("status", connectionError || (!state.supported ? "This website is not supported yet."
      : state.status !== "ready" ? state.reason
      : state.connected ? "Connected to TextHalo." : "Connect to enable page capture and audiobook controls."));
    if (state.connected && state.status === "ready") {
      renderControls(await request("reader-controls"));
      $("controls").hidden = false;
    }
  } catch (error) {
    setText("supported", "Unavailable");
    $("book").hidden = true;
    $("controls").hidden = true;
    $("connect").hidden = true;
    setText("status", error.message === "Unsupported request."
      ? "Reload TextHalo Reader in chrome://extensions to finish updating." : error.message);
  } finally { refreshing = false; }
}
async function act(action) {
  if (pending || controls?.busy) return;
  pending = true;
  actionError = "";
  renderControls(controls);
  try {
    await request("reader-control", {action, fingerprint: controls.context.reader.fingerprint});
  } catch (error) { actionError = error.message; }
  finally { pending = false; await refresh(); }
}
$("download-app").onclick = () => {
  chrome.tabs.create({ url: DOWNLOAD_URL });
};
$("connect").onclick = async () => {
  $("connect").disabled = true;
  connectionError = "";
  try { await request("relay-connect"); await refresh(); }
  catch (error) { connectionError = error.message; setText("status", connectionError); }
  finally { $("connect").disabled = false; }
};
$("disconnect").onclick = async () => {
  connectionError = "";
  actionError = "";
  try { await request("relay-disconnect"); await refresh(); }
  catch (error) { setText("status", error.message); }
};
$("page").onclick = () => void act("page");
$("full").onclick = () => void act("full");
void refresh();
// Popup lifetime owns polling; Chrome destroys the timer when the popup closes.
if (typeof setInterval === "function") setInterval(() => void refresh(), 1000);
