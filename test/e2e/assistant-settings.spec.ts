import { expect, test, type Page } from "@playwright/test";
async function boot(page: Page, untrusted = false, speechStatus = 3, onboarding = false, engine = "apple") {
  await page.addInitScript(({untrusted,speechStatus,onboarding,engine}) => {
    const state = { settings: {
      accessibility_prompted:!untrusted, onboarding_completed:!onboarding, launch_at_login:false,
      shortcuts:{speak:"Alt+S",stop:"Alt+X",transcribe:"",assistant:""},
      assistant:{language:"en-US",instruction:"",dictation_output:"insert",context:"selection"},
      engine,voice:null,rate:200,capture_mode:"copy_only",max_chars:10000,restore_clipboard:true,
      kokoro:{voice:"af_heart",quant:"q8",speed:1,keep_warm:false,idle_unload_minutes:5},
      chatterbox:{voice:"en",exaggeration:0.5,cfg_weight:0.5,ref_audio:null,keep_warm:false},
      fish:{voice_id:"",model_id:"fish-s21-pro",enhance_text:false,privacy_accepted:false}
    }, voices:[], engines:[{id:"apple",label:"Apple voices",summary:"",can_speak:true,status:"Ready",blocked_reason:null,needs_download:false,download_bytes:0,repo:"",voices:[],ref_voices:[],selected_voice:""}], trusted:!untrusted,secure_input:false,speaking:false,refused_shortcuts:[],system_language:"en_US",config_path:"/test/settings.json" };
    let entries = [{id:"1",createdAt:1700000000000,text:"Synthetic transcript",source:"Test editor"}];
    Object.assign(window, {__calls:[],__state:state,__native:async (command:string,args:any) => {
      (window as any).__calls.push({command,args});
      switch(command) {
        case "get_state": return structuredClone(state);
        case "save_settings": if ((window as any).__failSave) throw new Error("Could not save preferences"); state.settings = args.settings; return structuredClone(state);
        case "plugin:app|version": return "0.1.12";
        case "desktop_is_signed_in": return false;
        case "get_audio_history": return [];
        case "desktop_billing_plans": return {plans:[]};
        case "assistant_state": return {availability:{model:"available",microphone:3,recognition:speechStatus,screen:false,accessibility:true}};
        case "assistant_control": return null;
        case "preview_voice": return null;
        case "transcription_history": if(args.command === "delete") entries=entries.filter(e=>e.id!==args.id); if(args.command === "clear") entries=[]; return entries;
        default: throw new Error(command);
      }
    }});
  }, {untrusted,speechStatus,onboarding,engine});
  await page.route("**/src/main.tsx", route => route.fulfill({contentType:"application/javascript",body:`
    import { mockIPC } from "/node_modules/@tauri-apps/api/mocks.js";
    mockIPC(window.__native,{shouldMockEvents:true});
    await import("/src/main.tsx?e2e");
  `}));
  await page.goto("/");
}
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
test("assistant instructions persist with window context and no chat launcher",async({page})=>{
  await boot(page);
  await page.getByRole("button",{name:"Assistant",exact:true}).click();
  await page.getByLabel("Assistant instructions").fill("Write concise answers.");
  await page.getByLabel("Assistant context").selectOption("screen");
  await expect.poll(()=>page.evaluate(()=>(window as any).__state.settings.assistant.instruction)).toBe("Write concise answers.");
  await expect.poll(()=>page.evaluate(()=>(window as any).__state.settings.assistant.context)).toBe("screen");
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
  await expect(page.getByRole("heading", {name:"Find your voice"})).toBeVisible();
  await page.getByRole("button", {name:"Preview voice",exact:true}).click();
  expect(await page.evaluate(()=>(window as any).__calls.find((call:any)=>call.command === "preview_voice").args.engine)).toBe("apple");
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Read from any app"})).toBeVisible();
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Try your shortcut"})).toBeVisible();
  await expect(page.getByText("Compare plans", {exact:false})).toHaveCount(0);
  await page.getByRole("button", {name:"Finish setup",exact:true}).click();
  await expect(page.getByRole("heading", {name:"Home",exact:true})).toBeVisible();
  expect(await page.evaluate(()=>(window as any).__state.settings.onboarding_completed)).toBe(true);
});

 test("setup stays open when saving fails and allows retry", async ({page}) => {
  await boot(page, false, 3, true);
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await page.getByRole("button", {name:"Continue",exact:true}).click();
  await page.evaluate(() => { (window as any).__failSave = true; });
  await page.getByRole("button", {name:"Finish setup",exact:true}).click();
  await expect(page.getByRole("alert")).toContainText("save");
  await expect(page.getByRole("heading", {name:"Try your shortcut"})).toBeVisible();
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

test("setup preview selects its voice source before playing", async ({page}) => {
  await boot(page, false, 3, true, "kokoro");
  await page.getByRole("button", {name:"Preview voice",exact:true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).__state.settings.engine)).toBe("apple");
  const commands = await page.evaluate(() => (window as any).__calls.map((call:any) => call.command));
  expect(commands.indexOf("save_settings")).toBeLessThan(commands.indexOf("preview_voice"));
});
