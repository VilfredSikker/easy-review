import{I as _}from"./InlineThread-DjCW4VLe.js";import{c as s,q as k}from"./fixtures-W2IWC1LY.js";import"./iframe-hlk6mkNV.js";import"./preload-helper-C1FmrZbK.js";import"./attributes-DrKKlg4p.js";import"./app.svelte-BqqxQA-k.js";const I={title:"Diff/InlineThread",component:_,parameters:{layout:"padded",backgrounds:{default:"app"}}},e={args:{thread:s,hunk_idx:0}},r={args:{thread:k,hunk_idx:0}},a={args:{thread:{...s,synced:!0,source:"github"},hunk_idx:0}},n={args:{thread:{...s,stale:!0},hunk_idx:0}};var t,o,d;e.parameters={...e.parameters,docs:{...(t=e.parameters)==null?void 0:t.docs,source:{originalSource:`{
  args: {
    thread: commentThread,
    hunk_idx: 0
  }
}`,...(d=(o=e.parameters)==null?void 0:o.docs)==null?void 0:d.source}}};var c,m,u;r.parameters={...r.parameters,docs:{...(c=r.parameters)==null?void 0:c.docs,source:{originalSource:`{
  args: {
    thread: questionThread,
    hunk_idx: 0
  }
}`,...(u=(m=r.parameters)==null?void 0:m.docs)==null?void 0:u.source}}};var i,p,h;a.parameters={...a.parameters,docs:{...(i=a.parameters)==null?void 0:i.docs,source:{originalSource:`{
  args: {
    thread: {
      ...commentThread,
      synced: true,
      source: "github"
    },
    hunk_idx: 0
  }
}`,...(h=(p=a.parameters)==null?void 0:p.docs)==null?void 0:h.source}}};var g,l,x;n.parameters={...n.parameters,docs:{...(g=n.parameters)==null?void 0:g.docs,source:{originalSource:`{
  args: {
    thread: {
      ...commentThread,
      stale: true
    },
    hunk_idx: 0
  }
}`,...(x=(l=n.parameters)==null?void 0:l.docs)==null?void 0:x.source}}};const C=["Comment","Question","Synced","Stale"];export{e as Comment,r as Question,n as Stale,a as Synced,C as __namedExportsOrder,I as default};
