// Illustrated interactions follow the current Listen, Dictate, and Assistant UI.
export const WIDTH = 1280;
export const HEIGHT = 720;
export const FPS = 30;
export const DURATION = 30;
export const narration = [
  { at: 0, duration: 5, text: 'TextHalo. Listen, dictate, and get a little help.' },
  { at: 5, duration: 7, text: 'Select a passage, then press your shortcut. Hear it read aloud, and stop whenever you like.' },
  { at: 12, duration: 7, text: 'Hold your dictation shortcut and speak. Release to put your words into the current app, or copy them.' },
  { at: 19, duration: 7, text: 'Need a clearer draft? Select the text, hold your Assistant shortcut, and ask for an edit.' },
  { at: 26, duration: 4, text: 'Your words. Your way. TextHalo for Mac.' },
];
const C = { paper:'#f5f4ef', ink:'#18231f', muted:'#707a72', green:'#c8f06b', forest:'#183c32', line:'#e1e5da' };
const escape = s => String(s).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;');
const smooth = x => {x=Math.max(0,Math.min(1,x));return x*x*(3-2*x);};
const rect = (x,y,w,h,fill,rx=0,stroke='none') => `<rect x="${x}" y="${y}" width="${w}" height="${h}" fill="${fill}" rx="${rx}" stroke="${stroke}"/>`;
const text = (x,y,value,size=18,color=C.ink,weight=400,anchor='start') => `<text x="${x}" y="${y}" font-family="Avenir Next, Helvetica, sans-serif" font-size="${size}" font-weight="${weight}" fill="${color}" text-anchor="${anchor}">${escape(value)}</text>`;
const lines = (x,y,values,size=18,color=C.ink,weight=400,lineHeight=size*1.45) => values.map((v,i)=>text(x,y+i*lineHeight,v,size,color,weight)).join('');
function mark(x,y,scale=1,light=false) {
 return `<g transform="translate(${x} ${y}) scale(${scale})"><circle cx="16" cy="16" r="15" fill="none" stroke="${light?'#e2eddb':C.forest}" stroke-width="2"/><circle cx="21" cy="12" r="7" fill="none" stroke="${light?C.green:'#7baf46'}" stroke-width="2"/><circle cx="10" cy="23" r="3" fill="${light?'#e2eddb':C.forest}"/></g>`;
}
function masthead(dark=false) {
 return mark(64,40,1,dark)+text(110,64,'TextHalo',26,dark?'#f5f4ef':C.ink,700)+text(1216,60,'ANIMATED WORKFLOW DEMO',12,dark?'#c0ceba':C.muted,500,'end');
}
function windowFrame(title,content) {
 return `<g filter="url(#shadow)">${rect(527,166,689,386,'#ccd4c6',16,C.line)}</g>`+rect(527,166,689,386,'#fff',16,C.line)+rect(528,167,687,45,'#fafaf7',15)+rect(528,196,687,16,'#fafaf7')+['#e5a39b','#e8c880','#a9c49a'].map((c,i)=>`<circle cx="${549+i*16}" cy="190" r="4.5" fill="${c}"/>`).join('')+text(870,195,title,13,C.muted,500,'middle')+content;
}
function overlay(t,label,sub='') {
 const wave=Array.from({length:5},(_,i)=>{const h=7+Math.sin(t*8+i*.8)**2*17;return rect(795+i*7,118-h/2,3,h,'#a4c578',1.5);}).join('');
 return `<g filter="url(#shadow)">${rect(777,94,270,48,'#ccd4c6',24,C.line)}</g>`+rect(777,94,270,48,'#fff',24,C.line)+wave+text(842,124,label,15,C.ink,500)+rect(1019,113,9,9,C.muted,2)+(sub?text(912,163,sub,12,C.muted,400,'middle'):'');
}
function copy(label,title,subtitle) {
 return text(64,233,label,12,'#688052',600)+lines(64,296,title,53,C.ink,600,58)+lines(64,450,subtitle,19,C.muted,400,29);
}
function listen(t) {
 const selected=smooth((t-.4)/.7), playing=t>2.4;
 const content=text(569,265,'A NOTE TO MYSELF',12,C.muted,600)+text(569,308,'Make room for a new idea',29,C.ink,600)+lines(569,358,['Some days, a moment away from the screen','is all it takes to see things a little differently.'],18,C.muted)+rect(561,408,590*selected,35,'#dceebf',4)+text(569,433,'A short break can make room for a new idea.',23,C.ink,500)+text(569,485,'Keep your place. Take your time.',18,C.muted);
 const keys=rect(753,577,259,53,'#e9eddf',9)+text(883,611,'⌘  ⇧  S',24,C.forest,600,'middle');
 return copy('01 / LISTEN',['Give your eyes','a little break.'],['Select text. Press your shortcut.','Listen while you keep your flow.'])+windowFrame('Notes · Selected text',content)+keys+(playing?overlay(t,'Speaking'):text(884,120,'Select a passage',15,C.muted,400,'middle'));
}
function dictate(t) {
 const phrase='Let’s make time for a fresh idea tomorrow.';
 const typed=t>=4.6?phrase:phrase.slice(0,Math.floor(Math.max(0,t-.8)/3.3*phrase.length));
 const content=text(569,265,'A NEW MESSAGE',12,C.muted,600)+text(569,309,'Tomorrow’s plan',29,C.ink,600)+text(569,366,'Hi everyone,',20,C.ink)+text(569,414,typed,23,C.ink)+(t<4.6?rect(570+Math.min(560,typed.length*11),394,2,27,'#719849'):text(569,476,'See you then.',20,C.muted));
 return copy('02 / DICTATE',['Say it.','Write it.'],['Hold your shortcut and speak.','Release to insert or copy your words.'])+windowFrame('Mail · Example draft',content)+overlay(t,t<4.6?'Listening…':'Inserted',t<4.6?'Hold your dictation shortcut':'Release to finish')+rect(620,584,501,48,'#e9eddf',9)+text(870,614,'On-device transcription · macOS or Whisper',15,C.forest,500,'middle');
}
function assistant(t) {
 const edited=t>=4.1;
 const content=text(569,265,'A DRAFT IN PROGRESS',12,C.muted,600)+text(569,310,'Keep it clear',29,C.ink,600)+(edited?rect(562,360,577,49,'#e8f2db',5)+text(575,391,'Can we discuss the next steps?',26,C.forest,600):rect(562,349,589,105,'#dceebf',5)+lines(575,383,['I was wondering if we could maybe find','some time to talk about the next steps.'],22,C.ink,400,36))+(edited?text(569,463,'A little clearer. A little shorter.',18,C.muted):'');
 return copy('03 / ASSISTANT',['A clearer draft,','a request away.'],['Select text. Hold your shortcut.','Ask Assistant for the edit you need.'])+windowFrame('Notes · Example edit',content)+overlay(t,t<2?'Listening…':edited?'Edit applied':'Generating…')+rect(636,577,472,62,'#e9eddf',12)+text(872,600,'EXAMPLE REQUEST',10,C.muted,600,'middle')+text(872,626,'“Make this more concise.”',22,C.forest,500,'middle');
}
function intro(t) {
 const lift=12*(1-smooth(t/.6));
 return `<g transform="translate(0 ${lift})">`+text(640,235,'Your words.',69,C.ink,600,'middle')+text(640,316,'More possibilities.',69,'#628744',600,'middle')+text(640,373,'Three ways to work, one shortcut away.',22,C.muted,400,'middle')+['Listen','Dictate','Assistant'].map((v,i)=>rect(271+i*250,435,238,98,i===2?'#e9eddf':'#fff',14,C.line)+text(390+i*250,490,v,25,C.forest,600,'middle')).join('')+text(640,606,'For your Mac. In the apps you already use.',17,C.muted,400,'middle')+'</g>';
}
function outro() {
 return `<circle cx="640" cy="370" r="294" fill="none" stroke="#3c5847" opacity=".5"/><circle cx="640" cy="370" r="237" fill="none" stroke="#3c5847" opacity=".5"/>`+mark(483,182,1.6,true)+text(555,222,'TextHalo',44,'#f5f4ef',700)+lines(640,327,['Listen. Dictate.','Get a little help.'],57,'#f5f4ef',600,67).replaceAll('text-anchor="start"','text-anchor="middle"')+text(640,479,'Free local features · Optional paid cloud services',18,'#c0ceba',400,'middle')+rect(510,528,260,54,C.green,10)+text(640,562,'texthalo.app',23,C.forest,600,'middle');
}
export function frameSvg(time) {
 const index=Math.max(0,narration.findLastIndex(cue=>time>=cue.at));
 const start=narration[index].at;
 const dark=index===4;
 const body=[intro,listen,dictate,assistant,outro][index](time-start);
 const opacity=start===0?1:smooth((time-start)/.28);
 return `<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}"><defs><filter id="shadow" x="-20%" y="-30%" width="140%" height="170%"><feGaussianBlur stdDeviation="14"/></filter></defs>${rect(0,0,WIDTH,HEIGHT,dark?C.forest:C.paper)}${masthead(dark)}<g opacity="${opacity}">${body}</g>${rect(64,672,1152,3,dark?'#375346':'#e1e5da',1.5)}${rect(64,672,Math.max(1,1152*time/DURATION),3,dark?C.green:'#819d65',1.5)}</svg>`;
}
