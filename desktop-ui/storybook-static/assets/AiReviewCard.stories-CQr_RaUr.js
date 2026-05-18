import{A as l}from"./AiReviewCard-BZVMbiOv.js";import{a as g,b as u}from"./fixtures-W2IWC1LY.js";import"./iframe-hlk6mkNV.js";import"./preload-helper-C1FmrZbK.js";import"./attributes-DrKKlg4p.js";import"./SectionLabel-BHP48sks.js";const E={title:"RightPanel/AiReviewCard",component:l,parameters:{layout:"padded",backgrounds:{default:"rail"}}},a={args:{ai:g}},r={args:{ai:{...g,fresh:!1}}},e={args:{ai:u}};var s,i,o;a.parameters={...a.parameters,docs:{...(s=a.parameters)==null?void 0:s.docs,source:{originalSource:`{
  args: {
    ai: aiWithFindings
  }
}`,...(o=(i=a.parameters)==null?void 0:i.docs)==null?void 0:o.source}}};var t,n,m;r.parameters={...r.parameters,docs:{...(t=r.parameters)==null?void 0:t.docs,source:{originalSource:`{
  args: {
    ai: {
      ...aiWithFindings,
      fresh: false
    }
  }
}`,...(m=(n=r.parameters)==null?void 0:n.docs)==null?void 0:m.source}}};var c,p,d;e.parameters={...e.parameters,docs:{...(c=e.parameters)==null?void 0:c.docs,source:{originalSource:`{
  args: {
    ai: aiEmpty
  }
}`,...(d=(p=e.parameters)==null?void 0:p.docs)==null?void 0:d.source}}};const R=["Fresh","Stale","NoFindings"];export{a as Fresh,e as NoFindings,r as Stale,R as __namedExportsOrder,E as default};
