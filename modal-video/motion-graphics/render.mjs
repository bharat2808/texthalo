import path from 'node:path';
import fs from 'node:fs/promises';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { createHash } from 'node:crypto';
import sharp from 'sharp';
import { WIDTH, HEIGHT, FPS, DURATION, createStoryboard, demoLanguages } from './storyboard.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const output = path.join(root, 'website/public/videos');
// The public narration is the only content sent to Fish Audio.
// Keep private API credentials in the service environment, never in site assets.
if (!process.env.FISH_AUDIO_API_KEY) process.loadEnvFile(path.join(root, 'texthalo-server/.env'));
const fishKey = process.env.FISH_AUDIO_API_KEY;
if (!fishKey) throw new Error('Configure FISH_AUDIO_API_KEY to render Fish Audio narration.');
const fishModel = process.env.FISH_AUDIO_MODEL || 's2.1-pro';
const fishVoice = process.env.FISH_DEMO_VOICE_ID || '0efe38923c0b445ab09382e851ae1e30';
const voiceCache = path.join(root, 'modal-video/output/marketing/narration');
function run(command, args) {
  const result = spawnSync(command, args, {encoding:'utf8'});
  if(result.status!==0) throw new Error(`${command}: ${result.stderr || result.error}`);
  return result.stdout;
}
const args = process.argv.slice(2);
const validArgs = args.length === 0 || (args.length === 1 && args[0] === '--all')
  || (args.length === 2 && args[0] === '--language' && demoLanguages.includes(args[1]));
if (!validArgs) throw new Error('Usage: npm run website:demo -- [--all | --language <locale>]');
const selected = args[0] === '--all' ? demoLanguages : [args[1] ?? 'en'];
await fs.mkdir(output, { recursive: true });
for (const language of selected) await renderLanguage(language);

