import {test} from 'node:test';
import assert from 'node:assert/strict';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createServer} from 'vite';

function control(tree, type) {
  if (tree?.type === type) return tree;
  for (const child of React.Children.toArray(tree?.props?.children)) {
    const found = control(child, type);
    if (found) return found;
  }
}

test('Kokoro picker shows installed variants and forwards the chosen model ID', async () => {
  const vite = await createServer({server:{middlewareMode:true},appType:'custom'});
  try {
    const {default:Picker}=await vite.ssrLoadModule('/src/KokoroModelPicker.tsx');
    const models=[
      {id:'fp32',label:'Full precision (FP32)',description:'Largest download.',bytes:325532232,installed:true},
      {id:'q8f16',label:'Low memory (Q8F16)',description:'Lower memory use. Can be slower.',bytes:86033585,installed:false},
    ];
    const chosen=[];
    const props={models,selected:'q8f16',disabled:false,onSelect:id=>chosen.push(id)};
    const html=renderToStaticMarkup(React.createElement(Picker,props));
    assert.match(html,/Installed/);
    assert.match(html,/Download needed/);
    assert.match(html,/86 MB/);
    assert.match(html,/Lower memory use/);
    assert.doesNotMatch(html,/Voice files are shared/);
    const chatterboxHtml=renderToStaticMarkup(React.createElement(Picker,{...props,engine:'Chatterbox',models:[
      {id:'q4f16',label:'Smallest (Q4F16)',description:'Default.',bytes:1508958027,installed:true},
      {id:'fp32',label:'Full precision (FP32)',description:'Largest.',bytes:3284795450,installed:false},
    ],selected:'q4f16'}));
    assert.match(chatterboxHtml,/Chatterbox model size/);
    assert.match(chatterboxHtml,/1.51 GB/);
    assert.match(chatterboxHtml,/3.28 GB/);
    const deleted=[];
    const deleteProps={...props,selected:'fp32',onDelete:()=>deleted.push('fp32')};
    const deleteHtml=renderToStaticMarkup(React.createElement(Picker,deleteProps));
    assert.match(deleteHtml,/Delete selected model/);
    const deleteButton=control(Picker(deleteProps),'button');
    deleteButton.props.onClick();
    assert.deepEqual(deleted,['fp32']);
    assert.equal(control(Picker({...deleteProps,disabled:true}),'button').props.disabled,true);
    assert.doesNotMatch(renderToStaticMarkup(React.createElement(Picker,{...deleteProps,selected:'q8f16'})),/Delete selected model/);
    const partialProps={...deleteProps,selected:'q8f16',models:models.map(model=>({...model,removable:model.id==='q8f16'}))};
    assert.match(renderToStaticMarkup(React.createElement(Picker,partialProps)),/Delete selected model/);
    assert.equal(control(Picker({...deleteProps,deleteDisabled:true}),'button').props.disabled,true);
    const downloads=[];
    const downloadProps={...props,onDownload:()=>downloads.push('q8f16')};
    assert.match(renderToStaticMarkup(React.createElement(Picker,downloadProps)),/Download selected model/);
    const downloadButton=control(Picker(downloadProps),'button');
    downloadButton.props.onClick();
    assert.deepEqual(downloads,['q8f16']);
    assert.equal(control(Picker({...downloadProps,disabled:true}),'button').props.disabled,true);
    assert.doesNotMatch(renderToStaticMarkup(React.createElement(Picker,{...downloadProps,selected:'fp32'})),/Download selected model/);
    assert.match(renderToStaticMarkup(React.createElement(Picker,{...downloadProps,selected:'fp32',needsDownload:true})),/Download selected model/);
    const tree=Picker(props);
    const select=tree.props.children[1];
    assert.equal(select.type,'select');
    assert.equal(select.props.value,'q8f16');
    select.props.onChange({target:{value:'fp32'}});
    assert.deepEqual(chosen,['fp32']);
    const busy=Picker({...props,disabled:true});
    assert.equal(busy.props.children[1].props.disabled,true);
  } finally {await vite.close();}
});
