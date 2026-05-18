import{B as m}from"./BranchCard-ulReiTKb.js";import{p as l}from"./fixtures-W2IWC1LY.js";import"./iframe-hlk6mkNV.js";import"./preload-helper-C1FmrZbK.js";import"./SectionLabel-BHP48sks.js";const w={title:"RightPanel/BranchCard",component:m,parameters:{layout:"padded",backgrounds:{default:"rail"}}},e={args:{branch:"show-experiment-params",base:"main",pr:l,reviewed_count:2,total_count:5,additions:1927,deletions:10,checks_status:"success"}},a={args:{branch:"feat/cleanup",base:"main",pr:null,reviewed_count:0,total_count:3,additions:42,deletions:7,checks_status:null}},n={args:{branch:"wip/refactor",base:"main",pr:{number:99,title:"WIP refactor",state:"draft",base:"main",head:"wip/refactor"},reviewed_count:1,total_count:4,additions:100,deletions:50,checks_status:"pending"}};var r,t,s;e.parameters={...e.parameters,docs:{...(r=e.parameters)==null?void 0:r.docs,source:{originalSource:`{
  args: {
    branch: "show-experiment-params",
    base: "main",
    pr: prDraft,
    reviewed_count: 2,
    total_count: 5,
    additions: 1927,
    deletions: 10,
    checks_status: "success"
  }
}`,...(s=(t=e.parameters)==null?void 0:t.docs)==null?void 0:s.source}}};var o,c,i;a.parameters={...a.parameters,docs:{...(o=a.parameters)==null?void 0:o.docs,source:{originalSource:`{
  args: {
    branch: "feat/cleanup",
    base: "main",
    pr: null,
    reviewed_count: 0,
    total_count: 3,
    additions: 42,
    deletions: 7,
    checks_status: null
  }
}`,...(i=(c=a.parameters)==null?void 0:c.docs)==null?void 0:i.source}}};var d,p,u;n.parameters={...n.parameters,docs:{...(d=n.parameters)==null?void 0:d.docs,source:{originalSource:`{
  args: {
    branch: "wip/refactor",
    base: "main",
    pr: {
      number: 99,
      title: "WIP refactor",
      state: "draft",
      base: "main",
      head: "wip/refactor"
    },
    reviewed_count: 1,
    total_count: 4,
    additions: 100,
    deletions: 50,
    checks_status: "pending"
  }
}`,...(u=(p=n.parameters)==null?void 0:p.docs)==null?void 0:u.source}}};const k=["WithPr","NoPr","ChecksPending"];export{n as ChecksPending,a as NoPr,e as WithPr,k as __namedExportsOrder,w as default};
