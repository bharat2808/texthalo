import {readFileSync,mkdirSync,renameSync} from 'node:fs';
import {dirname,join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {randomUUID} from 'node:crypto';
import {spawnSync} from 'node:child_process';
const root=dirname(fileURLToPath(import.meta.url));
const manifest=JSON.parse(readFileSync(join(root,'manifest.json'),'utf8'));
const output=join(root,'output','store');mkdirSync(output,{recursive:true});
const destination=resolve(output,`texthalo-reader-${manifest.version}.zip`);
const temporary=destination+'.'+randomUUID()+'.zip';
// Explicit allowlist: never ship captures, OCR, audio, native binaries or test fixtures.
const files=['manifest.json','kindle.js','background.js','popup.html','popup.js','popup.css',...Object.values(manifest.icons)];
const result=spawnSync('/usr/bin/zip',['-j',temporary,...files.filter(f=>!f.startsWith('icons/'))],{cwd:root,stdio:'inherit'});
if(result.status!==0)process.exit(result.status||1);
const icons=spawnSync('/usr/bin/zip',[temporary,...files.filter(f=>f.startsWith('icons/'))],{cwd:root,stdio:'inherit'});
if(icons.status!==0)process.exit(icons.status||1);
renameSync(temporary,destination);
console.log(destination);
