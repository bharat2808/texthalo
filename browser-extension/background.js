const HOST = "com.texthalo.reader";
const supported = (url) => {
  try {
    const u = new URL(url);
    return (
      u.protocol === "https:" &&
      ["read.amazon.ca", "read.amazon.com"].includes(u.hostname) &&
      /^[A-Z0-9]{10}$/i.test(u.searchParams.get("asin") || "")
    );
  } catch {
    return false;
  }
};
function websiteSupported(url) {
  try {
    const u = new URL(url);
    return (
      u.protocol === "https:" &&
      ["read.amazon.ca", "read.amazon.com"].includes(u.hostname)
    );
  } catch {
    return false;
  }
}
async function activeTab(tabId) {
  const tab = Number.isInteger(tabId)
    ? await chrome.tabs.get(tabId)
    : (await chrome.tabs.query({ active: true, currentWindow: true }))[0];
  if (!tab?.id || !supported(tab.url))
    throw Error("Select the open Kindle book tab.");
  const [selected] = await chrome.tabs.query({
    active: true,
    windowId: tab.windowId,
  });
  if (selected?.id !== tab.id)
    throw Error("Keep the Kindle book selected in its browser window.");
  return tab;
}
async function inspect(tab) {
  const response = await chrome.tabs
    .sendMessage(tab.id, { protocolVersion: 1, type: "inspect" })
    .catch(() => {
      throw Error("Reload the Kindle tab once after loading the extension.");
    });
  if (!response?.ok)
    throw Error(response?.error || "Reader inspection failed.");
  return {
    ...response.data,
    tabId: tab.id,
    windowId: tab.windowId,
    zoom: await chrome.tabs.getZoom(tab.id),
    windowState: (await chrome.windows.get(tab.windowId)).state,
  };
}
async function run(message) {
  if (message.type === "reader-status") {
    const [tab] = await chrome.tabs.query({
      active: true,
      currentWindow: true,
    });
    if (!websiteSupported(tab?.url))
      return {
        supported: false,
        platform: null,
        title: "",
        progress: "",
        connected: false,
      };
    const state = await inspect(tab);
    return {
      ...state,
      supported: true,
      platform: "kindle",
      connected: relayState?.tabId === tab.id,
    };
  }
  if (message.type === "bridge-status")
    return chrome.runtime.sendNativeMessage(HOST, {
      protocolVersion: 1,
      requestId: crypto.randomUUID(),
      type: "hello",
    });
  if (message.type === "relay-disconnect") {
    await disconnectRelay();
    return { disconnected: true };
  }
  const tab = await activeTab(message.tabId);
  if (message.type === "relay-connect") {
    await connectRelay(tab);
    return { connected: true };
  }
  if (message.type === "inspect") return inspect(tab);
  if (["navigate", "ocr"].includes(message.type) &&
      (await chrome.windows.get(tab.windowId)).state === "minimized")
    throw Error("CHROME_MINIMIZED");
  if (message.type === "navigate") {
    const response = await chrome.tabs.sendMessage(tab.id, {
      protocolVersion: 1,
      type: message.type,
      direction: message.direction,
      bookId: message.bookId,
      fingerprint: message.fingerprint,
    });
    if (!response?.ok) throw Error(response?.error || "Navigation failed.");
    return { ...response.data, tabId: tab.id, windowId: tab.windowId };
  }
  if (message.type === "ocr") {
    const before = await inspect(tab);
    if (before.status !== "ready") throw Error(before.reason);
    if (
      before.bookId !== message.bookId ||
      before.fingerprint !== message.fingerprint
    )
      throw Error("Reader changed; inspect again.");
    if (before.viewport.visualScale !== 1)
      throw Error("Reset pinch zoom before capture.");
    let png;
    try {
      png = await chrome.tabs.captureVisibleTab(tab.windowId, {
        format: "png",
      });
    } catch {
      if ((await chrome.windows.get(tab.windowId)).state === "minimized")
        throw Error("CHROME_MINIMIZED");
      throw Error(
        "Open the TextHalo Reader extension on this tab and choose Enable page capture.",
      );
    }
    const current = await activeTab(tab.id);
    const after = await inspect(tab);
    if (after.windowState === "minimized" ||
        (await chrome.windows.get(tab.windowId)).state === "minimized")
      throw Error("CHROME_MINIMIZED");
    if (
      current.id !== tab.id ||
      after.fingerprint !== before.fingerprint ||
      JSON.stringify(after.region) !== JSON.stringify(before.region) ||
      after.viewport.width !== before.viewport.width ||
      after.viewport.height !== before.viewport.height
    )
      throw Error("Tab or page layout changed during capture. Retry.");
    const response = await chrome.runtime.sendNativeMessage(HOST, {
      protocolVersion: 1,
      requestId: crypto.randomUUID(),
      type: "ocr",
      url: before.url,
      bookId: before.bookId,
      region: before.region,
      viewport: before.viewport,
      png,
    });
    if (!response.ok) throw Error(response.error || "Local OCR failed.");
    return { ...response, reader: before };
  }
  throw Error("Unsupported request.");
}
chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (sender.id !== chrome.runtime.id || message?.protocolVersion !== 1) return;
  if (
    message.type === "reader-presence" &&
    sender.tab?.id &&
    supported(sender.url)
  ) {
    discover(sender.tab.id).then(
      () => respond({ ok: true }),
      (error) => respond({ ok: false, error: error.message }),
    );
    return true;
  }
  if (sender.url !== chrome.runtime.getURL("popup.html")) return;
  run(message).then(
    (data) => respond({ ok: true, data }),
    (error) => respond({ ok: false, error: error.message }),
  );
  return true;
});

