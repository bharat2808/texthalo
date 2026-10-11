import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFileSync} from 'node:fs';

function readerFixture({progress='Location 1 of 1583 ● 0%',previous=false,next=true,previousDisabled=false,nextDisabled=false}={}) {
  class Element {
    constructor(tag,text,rect,attributes={}) {Object.assign(this,{tagName:tag,textContent:text,rect,attributes,parentElement:null,className:'',childElementCount:0});}
    getAttribute(key) {return this.attributes[key]??null;}
    getBoundingClientRect() {return this.rect;}
    contains(other) {return this===other;}
  }
  const surface=new Element('CANVAS','',{x:200,y:150,width:600,height:700});
  const indicator=new Element('DIV',progress,{x:300,y:920,width:300,height:20});
  const title=new Element('DIV','Test Kindle Book',{x:200,y:20,width:400,height:30});
  const buttons=[];
  for(const [label,present,disabled] of [['Previous page',previous,previousDisabled],['Next page',next,nextDisabled]]) {
    if(present) {const button=new Element('BUTTON',label,{x:50,y:300,width:40,height:40});button.disabled=disabled;buttons.push(button);}
  }
  const context={Element,document:{hidden:false,querySelectorAll(selector){
    if(selector==='body *') return [indicator];
    if(selector==='h1,h2,[title],[aria-label],div,span') return [title];
    if(selector==='canvas,img,iframe,svg,div') return [surface];
    if(selector.includes('button')) return buttons;
    return [];
  },elementsFromPoint(){return [surface];}},window:{},innerWidth:1000,innerHeight:1000,devicePixelRatio:1,
  visualViewport:{scale:1},getComputedStyle:()=>({display:'block',visibility:'visible',opacity:'1',pointerEvents:'auto',backgroundImage:'none'}),
  location:{hostname:'read.amazon.ca',href:'https://read.amazon.ca/?asin=B0GY3RKVRZ'},URL,setInterval(){},
  chrome:{runtime:{id:'test',onMessage:{addListener(){}}}}};
  vm.runInNewContext(readFileSync(new URL('../browser-extension/kindle.js',import.meta.url),'utf8'),context);
  return context.__textHaloReader.inspect();
}

test('Kindle removes Previous at location one rather than disabling it',()=>{
  const state=readerFixture();
  assert.equal(state.status,'ready');
  assert.equal(state.startOfBook,true);
  assert.equal(state.endOfBook,false);
});
test('the final location with a removed Next control marks the end',()=>{
  const state=readerFixture({progress:'Location 1583 of 1583 ● 100%',previous:true,next:false});
  assert.equal(state.endOfBook,true);
  assert.equal(state.startOfBook,false);
});
test('page one can still have a preceding cover view',()=>{
  assert.equal(readerFixture({progress:'Page 1 of 134 ● 0%',previous:true}).startOfBook,false);
});
test('a missing control away from a verified edge is not a book boundary',()=>{
  const state=readerFixture({progress:'Page 12 of 134 ● 8%'});
  assert.equal(state.startOfBook,false);
});
test('missing both controls does not prove either boundary',()=>{
  const state=readerFixture({previous:false,next:false});
  assert.equal(state.startOfBook,false);
  assert.equal(state.endOfBook,false);
});
test('disabled controls still mark the corresponding book boundary',()=>{
  assert.equal(readerFixture({previous:true,previousDisabled:true}).startOfBook,true);
  assert.equal(readerFixture({progress:'Page 134 of 134 ● 100%',previous:true,nextDisabled:true}).endOfBook,true);
});

test('Roman-numbered opening pages expose the real reading position', () => {
  const state = readerFixture({ progress: 'Page iii of 405 • 0%', previous: true });
  assert.equal(state.status, 'ready');
  assert.equal(state.progress, 'Page iii of 405 • 0%');
  assert.equal(state.startOfBook, false);
  assert.equal(state.endOfBook, false);
});
test('Roman page one is a beginning only when there is no preceding view', () => {
  assert.equal(readerFixture({ progress: 'Page i of 405 • 0%' }).startOfBook, true);
  assert.equal(readerFixture({ progress: 'Page i of 405 • 0%', previous: true }).startOfBook, false);
});
test('Roman front matter is not compared with the numbered page total', () => {
  const state = readerFixture({ progress: 'Page iv of 4 • 0%', previous: true, next: false });
  assert.equal(state.status, 'ready');
  assert.equal(state.endOfBook, false);
});
test('invalid Roman page labels and Roman locations are refused', () => {
  for (const progress of ['Page iiii of 405', 'Page invalid of 405', 'Location iii of 405']) {
    assert.notEqual(readerFixture({ progress }).status, 'ready');
  }
});
