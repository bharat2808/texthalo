const $ = (id) => document.getElementById(id);
async function request(type) {
  let timer;
  const response = await Promise.race([
    chrome.runtime.sendMessage({ protocolVersion: 1, type }),
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
async function refresh() {
  try {
    const state = await request("reader-status");
    $("supported").textContent = String(state.supported);
    $("supported").className = state.supported ? "yes" : "no";
    $("book").hidden = !state.supported;
    $("title").textContent = state.title || "Open a book in Kindle";
    $("position").textContent = state.progress || "Page number unavailable";
    $("connect").hidden = !state.supported || state.status !== "ready";
    $("status").textContent = !state.supported
      ? "This website is not supported yet."
      : state.status !== "ready"
        ? state.reason
        : state.connected
          ? "Connected to TextHalo. Continue in the app."
          : "Book detected. Connecting to TextHalo…";
  } catch (error) {
    $("supported").textContent = "Unavailable";
    $("book").hidden = true;
    $("status").textContent =
      error.message === "Unsupported request."
        ? "Reload TextHalo Reader in chrome://extensions to finish updating."
        : error.message;
  }
}
$("connect").onclick = async () => {
  const button = $("connect");
  button.disabled = true;
  try {
    await request("relay-connect");
    $("status").textContent = "Page capture enabled. Continue in TextHalo.";
  } catch (error) {
    $("status").textContent = error.message;
  } finally {
    button.disabled = false;
  }
};
void refresh();