let relayPort = null,
  relayState = null,
  relayPending = new Map();
function nativeRelay(type, payload) {
  if (!relayPort) return Promise.reject(Error("App relay is disconnected."));
  return new Promise((resolve, reject) => {
    const requestId = crypto.randomUUID();
    const timeout = setTimeout(() => {
      relayPending.delete(requestId);
      reject(Error("App relay timed out."));
    }, 12000);
    relayPending.set(requestId, { resolve, reject, timeout });
    relayPort.postMessage({
      protocolVersion: 1,
      requestId,
      type,
      payload: JSON.stringify(payload),
    });
  });
}
async function disconnectRelay() {
  if (relayState && relayPort)
    await nativeRelay("relay-disconnect", { relayId: relayState.id }).catch(
      () => {},
    );
  relayState = null;
  const port = relayPort;
  relayPort = null;
  port?.disconnect();
  for (const entry of relayPending.values()) {
    clearTimeout(entry.timeout);
    entry.reject(Error("App relay disconnected."));
  }
  relayPending.clear();
}
async function connectRelay(tab) {
  await disconnectRelay();
  const reader = await inspect(tab);
  if (reader.status !== "ready") throw Error(reader.reason);
  relayState = {
    id: crypto.randomUUID(),
    tabId: tab.id,
    bookId: reader.bookId,
  };
  relayPort = chrome.runtime.connectNative(HOST);
  const port = relayPort;
  relayPort.onMessage.addListener((message) => {
    const pending = relayPending.get(message.requestId);
    if (!pending) return;
    clearTimeout(pending.timeout);
    relayPending.delete(message.requestId);
    message.ok
      ? pending.resolve(message.data)
      : pending.reject(Error(message.error || "App relay failed."));
  });
  relayPort.onDisconnect.addListener(() => {
    if (relayPort !== port) return;
    const error =
      chrome.runtime.lastError?.message || "Native relay disconnected.";
    relayPort = null;
    relayState = null;
    for (const pending of relayPending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(Error(error));
    }
    relayPending.clear();
  });
  try {
    await nativeRelay("relay-register", { relayId: relayState.id, reader });
  } catch (error) {
    await disconnectRelay();
    throw error;
  }
  const binding = relayState;
  pumpRelay().catch(() => {
    if (relayState === binding) return disconnectRelay();
  });
}
async function pumpRelay() {
  const binding = relayState;
  while (relayState === binding && relayPort) {
    let response;
    try {
      const tab = await activeTab(binding.tabId);
      const reader = await inspect(tab);
      if (reader.bookId !== binding.bookId)
        throw Error("Connected book changed.");
      response = await nativeRelay("relay-poll", {
        relayId: binding.id,
        reader: reader.status === "ready" ? reader : null,
      });
    } catch (error) {
      if (relayState !== binding || !relayPort) break;
      await new Promise((resolve) => setTimeout(resolve, 1000));
      if (relayState !== binding || !relayPort) break;
      // The native helper rereads the private endpoint file after an app restart.
      try {
        const tab = await activeTab(binding.tabId);
        const reader = await inspect(tab);
        if (reader.bookId !== binding.bookId)
          throw Error("The connected book changed.");
        await nativeRelay("relay-register", { relayId: binding.id, reader });
      } catch {}
      continue;
    }
    const command = response.command;
    if (command) {
      let result;
      try {
        if (
          command.relayId !== binding.id ||
          command.expiresAt < Date.now() ||
          !["inspect", "ocr", "navigate"].includes(command.type)
        )
          throw Error("Expired or unsupported app command.");
        const tab = await activeTab(binding.tabId);
        const current = await inspect(tab);
        if (current.bookId !== binding.bookId)
          throw Error("The connected book changed.");
        result = {
          ok: true,
          data: await run({ ...command, tabId: binding.tabId }),
        };
      } catch (error) {
        result = { ok: false, error: error.message };
      }
      if (relayState === binding)
        await nativeRelay("relay-reply", {
          relayId: binding.id,
          id: command.id,
          result,
        });
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
}

let discovering = false;
async function discover(tabId) {
  if (discovering) return;
  discovering = true;
  try {
    const tab = await activeTab(tabId),
      reader = await inspect(tab);
    if (reader.status !== "ready") return;
    if (relayState?.tabId === tab.id && relayState.bookId === reader.bookId)
      return;
    await connectRelay(tab);
  } finally {
    discovering = false;
  }
}
chrome.tabs.onActivated?.addListener(({ tabId }) => {
  discover(tabId).catch(() => {
    if (relayState?.tabId !== tabId) disconnectRelay().catch(() => {});
  });
});
chrome.tabs.onRemoved?.addListener((tabId) => {
  if (relayState?.tabId === tabId) disconnectRelay().catch(() => {});
});
chrome.runtime.onStartup?.addListener(() => {
  discover().catch(() => {});
});
discover().catch(() => {});
