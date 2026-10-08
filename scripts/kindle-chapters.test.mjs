import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import {readFileSync} from "node:fs";

function sidebarFixture(labels = ["Chapter One", "Chapter Two"]) {
  let opened = false, position = "page5";
  class Element {
    constructor(label, click) { this.textContent = label; this.click = click; this.parentElement = null; }
    getAttribute() { return null; }
    getBoundingClientRect() { return {x:10,y:10,width:100,height:30}; }
    scrollIntoView() {}
    dispatchEvent() {}
  }
  const entries = labels.map(label => new Element(label, () => { position=label; opened=false; }));
  const toggle = new Element("Table of Contents", () => { opened=!opened; });
  const close = new Element("Close", () => { opened=false; });
  const list = new Element("", () => {});
  list.querySelectorAll=()=>entries;
  const document = { hidden:false, querySelectorAll(selector) {
    if (selector==='[role="listbox"]') return opened ? [list] : [];
    if (selector.includes('button')) return opened ? [toggle,...entries,close] : [toggle];
    return [];
  }};
  const context = {document, Element, window:{}, MouseEvent:class {}, innerWidth:1000,innerHeight:1000,
    getComputedStyle:()=>({display:"block",visibility:"visible",opacity:"1"}),
    location:{hostname:"read.amazon.ca",href:"https://read.amazon.ca/?asin=B0GY3RKVRZ"},URL,Date,
    setInterval(){},setTimeout(fn){fn();},
    chrome:{runtime:{id:"test",onMessage:{addListener(){}}}},
    fixtureReader:()=>({status:"ready",bookId:"B0GY3RKVRZ",fingerprint:position})};
  const source=readFileSync(new URL('../browser-extension/kindle.js',import.meta.url),'utf8')
    .replace('const api = {', 'inspect = globalThis.fixtureReader;\n  const api = {');
  vm.runInNewContext(source,context);
  return {api:context.__textHaloReader,opened:()=>opened,position:()=>position};
}
test("reading the chapter list closes the sidebar without turning the page",async()=>{
  const h=sidebarFixture();
  const result=await h.api.chapters({bookId:"B0GY3RKVRZ",fingerprint:"page5"});
  assert.deepEqual(Array.from(result.chapters,c=>c.label),["Chapter One","Chapter Two"]);
  assert.equal(h.opened(),false);
  assert.equal(h.position(),"page5");
});
test("chapter navigation verifies the entry identity and a changed reading position",async()=>{
  const h=sidebarFixture();
  const result=await h.api.chapters({bookId:"B0GY3RKVRZ",fingerprint:"page5"});
  const reader=await h.api.chapterNavigate({bookId:"B0GY3RKVRZ",fingerprint:"page5",chapterId:result.chapters[1].id});
  assert.equal(reader.chapter.label,"Chapter Two");
  assert.equal(reader.navigationVerified,true);
  assert.equal(h.opened(),false);
});
test("stale chapter identities do not navigate or leave the sidebar open",async()=>{
  const h=sidebarFixture();
  await assert.rejects(h.api.chapterNavigate({bookId:"B0GY3RKVRZ",fingerprint:"page5",chapterId:"stale"}),/chapter.*changed|chapter.*unavailable/i);
  assert.equal(h.position(),"page5");
  assert.equal(h.opened(),false);
});