async function renderLanguage(language) {
  const { narration, frameSvg, prepareTypography } = createStoryboard(language);
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), `texthalo-demo-${language}-`));
  const basename = `texthalo-workflow-demo${language === 'en' ? '' : `-${language}`}`;
  const movie = path.join(output, `${basename}.mp4`);
  const poster = path.join(output, `${basename}-poster.png`);
  const captions = path.join(output, `${basename}.vtt`);
  const proof = path.join(temp, 'contact-sheet.png');
  const encodedMovie = path.join(temp, 'workflow-demo.mp4');
  console.log(`Rendering ${language}`);
  await prepareTypography(sharp);
  try {
    await fs.mkdir(voiceCache, {recursive:true});
    const audio = [];
    const cues = [];
    const timestamp = seconds => {
      const ms = Math.round(seconds * 1000);
      return `${String(Math.floor(ms / 3600000)).padStart(2,'0')}:${String(Math.floor(ms / 60000) % 60).padStart(2,'0')}:${String(Math.floor(ms / 1000) % 60).padStart(2,'0')}.${String(ms % 1000).padStart(3,'0')}`;
    };
    for (const [i, cue] of narration.entries()) {
      const hash = createHash('sha256').update(JSON.stringify({text:cue.text,voice:fishVoice,model:fishModel})).digest('hex').slice(0,20);
      const take=path.join(voiceCache,`fish-${hash}.mp3`), wav=path.join(temp,`voice-${i}.wav`);
      try { await fs.access(take); }
      catch {
        console.log(`Generating Fish Audio narration ${i+1}/${narration.length}`);
        const response=await fetch('https://api.fish.audio/v1/tts', {
          method:'POST',
          headers:{Authorization:`Bearer ${fishKey}`, 'Content-Type':'application/json', model:fishModel},
          body:JSON.stringify({text:cue.text,reference_id:fishVoice,format:'mp3'}),
          signal:AbortSignal.timeout(60000),
        });
        if(!response.ok) throw new Error(`Fish Audio narration rejected (HTTP ${response.status}).`);
        await fs.writeFile(take,Buffer.from(await response.arrayBuffer()));
      }
      const length=Number(run('ffprobe',['-v','error','-show_entries','format=duration','-of','csv=p=0',take]).trim());
      // Fit the take to its scene with a small lead-in and tail.
      if (!Number.isFinite(length) || length <= 0) throw new Error(`Invalid narration duration for ${language}, scene ${i+1}.`);
      const tempo=Math.max(1,length/(cue.duration-.35));
      if (tempo > 1.35) throw new Error(`${language}, scene ${i+1} needs shorter narration (${tempo.toFixed(2)}× tempo).`);
      console.log(`Narration ${i+1}: ${length.toFixed(2)}s; tempo ${tempo.toFixed(2)}×`);
      run('ffmpeg',['-y','-loglevel','error','-i',take,'-af',`aresample=48000,atempo=${tempo},adelay=150:all=1,apad,atrim=end_sample=${cue.duration*48000},asetpts=N/SR/TB`,'-ar','48000','-ac','2',wav]);
      const paddedLength=Number(run('ffprobe',['-v','error','-show_entries','format=duration','-of','csv=p=0',wav]).trim());
      if(Math.abs(paddedLength-cue.duration)>.02) throw new Error(`Narration scene ${i+1} has incorrect timing.`);
      audio.push(wav);
      cues.push(`${timestamp(cue.at + .15)} --> ${timestamp(cue.at + Math.min(cue.duration, length / tempo + .15))}\n${cue.text}`);
    }
    const captionText = `WEBVTT\n\n${cues.join('\n\n')}\n`;
    await fs.writeFile(path.join(temp,'audio.txt'),audio.map(f=>`file '${f}'`).join('\n'));
    const speech=path.join(temp,'narration.wav');
    run('ffmpeg',['-y','-loglevel','error','-f','concat','-safe','0','-i',path.join(temp,'audio.txt'),'-c:a','pcm_s16le',speech]);
    run('python3',[path.join(here,'soundtrack.py')]);
    const score=path.join(root,'modal-video/output/marketing/texthalo-original-soundtrack.wav');
    const encoder=spawn('ffmpeg',['-y','-loglevel','error','-f','image2pipe','-framerate',String(FPS),'-vcodec','png','-i','pipe:0','-i',speech,'-i',score,'-filter_complex','[1:a]volume=1.2[voice];[2:a]volume=0.30[music];[voice][music]amix=inputs=2:duration=first:normalize=0,alimiter=limit=0.95[a]','-map','0:v','-map','[a]','-t',String(DURATION),'-c:v','libx264','-crf','19','-preset','medium','-pix_fmt','yuv420p','-c:a','aac','-b:a','160k','-movflags','+faststart',encodedMovie],{stdio:['pipe','ignore','pipe']});
    let error=''; encoder.stderr.on('data',chunk=>{error+=chunk;});
    const complete=once(encoder,'close');
    for(let i=0;i<DURATION*FPS;i++) {
      const png=await sharp(Buffer.from(frameSvg(i/FPS))).png().toBuffer();
      if(!encoder.stdin.write(png)) await once(encoder.stdin,'drain');
      if(i%150===0) console.log(`Rendered ${i/FPS}s / ${DURATION}s`);
    }
    encoder.stdin.end();
    const [code]=await complete;
    if(code!==0) throw new Error(`Video encoding failed: ${error}`);
    await fs.copyFile(encodedMovie,movie);
    await fs.writeFile(captions, captionText);
    await sharp(Buffer.from(frameSvg(1.8))).png().toFile(poster);
    const thumbs=await Promise.all([1.8,7,15,23.5,28.5].map(t=>sharp(Buffer.from(frameSvg(t))).resize(640,360).png().toBuffer()));
    await sharp({create:{width:1280,height:1080,channels:3,background:'#f5f4ef'}}).composite(thumbs.map((input,i)=>({input,left:(i%2)*640,top:Math.floor(i/2)*360}))).png().toFile(proof);
    console.log(JSON.stringify({language,movie,poster,contactSheet:proof,width:WIDTH,height:HEIGHT,duration:DURATION}));
  } catch(error) {
    console.error(`Render inputs preserved at ${temp}`);
    throw error;
  }

}
