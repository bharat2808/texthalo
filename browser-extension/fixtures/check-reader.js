async page => {
  await page.route('https://read.amazon.ca/**', route => route.fulfill({path:'browser-extension/fixtures/reader.html',contentType:'text/html'}));
  await page.goto('https://read.amazon.ca/?asin=B0GY3RKVRZ');
  const worker=page.context().serviceWorkers()[0];
  if(!worker)throw Error('Extension did not load');
  const send=message=>worker.evaluate(async message=>{const [tab]=await chrome.tabs.query({url:'https://read.amazon.ca/*'});return chrome.tabs.sendMessage(tab.id,message);},message);
  const before=await send({protocolVersion:1,type:'inspect'});
  if(before.data.status!=='ready'||before.data.title!=='TextHalo reader test book')throw Error('Inspection or title failed');
  const next=await send({protocolVersion:1,type:'navigate',direction:'next',bookId:before.data.bookId,fingerprint:before.data.fingerprint});
  if(!next.ok||!next.data.navigationVerified||next.data.progress!=='Page 9 of 134')throw Error('Next page verification failed');
  const stale=await send({protocolVersion:1,type:'navigate',direction:'next',bookId:before.data.bookId,fingerprint:before.data.fingerprint});
  if(stale.ok)throw Error('Stale navigation accepted');
  const previous=await send({protocolVersion:1,type:'navigate',direction:'previous',bookId:next.data.bookId,fingerprint:next.data.fingerprint});
  if(!previous.ok||previous.data.progress!=='Page 8 of 134')throw Error('Previous page verification failed');
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const panel=await send({protocolVersion:1,type:'inspect'});
  if(panel.data.status==='ready')throw Error('Covering panel accepted');
  await page.getByRole('button',{name:'Close',exact:true}).click();
  await page.setViewportSize({width:720,height:900});
  const narrow=await send({protocolVersion:1,type:'inspect'});
  if(narrow.data.status!=='ready'||narrow.data.region.width>=before.data.region.width)throw Error('Resize failed');
  await page.setViewportSize({width:1280,height:900});
  return {title:before.data.title,next:next.data.progress,previous:previous.data.progress,staleRefused:!stale.ok,panelRefused:panel.data.status,narrowBounds:narrow.data.region};
}
