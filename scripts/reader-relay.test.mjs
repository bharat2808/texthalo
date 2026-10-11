import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import { readFileSync } from "node:fs";
import { webcrypto } from "node:crypto";
function harness() {
  let listener;
  const context = {
    URL,
    crypto: webcrypto,
    Date,
    setTimeout: (fn) => {
      queueMicrotask(fn);
      return 1;
    },
    clearTimeout: () => {},
    chrome: {
      tabs: { query: async () => [] },
      runtime: {
        id: "test",
        getURL: (p) => `chrome-extension://test/${p}`,
        onMessage: {
          addListener: (fn) => {
            listener = fn;
          },
        },
      },
    },
  };
  vm.runInNewContext(
    readFileSync(
      new URL("../browser-extension/background.js", import.meta.url),
      "utf8",
    ) +
      `\nglobalThis.api={run,pumpRelay,discover,setBinding(binding){relayState=binding;relayPort={};},stop(){relayState=null;},native(fn){nativeRelay=fn;},reader(fn){activeTab=fn;inspect=async()=>({status:'ready',bookId:'B0GY3RKVRZ'});}};`,
    context,
  );
  return { api: context.api, listener, chrome: context.chrome };
}
test("webpage and foreign-extension messages cannot invoke the app relay", () => {
  const { listener } = harness();
  let answered = false;
  for (const sender of [
    { id: "foreign", url: "chrome-extension://test/popup.html" },
    { id: "test", url: "https://read.amazon.ca/" },
    { id: "test", url: "chrome-extension://test/other.html" },
  ]) {
    assert.equal(
      listener({ protocolVersion: 1, type: "relay-connect" }, sender, () => {
        answered = true;
      }),
      undefined,
    );
  }
  assert.equal(answered, false);
  assert.equal(
    listener(
      { protocolVersion: 1, type: "unsupported" },
      { id: "test", url: "chrome-extension://test/popup.html" },
      () => {},
    ),
    true,
  );
});
test("an app restart retries registration for the same connected book", async () => {
  const { api } = harness();
  api.setBinding({ id: "session", tabId: 1, bookId: "B0GY3RKVRZ" });
  api.reader(async () => ({ id: 1 }));
  let polls = 0,
    registrations = 0;
  api.native(async (type, payload) => {
    if (type === "relay-poll") {
      if (!polls++) throw Error("App restarted");
      api.stop();
      return {};
    }
    if (type === "relay-register") {
      registrations++;
      assert.equal(payload.reader.bookId, "B0GY3RKVRZ");
      return {};
    }
  });
  await api.pumpRelay();
  assert.equal(registrations, 1);
});
test("expired commands are refused after presence inspection, without navigation", async () => {
  const { api } = harness();
  api.setBinding({ id: "session", tabId: 1, bookId: "B0GY3RKVRZ" });
  let browserReads = 0,
    reply,
    polls = 0;
  api.reader(async () => {
    browserReads++;
    return { id: 1 };
  });
  api.native(async (type, payload) => {
    if (type === "relay-poll") {
      if (!polls++)
        return {
          command: {
            id: "old",
            relayId: "session",
            expiresAt: 0,
            type: "navigate",
          },
        };
      api.stop();
      return {};
    }
    if (type === "relay-reply") {
      reply = payload.result;
      return {};
    }
  });
  await api.pumpRelay();
  assert.equal(browserReads, 2);
  assert.equal(reply.ok, false);
  assert.match(reply.error, /Expired/);
});

test("popup ends checking when an outdated worker refuses its request", async () => {
  const elements = new Map();
  const element = (id) => {
    if (!elements.has(id))
      elements.set(id, { textContent: "Checking…", hidden: false });
    return elements.get(id);
  };
  const context = {
    document: { getElementById: element, querySelector: element },
    chrome: {
      runtime: {
        sendMessage: async () => ({ ok: false, error: "Unsupported request." }),
      },
    },
    setTimeout,
    clearTimeout,
  };
  vm.runInNewContext(
    readFileSync(
      new URL("../browser-extension/popup.js", import.meta.url),
      "utf8",
    ),
    context,
  );
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(element("supported").textContent, "Unavailable");
  assert.match(element("status").textContent, /Reload TextHalo Reader/);
});

