import { expect, test, type Page } from "@playwright/test";
test("cloud Assistant preserves the voice flow and saves explicit cloud consent", async ({ page }) => {
  await boot(page, false, 3, false, "apple", { model: "unavailable", cloudConfigured: true, cloudSignedIn: true, cloudScreenInput: true, screen: true });
  await page.getByRole("button", { name: "Assistant", exact: true }).click();
  await expect(page.getByRole("group", { name: "Assistant model" }).getByRole("button", { name: /^On this Mac/ })).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("group", { name: "Assistant model" }).getByRole("button", { name: /^TextHalo Cloud/ }).click();
  await expect(page.getByLabel("Enable cloud processing for Assistant")).not.toBeChecked();
  await page.getByLabel("Enable cloud processing for Assistant").check();
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.assistant.cloud_consent_version)).toBe(1);
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.assistant.provider)).toBe("cloud");
  await expect(page.getByText("Hold your shortcut and ask to write, edit, or read aloud.")).toBeVisible();
  await expect(page.getByText("On-device Assistant needs Apple Intelligence.", { exact: false })).toHaveCount(0);
  await page.getByRole("group", { name: "Assistant model" }).getByRole("button", { name: /^On this Mac/ }).press("Enter");
  await expect(page.getByText("On-device Assistant needs Apple Intelligence.", { exact: false })).toBeVisible();
});
test("hold keys can be assigned independently and duplicate holds are rejected", async ({page}) => {
  await boot(page);
  await page.getByRole("button", {name:"Dictate", exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await expect(page.getByRole("button", {name:"Hold Right Option", exact:true}).locator('[aria-hidden="true"]')).toHaveText("⌥");
  await expect(page.getByRole("button", {name:"Hold Escape", exact:true}).locator('[aria-hidden="true"]')).toHaveText("⎋");
  await expect(page.getByRole("button", {name:"Hold Fn", exact:true}).locator('[aria-hidden="true"]')).toHaveText("fn");
  await page.getByRole("button", {name:"Hold Right Option", exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("Hold:RightOption");
  await expect(page.getByText("Hold Right Option", {exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Assistant", exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await page.getByRole("button", {name:"Hold Right Option", exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("Already used by Dictate");
  await page.getByRole("button", {name:"Hold Fn", exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.assistant)).toBe("Hold:Fn");
  await expect(page.getByText(/Globe key/)).toBeVisible();
});
test("shortcut recording distinguishes a quick modifier tap, a hold, and a combination", async ({page}) => {
  await boot(page);
  await page.getByRole("button", {name:"Dictate", exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await page.keyboard.press("AltLeft");
  await page.waitForTimeout(400);
  expect(await page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("");
  await page.keyboard.down("AltLeft");
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("Hold:LeftOption");
  await page.keyboard.up("AltLeft");
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await page.keyboard.press("Alt+k");
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("Alt+K");
});
async function boot(page: Page, untrusted = false, speechStatus = 3, onboarding = false, engine = "apple", availability: Record<string, unknown> = {}) {
  await page.addInitScript(({untrusted,speechStatus,onboarding,engine,availability}) => {
    const state = { settings: {
      accessibility_prompted:!untrusted, onboarding_completed:!onboarding, launch_at_login:false,
      shortcuts:{speak:"Alt+S",stop:"Alt+X",transcribe:"",assistant:""},
      assistant:{language:"en-US",instruction:"",dictation_output:"insert"},
      engine,voice:null,rate:200,capture_mode:"copy_only",max_chars:10000,restore_clipboard:true,
      kokoro:{voice:"af_heart",quant:"q8f16",speed:1,keep_warm:false,idle_unload_minutes:5},
      chatterbox:{quant:"q4f16",voice:"en",exaggeration:0.5,cfg_weight:0.5,ref_audio:null,keep_warm:false},
      fish:{voice_id:"",model_id:"fish-s21-pro",enhance_text:false,privacy_accepted:false}
    }, voices:[], engines:[{id:"apple",label:"Apple voices",summary:"",can_speak:true,status:"Ready",blocked_reason:null,needs_download:false,download_bytes:0,repo:"",voices:[],ref_voices:[],selected_voice:""},{id:"fish",label:"Fish Audio hosted voices",summary:"Cloud",can_speak:false,status:"Sign in",blocked_reason:null,needs_download:false,download_bytes:0,repo:"",voices:[],ref_voices:[],models:[],selected_voice:""}], trusted:!untrusted,secure_input:false,speaking:false,refused_shortcuts:[],system_language:"en_US",config_path:"/test/settings.json" };
    let entries = [{id:"1",createdAt:1700000000000,text:"Synthetic transcript",source:"Test editor"}];
    Object.assign(window, {__availability:{model:"available",microphone:3,recognition:speechStatus,screen:false,imageInput:true,accessibility:!untrusted,...availability},__calls:[],__state:state,__native:async (command:string,args:any) => {
      (window as any).__calls.push({command,args});
      switch(command) {
        case "get_state": return structuredClone(state);
        case "save_settings": if ((window as any).__failSave) throw new Error("Could not save preferences"); state.settings = args.settings; return structuredClone(state);
        case "plugin:app|version": return "0.1.12";
        case "desktop_is_signed_in": return false;
        case "get_audio_history": return [];
        case "generation_history": {
          const run = {id:"audit-1",createdAt:1700000000000,source:"Chrome",status:"result",request:"Read this page",transcript:"Read this page",output:"The visible passage.",rawOutput:"1|The visible passage.",systemInstructions:"Synthetic system instructions",userPrompt:"Synthetic context then request",screenshot:"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1sAAAAASUVORK5CYII=",events:[{phase:"result",at:1700000000100,message:"Preparing narration"}]};
          return args?.id ? run : [run];
        }
        case "desktop_billing_plans": return {plans:[]};
        case "assistant_state": return {availability:structuredClone((window as any).__availability)};
        case "assistant_control": if ((window as any).__permissionError && args.command.startsWith("permission_")) throw new Error("Permission request failed"); return null;
        case "request_accessibility": state.settings.accessibility_prompted = true; return structuredClone(state);
        case "open_accessibility_settings": return null;
        case "preview_voice": return null;
        case "transcription_history": if(args.command === "delete") entries=entries.filter(e=>e.id!==args.id); if(args.command === "clear") entries=[]; return entries;
        default: throw new Error(command);
      }
    }});
  }, {untrusted,speechStatus,onboarding,engine,availability});
  await page.route(url => url.pathname === "/src/main.tsx" && !url.searchParams.has("e2e"), route => route.fulfill({contentType:"application/javascript",body:`
    import { mockIPC } from "/node_modules/@tauri-apps/api/mocks.js";
    mockIPC(window.__native,{shouldMockEvents:true});
    await import("/src/main.tsx?e2e");
  `}));
  await page.goto("/");
}
test("Assistant history exposes input output prompts and captured screenshot", async ({page}) => {
  await boot(page);
  await page.getByRole("navigation", {name:"Main"}).getByRole("button", {name:"History", exact:true}).click();
  await page.getByRole("group", {name:"History type"}).getByRole("button", {name:"Assistant", exact:true}).click();
  await page.getByRole("button", {name:"View run"}).click();
  const record = page.getByRole("article", {name:"Generation record"});
  await expect(record.getByText("The visible passage.", {exact:true})).toBeVisible();
  await record.getByText("Captured screenshot", {exact:true}).click();
  await expect(record.getByRole("img")).toBeVisible();
  await record.getByText("System instructions", {exact:true}).click();
  await expect(record.getByText("Synthetic system instructions", {exact:true})).toBeVisible();
  await record.getByText("Model input", {exact:true}).click();
  await expect(record.getByText("Synthetic context then request", {exact:true})).toBeVisible();
});

test("Home leads to tasks and Settings owns permissions", async ({page}) => {
  await boot(page);
  await expect(page.getByRole("heading",{name:"Home",exact:true})).toBeVisible();
  await expect(page.getByText(/Accessibility access granted/)).toHaveCount(0);
  await page.getByRole("button",{name:"Settings",exact:true}).click();
  await page.getByRole("button",{name:"Permissions",exact:true}).click();
  await expect(page.getByRole("heading",{name:"Permissions",exact:true})).toBeVisible();
  await expect(page.getByText("Screen Recording",{exact:true})).toBeVisible();
  await page.getByRole("button",{name:"Enable Screen Recording",exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__calls.at(-1).args.command)).toBe("permission_screen");
});
test("transcription history is separate and language changes persist",async({page})=>{
  await boot(page);
  await page.getByRole("button",{name:"Dictate",exact:true}).click();
  await page.getByLabel("Language").selectOption("fr-FR");
  await expect.poll(()=>page.evaluate(()=>(window as any).__state.settings.assistant.language)).toBe("fr-FR");
  await page.getByRole("button",{name:"History",exact:true}).click();
  await page.getByRole("button",{name:"Transcripts",exact:true}).click();
  await expect(page.getByText("Synthetic transcript",{exact:true})).toBeVisible();
  await page.getByRole("button",{name:"Delete transcript",exact:true}).click();
  await expect(page.getByText("Synthetic transcript",{exact:true})).toHaveCount(0);
  await expect(page.getByText(/No transcripts yet/)).toBeVisible();
});
test("assistant instructions persist with automatic private context and no chat launcher",async({page})=>{
  await boot(page);
  await page.getByRole("button",{name:"Assistant",exact:true}).click();
  await page.getByLabel("Assistant instructions").fill("Write concise answers.");
  await page.getByRole("heading", {name:"Assistant", exact:true}).click();
  await expect.poll(()=>page.evaluate(()=>(window as any).__state.settings.assistant.instruction)).toBe("Write concise answers.");
  await expect(page.getByText(/automatically uses selected text/)).toBeVisible();
  await expect(page.getByRole("button",{name:"Open assistant",exact:true})).toHaveCount(0);
});

test("opening Home does not request Accessibility automatically",async({page})=>{
  await boot(page,true);
  await expect(page.getByRole("heading",{name:"Home",exact:true})).toBeVisible();
  await page.waitForTimeout(300);
  expect(await page.evaluate(()=>(window as any).__calls.filter((c:any)=>c.command === "request_accessibility"))).toEqual([]);
});

test("denied Speech Recognition remains actionable",async({page})=>{
  await boot(page,false,1);
  await page.getByRole("button",{name:"Settings",exact:true}).click();
  await page.getByRole("button",{name:"Permissions",exact:true}).click();
  await expect(page.getByRole("button",{name:"Enable Speech Recognition",exact:true})).toBeEnabled();
  await expect(page.getByText("Denied",{exact:true})).toBeVisible();
});

 test("settings stay reachable without crowding primary navigation", async ({page}) => {
  await boot(page);
  const nav = page.getByRole("navigation", {name:"Main"});
  await expect(nav.getByRole("button", {name:"Capture",exact:true})).toHaveCount(0);
  await nav.getByRole("button", {name:"Settings",exact:true}).click();
  await expect(page.getByText("Open TextHalo when you log in")).toBeVisible();
  await page.getByRole("button", {name:"Text capture",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Text capture",exact:true})).toBeVisible();
});

test("setup previews a voice before permission and finishes without pricing", async ({page}) => {
  await boot(page, false, 3, true);
  await expect(page.getByRole("heading", {name:"Choose a voice"})).toBeVisible();
  await page.getByRole("button", {name:"Preview voice",exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__calls.find((call:any)=>call.command === "preview_voice").args.engine)).toBe("apple");
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Allow text access"})).toBeVisible();
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Reading shortcuts"})).toBeVisible();
  await expect(page.getByText("Compare plans", {exact:false})).toHaveCount(0);
  await page.getByRole("button", {name:"Finish setup",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Home",exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__state.settings.onboarding_completed)).toBe(true);
});

 test("setup stays open when saving fails and allows retry", async ({page}) => {
  await boot(page, false, 3, true);
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await page.getByRole("button", {name:"Skip for now",exact:true}).click();
  await page.evaluate(() => { (window as any).__failSave = true; });
  await page.getByRole("button", {name:"Finish setup",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("save");
  await expect(page.getByRole("heading", {name:"Reading shortcuts"})).toBeVisible();
  await page.evaluate(() => { (window as any).__failSave = false; });
  await page.getByRole("button", {name:"Finish setup",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Home",exact:true})).toBeVisible();
});

 test("primary screens fit the supported desktop window sizes", async ({page}, testInfo) => {
  await boot(page);
  for (const size of [{width:780,height:640},{width:680,height:480}]) {
    await page.setViewportSize(size);
    for (const name of ["Home", "Voices", "Dictate", "Assistant", "History", "Settings"]) {
      await page.getByRole("navigation",{name:"Main"}).getByRole("button",{name,exact:true}).click();
      await expect(page.locator(".pane")).toBeVisible();
      expect(await page.locator(".pane").evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
    }
  }
  await page.setViewportSize({width:780,height:640});
  await page.getByRole("button",{name:"Home",exact:true}).click();
  await page.screenshot({path:testInfo.outputPath("home-light.png")});
  await page.emulateMedia({colorScheme:"dark"});
  await page.screenshot({path:testInfo.outputPath("home-dark.png")});
});

test("setup preview preserves its selected voice source", async ({page}) => {
  await boot(page, false, 3, true, "kokoro");
  await page.evaluate(() => {
    (window as any).__state.engines.push({id:"kokoro",label:"Kokoro 82M",summary:"Local",can_speak:true,status:"Ready",needs_download:false,download_bytes:0,
      models:[{id:"q8f16",label:"Low memory",description:"Compact",bytes:86033585,installed:true}],
      voices:[{id:"af_heart",label:"Heart",language:"en",unavailable:null}],ref_voices:[],selected_voice:"af_heart"});
  });
  await page.getByRole("button", {name:"Preview voice",exact:true}).click();
  expect(await page.evaluate(() => (window as any).__state.settings.engine)).toBe("kokoro");
  const preview = await page.evaluate(() => (window as any).__calls.find((call:any) => call.command === "preview_voice"));
  expect(preview.args.engine).toBe("kokoro");
  expect(preview.args.voice).toBe("af_heart");
});

test("setup skips optional steps without enabling features or requesting access", async ({page}) => {
  await boot(page, true, 0, true);
  for (let step = 0; step < 3; step++) await page.getByRole("button", {name:"Skip for now",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Home",exact:true})).toBeVisible();
  const result=await page.evaluate(() => ({settings:(window as any).__state.settings,calls:(window as any).__calls}));
  expect(result.settings.engine).toBe("apple");
  expect(result.settings.shortcuts.transcribe).toBe("");
  expect(result.settings.shortcuts.assistant).toBe("");
  expect(result.settings.onboarding_completed).toBe(true);
  expect(result.calls.some((call:any)=>call.command === "install_engine" || call.command === "request_accessibility" || call.args?.command?.startsWith("permission_"))).toBe(false);
});

test("setup offers local and cloud voices and points optional features to Settings", async ({page}) => {
  await boot(page, false, 3, true);
  const source=page.getByLabel("Voice source",{exact:true});
  await expect(source.locator('optgroup[label="Local"]')).toHaveCount(1);
  await expect(source.locator('optgroup[label="Cloud"]')).toHaveCount(1);
  await source.selectOption("fish");
  await expect(page.getByText(/Text is sent to the cloud to generate audio/)).toBeVisible();
  await page.getByRole("button", {name:"Skip for now",exact:true}).click();
  await expect(page.getByText("Optional Dictation and Assistant permissions", {exact:true})).toHaveCount(0);
  await page.getByRole("button", {name:"Skip for now",exact:true}).click();
  await expect(page.locator(".setup-shortcut-card")).toHaveCount(2);
  await page.getByRole("button", {name:"Open Settings",exact:true}).click();
  await expect(page.getByRole("heading", {name:"General",exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Set up Dictation",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Dictate",exact:true})).toBeVisible();
});

for (const [screen, role] of [["Dictate", "transcribe"], ["Assistant", "assistant"]]) {
  test(`${screen} changes and cancels shortcuts without leaving the feature`, async ({page}) => {
    await boot(page);
    await page.getByRole("button", {name:screen,exact:true}).click();
    await page.getByRole("button", {name:"Change shortcut",exact:true}).click();
    await expect(page.getByRole("heading", {name:screen,exact:true})).toBeVisible();
    await page.keyboard.press("Control+Shift+D");
    await expect.poll(() => page.evaluate(role => (window as any).__state.settings.shortcuts[role], role)).toBe("Ctrl+Shift+D");
    await expect(page.getByText("Shortcut saved", {exact:true})).toBeVisible();
    await page.getByRole("button", {name:"Change shortcut",exact:true}).click();
    await page.keyboard.press("Escape");
    expect(await page.evaluate(role => (window as any).__state.settings.shortcuts[role], role)).toBe("Ctrl+Shift+D");
  });
}

test("inline shortcut rejects a duplicate and lets the user retry", async ({page}) => {
  await boot(page);
  await page.getByRole("button", {name:"Dictate",exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut",exact:true}).click();
  await page.keyboard.press("Alt+S");
  await expect(page.getByRole("alert")).toContainText(/already used/i);
  expect(await page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("");
  await page.keyboard.press("Control+Shift+D");
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("Ctrl+Shift+D");
});

test("Assistant requests only missing permissions and reflects grants inline", async ({page}) => {
  await boot(page, false, 0);
  await page.getByRole("button", {name:"Assistant",exact:true}).click();
  await expect(page.getByRole("button", {name:"Enable Microphone",exact:true})).toHaveCount(0);
  await expect(page.getByRole("button", {name:"Enable Screen Recording",exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Enable Speech Recognition",exact:true}).click();
  await expect(page.getByRole("button", {name:"Waiting for macOS…",exact:true})).toBeDisabled();
  await expect(page.getByRole("heading", {name:"Assistant",exact:true})).toBeVisible();
  expect(await page.evaluate(() => (window as any).__calls.filter((call:any) => call.args?.command?.startsWith("permission_")).map((call:any) => call.args.command))).toEqual(["permission_speech"]);
  await page.evaluate(async () => {
    (window as any).__availability.recognition = 3;
    const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
    await emit("texthalo:assistant-availability", structuredClone((window as any).__availability));
  });
  await expect(page.getByRole("button", {name:"Waiting for macOS…",exact:true})).toHaveCount(0);
  await expect(page.getByRole("region", {name:"Assistant access"}).getByRole("status")).toHaveText("1 needed");
  await page.getByRole("button", {name:"Enable Screen Recording",exact:true}).click();
  expect(await page.evaluate(() => (window as any).__calls.filter((call:any) => call.args?.command?.startsWith("permission_")).map((call:any) => call.args.command))).toEqual(["permission_speech", "permission_screen"]);
});

test("Dictate opens denied permission settings without requesting other access", async ({page}) => {
  await boot(page, false, 1);
  await page.getByRole("button", {name:"Dictate",exact:true}).click();
  await page.getByRole("button", {name:"Open Speech Recognition Settings",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Dictate",exact:true})).toBeVisible();
  expect(await page.evaluate(() => (window as any).__calls.filter((call:any) => call.args?.command?.startsWith("settings_")).map((call:any) => call.args.command))).toEqual(["settings_speech"]);
});

test("Listen requests Accessibility directly from Home", async ({page}) => {
  await boot(page, true);
  await page.getByRole("button", {name:"Enable access",exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__calls.filter((call:any) => call.command === "request_accessibility").length)).toBe(1);
  await expect(page.getByRole("heading", {name:"Home",exact:true})).toBeVisible();
});

 test("restricted and unknown permissions never trigger prompts", async ({page}) => {
  await boot(page, false, 2);
  await page.getByRole("button", {name:"Assistant",exact:true}).click();
  await expect(page.getByText("Restricted by macOS", {exact:true})).toBeVisible();
  await page.evaluate(async () => {
    delete (window as any).__availability.microphone;
    delete (window as any).__availability.accessibility;
    const {emit} = await import("/node_modules/@tauri-apps/api/event.js");
    await emit("texthalo:assistant-availability", structuredClone((window as any).__availability));
  });
  await expect(page.getByRole("button", {name:"Enable Microphone",exact:true})).toBeDisabled();
  await expect(page.getByRole("button", {name:"Open Accessibility Settings",exact:true})).toBeDisabled();
  await expect(page.getByText("Restricted by macOS", {exact:true})).toBeVisible();
  await expect(page.getByRole("button", {name:"Enable Speech Recognition",exact:true})).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).__calls.filter((call:any) => call.args?.command?.startsWith("permission_")).length)).toBe(0);
});

test("feature permission errors remain inline and can be retried", async ({page}) => {
  await boot(page, true, 3, false, "apple", {microphone:0});
  await page.getByRole("button", {name:"Assistant",exact:true}).click();
  await page.getByRole("button", {name:"Enable Accessibility",exact:true}).click();
  expect(await page.evaluate(() => (window as any).__calls.filter((call:any) => call.command === "request_accessibility").length)).toBe(1);
  await page.evaluate(() => { (window as any).__permissionError = true; });
  await page.getByRole("button", {name:"Enable Microphone",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("Permission request failed");
  await page.evaluate(() => { (window as any).__permissionError = false; });
  await page.getByRole("button", {name:"Enable Microphone",exact:true}).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("heading", {name:"Assistant",exact:true})).toBeVisible();
});

test("copy-only dictation does not ask for Accessibility or screen access", async ({page}) => {
  await boot(page, true);
  await page.getByRole("button", {name:"Dictate",exact:true}).click();
  await page.getByLabel("When I finish speaking").selectOption("copy");
  await expect(page.getByRole("button", {name:"Enable Accessibility",exact:true})).toHaveCount(0);
  await expect(page.getByRole("button", {name:"Enable Screen Recording",exact:true})).toHaveCount(0);
});

test("failed shortcut save stays on the feature with an error", async ({page}) => {
  await boot(page);
  await page.getByRole("button", {name:"Dictate",exact:true}).click();
  await page.evaluate(() => { (window as any).__failSave = true; });
  await page.getByRole("button", {name:"Change shortcut",exact:true}).click();
  await page.keyboard.press("Control+Shift+D");
  await expect(page.getByRole("alert")).toContainText("Could not save this shortcut");
  await expect(page.getByRole("heading", {name:"Dictate",exact:true})).toBeVisible();
  expect(await page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("");
});

test("optional recording shortcuts can be cleared directly", async ({page}) => {
  await boot(page);
  await page.getByRole("button", {name:"Dictate",exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut",exact:true}).click();
  await page.keyboard.press("Control+Shift+D");
  await expect(page.getByText("Shortcut saved", {exact:true})).toBeVisible();
  await page.getByRole("button", {name:"Clear shortcut",exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.shortcuts.transcribe)).toBe("");
  await expect(page.getByRole("heading", {name:"Dictate",exact:true})).toBeVisible();
});

test("section readiness explains missing setup and updates when ready", async ({page}) => {
  await boot(page, false, 3, false, "apple", {screen:true});
  const nav = page.getByRole("navigation", {name:"Main"});
  const dictate = page.getByRole("button", {name:"Dictate status", exact:true});
  const assistant = page.getByRole("button", {name:"Assistant status", exact:true});
  await expect(page.getByRole("button", {name:"Voices status", exact:true})).toHaveAttribute("title", "Ready to use");
  await expect(dictate).toHaveAttribute("title", /Set a Dictate shortcut/);
  await dictate.hover();
  await expect(page.getByRole("tooltip")).toContainText("Set a Dictate shortcut");
  await dictate.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await assistant.focus();
  await expect(page.getByRole("tooltip")).toContainText("Set an Assistant shortcut");
  // Test configured shortcuts through the real feature controls.
  await nav.getByRole("button", {name:"Dictate", exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await page.keyboard.press("Control+Shift+D");
  await expect(dictate).toHaveAttribute("title", "Ready to use");
  await nav.getByRole("button", {name:"Assistant", exact:true}).click();
  await page.getByRole("button", {name:"Change shortcut", exact:true}).click();
  await page.keyboard.press("Control+Shift+A");
  await expect(assistant).toHaveAttribute("title", "Ready to use");
});

test("section readiness only requires permissions needed by each feature", async ({page}) => {
  await boot(page, false, 1, false, "apple", {microphone:2, model:"unavailable", screen:false});
  await expect(page.getByRole("button", {name:"Dictate status", exact:true})).toHaveAttribute("title", /Microphone access is denied/);
  await expect(page.getByRole("button", {name:"Dictate status", exact:true})).toHaveAttribute("title", /Speech Recognition access is denied/);
  await expect(page.getByRole("button", {name:"Assistant status", exact:true})).toHaveAttribute("title", /Apple Intelligence/);
  await expect(page.getByRole("button", {name:"Voices status", exact:true})).toHaveAttribute("title", "Ready to use");
});


test("Assistant requires screen permission for native image context and explains privacy", async ({page}) => {
  await boot(page);
  await page.getByRole("navigation", {name:"Main"}).getByRole("button", {name:"Assistant", exact:true}).click();
  await expect(page.getByRole("button", {name:"Enable Screen Recording", exact:true})).toBeVisible();
  await expect(page.getByRole("button", {name:"Assistant status", exact:true})).toHaveAttribute("title", /Allow Screen Recording/);
  await expect(page.getByText(/captures your current window only while you use Assistant/)).toBeVisible();
});

test("Assistant remains usable when this build has no native image input", async ({page}) => {
  await boot(page, false, 3, false, "apple", {imageInput:false, screen:false});
  await page.getByRole("navigation", {name:"Main"}).getByRole("button", {name:"Assistant", exact:true}).click();
  await expect(page.getByText("Visual context unavailable in this build", {exact:true})).toBeVisible();
  await expect(page.getByRole("button", {name:"Assistant status", exact:true})).not.toHaveAttribute("title", /Screen Recording/);
});
