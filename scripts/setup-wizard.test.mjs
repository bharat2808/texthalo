import {test} from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createServer} from 'vite';

test('setup introduces every feature with optional steps and a complete skip path', async () => {
  const vite=await createServer({server:{middlewareMode:true,hmr:false},appType:'custom'});
  try {
    const {default:Wizard}=await vite.ssrLoadModule('/src/SetupWizard.tsx');
    const html=renderToStaticMarkup(React.createElement(Wizard,{
      settings:{engine:'chatterbox',onboarding_completed:false,launch_at_login:false,accessibility_prompted:false,
        shortcuts:{speak:'Alt+S',stop:'Alt+X',transcribe:'',assistant:''},voice:null,
        kokoro:{voice:'af_heart',quant:'fp32',speed:1,keep_warm:true,idle_unload_minutes:30},
        chatterbox:{voice:'en',quant:'q4f16',exaggeration:0.1,cfg_weight:0.5,ref_audio:null,keep_warm:true}},
      engines:[],voices:[],trusted:false,externalError:null,recordingShortcut:null,
      downloadBusy:false,install:null,speaking:false,onSave:async()=>{},onDownload:async()=>{},
      onRequestAccessibility(){},onOpenAccessibilitySettings(){},onPreviewVoice:async()=>{},onStopPreview(){},onRecordShortcut(){},onFinish(){},
    }));
    assert.match(html,/>Listen</);
    assert.match(html,/>Dictate</);
    assert.match(html,/>Assistant</);
    assert.match(html,/Skip setup/);
    assert.match(html,/Skip for now/);
    assert.match(html,/Step 1 of 4/);
    assert.doesNotMatch(html,/ONE QUICK PERMISSION/);
  } finally {await vite.close();}
});