test("popup times out a worker that never responds", async () => {
  const elements = new Map();
  const element = (id) => {
    if (!elements.has(id))
      elements.set(id, { textContent: "Checking…", hidden: false });
    return elements.get(id);
  };
  let timeout;
  const context = {
    document: { getElementById: element, querySelector: element },
    chrome: { runtime: { sendMessage: () => new Promise(() => {}) } },
    setTimeout: (fn) => {
      timeout = fn;
      return 1;
    },
    clearTimeout: () => {},
  };
  vm.runInNewContext(
    readFileSync(
      new URL("../browser-extension/popup.js", import.meta.url),
      "utf8",
    ),
    context,
  );
  timeout(); // Native-host probe expires first.
  await new Promise((resolve) => setImmediate(resolve));
  timeout(); // The reader-status request also receives no response.
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(element("supported").textContent, "Unavailable");
  assert.match(element("status").textContent, /timed out/);
});

function minimizedHarness(state) {
  const h = harness();
  const tab = {id:1,windowId:2,url:"https://read.amazon.ca/?asin=B0GY3RKVRZ"};
  h.chrome.windows = {get:async()=>({state})};
  h.chrome.tabs.get=async()=>tab;
  h.chrome.tabs.query=async()=>[tab];
  let turns=0,captures=0;
  h.chrome.tabs.sendMessage=async(_id,message)=>{
    if(message.type==="navigate") turns++;
    return {ok:true,data:{status:"ready",bookId:"B0GY3RKVRZ",fingerprint:"view",viewport:{visualScale:1}}};
  };
  h.chrome.tabs.getZoom=async()=>1;
  h.chrome.tabs.captureVisibleTab=async()=>{captures++;return "png";};
  return {...h,counts:()=>({turns,captures})};
}
test("a minimized book remains connected and reports its window state",async()=>{
  const {api}=minimizedHarness("minimized");
  const reader=await api.run({type:"inspect",tabId:1});
  assert.equal(reader.status,"ready");
  assert.equal(reader.windowState,"minimized");
});
test("minimized windows refuse navigation and capture before changing the reader",async()=>{
  const {api,counts}=minimizedHarness("minimized");
  for(const type of ["navigate","ocr"]){
    await assert.rejects(api.run({type,tabId:1,bookId:"B0GY3RKVRZ",fingerprint:"view",direction:"next"}),/CHROME_MINIMIZED/);
  }
  assert.deepEqual(counts(),{turns:0,captures:0});
});
test("an unfocused normal window still allows pagination",async()=>{
  const {api,counts}=minimizedHarness("normal");
  await api.run({type:"navigate",tabId:1,bookId:"B0GY3RKVRZ",fingerprint:"view",direction:"next"});
  assert.equal(counts().turns,1);
});
test("minimizing during a rejected capture waits instead of reporting missing permission",async()=>{
  const {api,chrome}=minimizedHarness("normal");
  chrome.tabs.captureVisibleTab=async()=>{
    chrome.windows.get=async()=>({state:"minimized"});
    throw Error("Capture unavailable");
  };
  await assert.rejects(api.run({type:"ocr",tabId:1,bookId:"B0GY3RKVRZ",fingerprint:"view"}),/CHROME_MINIMIZED/);
});
test("minimizing during a successful capture discards that capture and waits",async()=>{
  const {api,chrome}=minimizedHarness("normal");
  chrome.tabs.captureVisibleTab=async()=>{
    chrome.windows.get=async()=>({state:"minimized"});
    return "png";
  };
  await assert.rejects(api.run({type:"ocr",tabId:1,bookId:"B0GY3RKVRZ",fingerprint:"view"}),/CHROME_MINIMIZED/);
});

test("the app relay pump rejects removed sidebar commands", async () => {
  for (const type of ["chapters", "chapter-navigate"]) {
    const {api, chrome} = harness();
    api.setBinding({id:"session",tabId:1,bookId:"B0GY3RKVRZ"});
    api.reader(async()=>({id:1,windowId:2}));
    chrome.windows={get:async()=>({state:"normal"})};
    let forwarded, reply;
    chrome.tabs.sendMessage=async(_id,message)=>{
      forwarded=message;
      return {ok:true,data:{chapters:[{id:"one",label:"Chapter One"}]}};
    };
    api.native(async(method,payload)=>{
      if(method==="relay-poll") return {command:{id:"request",relayId:"session",expiresAt:Date.now()+15000,type,chapterId:"one",bookId:"B0GY3RKVRZ",fingerprint:"view"}};
      if(method==="relay-reply") {reply=payload.result;api.stop();return {};}
    });
    await api.pumpRelay();
    assert.equal(reply.ok,false,`${type} must be rejected`);
    assert.match(reply.error,/Expired or unsupported app command/);
    assert.equal(forwarded,undefined);
  }
});


test("detecting a ready Kindle book never opens a native connection automatically", async () => {
  const {api, chrome} = minimizedHarness("normal");
  let connections = 0;
  chrome.runtime.connectNative = () => { connections++; throw Error("Unexpected automatic connection"); };
  await api.discover(1);
  assert.equal(connections, 0);
});

test("reader controls require the selected book's relay and use a separate native request", async () => {
  const {api, chrome} = minimizedHarness("normal");
  await assert.rejects(api.run({type:"reader-control",action:"generate"}), /Connect this book/);
  api.setBinding({id:"session",tabId:2,bookId:"B0GY3RKVRZ"});
  await assert.rejects(api.run({type:"reader-control",action:"generate"}), /Connect this book/);
  api.setBinding({id:"session",tabId:1,bookId:"B0GY3RKVRZ"});
  let sent;
  chrome.runtime.sendNativeMessage = async (host, message) => {
    sent = {host,message};
    return {ok:true,data:{accepted:true}};
  };
  const result = await api.run({type:"reader-control",action:"page",fingerprint:"view"});
  assert.equal(result.accepted, true);
  assert.equal(sent.host, "com.texthalo.reader");
  assert.equal(sent.message.type, "relay-control");
  const payload = JSON.parse(sent.message.payload);
  assert.equal(payload.bookId, "B0GY3RKVRZ");
  assert.equal(payload.relayId, "session");
  assert.equal(payload.fingerprint, "view");
});

test("reader controls display native app failures", async () => {
  const {api, chrome} = minimizedHarness("normal");
  api.setBinding({id:"session",tabId:1,bookId:"B0GY3RKVRZ"});
  chrome.runtime.sendNativeMessage = async () => ({ok:false,error:"Open TextHalo"});
  await assert.rejects(api.run({type:"reader-controls"}), /Open TextHalo/);
});


test("the connected popup offers both simple actions before any book or playback exists", async () => {
  const elements = new Map();
  const element = id => {
    if (!elements.has(id)) elements.set(id, {textContent:"",hidden:false,replaceChildren(){}});
    return elements.get(id);
  };
  const sent=[];
  const context={document:{getElementById:element,querySelector:element},setTimeout,clearTimeout,
    chrome:{runtime:{sendMessage:async message=>{
      sent.push(message);
      if(message.type==="reader-status") return {ok:true,data:{supported:true,status:"ready",connected:true}};
      if(message.type==="reader-controls") return {ok:true,data:{context:{book:null,reader:{fingerprint:"view"}},playback:null,generating:null,busy:false}};
      return {ok:true,data:{accepted:true}};
    }}}};
  vm.runInNewContext(readFileSync(new URL("../browser-extension/popup.js",import.meta.url),"utf8"),context);
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(element("controls").hidden,false);
  assert.equal(element("page").disabled,false);
  assert.equal(element("full").disabled,false);
  element("page").onclick();
  await new Promise(resolve=>setImmediate(resolve));
  assert.ok(sent.some(message=>message.type==="reader-control"&&message.action==="page"));
});

test("connected polling preserves visible controls and disables duplicate full builds", async () => {
  const elements = new Map(), hiddenWrites = [];
  const element = id => {
    if (!elements.has(id)) {
      let hidden = true;
      elements.set(id, {textContent:"",value:"",replaceChildren(){},
        get hidden(){return hidden;},set hidden(value){hidden=value;if(id==="controls")hiddenWrites.push(value);}});
    }
    return elements.get(id);
  };
  let generating = null;
  const document = {getElementById:element,querySelector:element,activeElement:null};
  const context={document,setTimeout,clearTimeout,chrome:{runtime:{sendMessage:async message=>
    message.type==="reader-status" ? {ok:true,data:{supported:true,status:"ready",connected:true}} :
    {ok:true,data:{context:{book:{id:"book",checkpoints:[],generated:1},reader:{fingerprint:"view"}},
      playback:{bookId:"book",audio:{paused:true,speed:1,position:0,buffered:60}},busy:false,generating}}
  }}};
  vm.runInNewContext(readFileSync(new URL("../browser-extension/popup.js",import.meta.url),"utf8")+
    "\nglobalThis.refreshPopup=refresh;",context);
  await new Promise(resolve=>setImmediate(resolve));

  hiddenWrites.length=0;
  await context.refreshPopup();
  assert.equal(element("page").disabled,false);
  assert.equal(element("full").disabled,false);
  assert.equal(element("controls").hidden,false);
  assert.ok(!hiddenWrites.includes(true));
  generating = "book";
  await context.refreshPopup();
  assert.equal(element("full").disabled,true);
  assert.equal(element("page").disabled,false);
});
