//! Annotation content script injected into the review browser (native child webview
//! and/or proxied iframe subframes).

/// Injected at document start. Communicates with the host via Tauri IPC when available,
/// otherwise `window.parent.postMessage` (iframe + `erp://` proxy fallback).
pub const FRAME_SCRIPT: &str = r#"(function(){
  if(window.__er_injected)return;
  window.__er_injected=true;

  try{if(window.location.protocol==='tauri:')return;}catch(_){}

  function reportToHost(payload){
    function send(){
      try{
        var inv=window.__TAURI_INTERNALS__&&window.__TAURI_INTERNALS__.invoke;
        if(typeof inv==='function'){
          inv('browser_host_message',{payload:payload}).catch(function(){});
          return true;
        }
      }catch(_){}
      try{
        var t=window.__TAURI__;
        if(t&&t.core&&typeof t.core.invoke==='function'){
          t.core.invoke('browser_host_message',{payload:payload}).catch(function(){});
          return true;
        }
      }catch(_){}
      try{window.parent.postMessage(payload,'*');return true;}catch(_){}
      return false;
    }
    if(send())return;
    var tries=0;
    var timer=setInterval(function(){
      tries+=1;
      if(send()||tries>=40)clearInterval(timer);
    },50);
  }

  function useProxyScheme(){
    try{
      var p=window.location.protocol;
      return p==='erp:'||p==='erps:';
    }catch(_){return false;}
  }

  function cssPath(el){
    if(!el||el.nodeType!==1)return null;
    var path=[],cur=el;
    while(cur&&cur.nodeType===1&&path.length<8){
      var part=cur.nodeName.toLowerCase();
      if(cur.id){part+='#'+CSS.escape(cur.id);path.unshift(part);break;}
      var cls=Array.from(cur.classList).slice(0,2).map(function(c){return'.'+CSS.escape(c);}).join('');
      if(cls)part+=cls;
      var parent=cur.parentElement;
      if(parent){var siblings=Array.from(parent.children).filter(function(c){return c.nodeName===cur.nodeName;});if(siblings.length>1)part+=':nth-of-type('+(siblings.indexOf(cur)+1)+')';}
      path.unshift(part);cur=parent;
    }
    return path.join(' > ');
  }

  function cleanText(value,max){
    try{
      var s=(value||'').replace(/\s+/g,' ').trim();
      return s.length>max?s.slice(0,max-1)+'…':s;
    }catch(_){return null;}
  }

  function interestingAttrs(el){
    var names=['id','class','role','aria-label','aria-describedby','aria-labelledby','title','placeholder','name','type','value','href','src','alt','data-testid','data-test','data-cy'];
    var out={};
    for(var i=0;i<names.length;i++){
      try{
        var v=el.getAttribute(names[i]);
        if(v!==null&&v!=='')out[names[i]]=cleanText(v,240);
      }catch(_){}
    }
    return out;
  }

  function shortNode(el){
    if(!el||el.nodeType!==1)return null;
    var attrs=interestingAttrs(el);
    return{
      tag:el.tagName?el.tagName.toLowerCase():null,
      id:el.id||null,
      classes:Array.from(el.classList||[]).slice(0,8),
      role:el.getAttribute('role')||null,
      aria_label:el.getAttribute('aria-label')||null,
      text:cleanText(el.innerText||el.textContent||'',180),
      attrs:attrs
    };
  }

  function parentChain(el){
    var chain=[],cur=el&&el.parentElement;
    while(cur&&cur.nodeType===1&&chain.length<5){
      chain.push(shortNode(cur));
      cur=cur.parentElement;
    }
    return chain;
  }

  function elementContext(el){
    try{
      var tag=el.tagName?el.tagName.toLowerCase():'unknown';
      var label=el.getAttribute('aria-label')||el.getAttribute('title')||el.getAttribute('placeholder')||cleanText(el.innerText||el.textContent||'',80);
      return label?tag+': '+label:tag;
    }catch(_){return null;}
  }

  function domContext(el,selector,rect){
    try{
      var parent=el.parentElement;
      return{
        selector:selector||null,
        summary:elementContext(el),
        node:shortNode(el),
        rect:rect||null,
        parent_chain:parentChain(el),
        nearby_text:cleanText(parent?(parent.innerText||parent.textContent||''):'',500),
        outer_html:cleanText(el.outerHTML||'',1200)
      };
    }catch(_){return null;}
  }

  function reportLocation(){
    reportToHost({__er_location:true,href:window.location.href});
  }
  function reportReady(){
    reportToHost({__er_ready:true,href:window.location.href});
  }
  function proxyUrl(raw){
    if(!useProxyScheme())return raw;
    try{
      var u=new URL(raw,window.location.href);
      if(u.protocol==='http:')return 'erp://'+u.host+u.pathname+u.search+u.hash;
      if(u.protocol==='https:')return 'erps://'+u.host+u.pathname+u.search+u.hash;
    }catch(_){}
    return raw;
  }
  reportLocation();
  reportReady();
  if(document.readyState==='loading'){
    document.addEventListener('DOMContentLoaded',reportReady,{once:true});
  }
  window.addEventListener('load',reportReady,{once:true});
  window.addEventListener('popstate',reportLocation);
  window.addEventListener('hashchange',reportLocation);
  document.addEventListener('click',function(ev){
    if(ev.defaultPrevented||ev.button!==0||ev.metaKey||ev.ctrlKey||ev.shiftKey||ev.altKey)return;
    var a=ev.target&&ev.target.closest?ev.target.closest('a[href]'):null;
    if(!a)return;
    var target=(a.getAttribute('target')||'').toLowerCase();
    if(target&&target!=='_self')return;
    var next=proxyUrl(a.href);
    if(next!==a.href){ev.preventDefault();window.location.href=next;}
  },true);
  document.addEventListener('submit',function(ev){
    if(ev.defaultPrevented)return;
    var form=ev.target;
    if(!form||!form.action)return;
    var target=(form.getAttribute('target')||'').toLowerCase();
    if(target&&target!=='_self')return;
    var method=(form.getAttribute('method')||'get').toLowerCase();
    if(method!=='get')return;
    var next=proxyUrl(form.action);
    if(next===form.action)return;
    ev.preventDefault();
    try{
      var params=new URLSearchParams(new FormData(form));
      var sep=next.indexOf('?')>=0?'&':'?';
      window.location.href=params.toString()?next+sep+params.toString():next;
    }catch(_){window.location.href=next;}
  },true);

  function pierceShadowAtPoint(el,x,y){
    var cur=el;
    while(cur&&cur.shadowRoot){
      var inner=cur.shadowRoot.elementFromPoint(x,y);
      if(!inner||inner===cur)break;
      cur=inner;
    }
    return cur;
  }

  function deepElementFromPoint(doc,x,y,ox,oy){
    var el=doc.elementFromPoint(x,y);
    if(!el)return null;
    el=pierceShadowAtPoint(el,x,y);
    if(el.tagName==='IFRAME'){
      try{
        var fc=el.contentDocument;
        if(fc&&fc.documentElement){
          var fr=el.getBoundingClientRect();
          var result=deepElementFromPoint(fc,x-fr.left,y-fr.top,ox+fr.left,oy+fr.top);
          if(result)return result;
        }
      }catch(_){}
    }
    if(el===doc.documentElement||el===doc.body)return null;
    return{el:el,ox:ox,oy:oy};
  }

  function deepQuerySelector(doc,sel,ox,oy){
    try{
      var el=doc.querySelector(sel);
      if(el)return{el:el,ox:ox,oy:oy};
    }catch(_){}
    var frames=doc.querySelectorAll('iframe');
    for(var fi=0;fi<frames.length;fi++){
      try{
        var fc=frames[fi].contentDocument;
        if(!fc)continue;
        var fr=frames[fi].getBoundingClientRect();
        var result=deepQuerySelector(fc,sel,ox+fr.left,oy+fr.top);
        if(result)return result;
      }catch(_){}
    }
    return null;
  }

  function hoverPayloadFromPoint(x,y){
    try{
      var hit=deepElementFromPoint(document,x,y,0,0);
      if(!hit)return{__er_hover_result:true,selector:null,rect:null,element_context:null,dom_context:null};
      var r=hit.el.getBoundingClientRect();
      var selector=cssPath(hit.el);
      var rect={left:hit.ox+r.left,top:hit.oy+r.top,width:r.width,height:r.height};
      return{__er_hover_result:true,selector:selector,rect:rect,element_context:elementContext(hit.el),dom_context:domContext(hit.el,selector,rect)};
    }catch(_){return{__er_hover_result:true,selector:null,rect:null,element_context:null,dom_context:null};}
  }

  function annotatePayloadFromPoint(x,y){
    try{
      var hit=deepElementFromPoint(document,x,y,0,0);
      if(!hit)return{__er_annotate:true,x:x,y:y,w:24,h:24,selector:null,element_context:null,dom_context:null};
      var r=hit.el.getBoundingClientRect();
      var selector=cssPath(hit.el);
      var rect={left:hit.ox+r.left,top:hit.oy+r.top,width:r.width,height:r.height};
      return{__er_annotate:true,x:rect.left,y:rect.top,w:rect.width,h:rect.height,selector:selector,element_context:elementContext(hit.el),dom_context:domContext(hit.el,selector,rect)};
    }catch(_){return{__er_annotate:true,x:x,y:y,w:24,h:24,selector:null,element_context:null,dom_context:null};}
  }

  var annotateActive=false;
  var annotateHoverRaf=0;
  var hoverBoxEl=null;
  function ensureHoverBox(){
    if(hoverBoxEl&&hoverBoxEl.isConnected)return hoverBoxEl;
    hoverBoxEl=document.createElement('div');
    hoverBoxEl.id='__er_hover_box';
    hoverBoxEl.setAttribute('style','position:fixed;pointer-events:none;z-index:2147483646;display:none;border:2px solid rgba(99,179,237,0.9);background:rgba(99,179,237,0.08);outline:1px solid rgba(255,255,255,0.15);border-radius:2px;box-sizing:border-box;');
    (document.documentElement||document.body).appendChild(hoverBoxEl);
    return hoverBoxEl;
  }
  function updateHoverBox(rect){
    var box=ensureHoverBox();
    if(!rect||rect.width<1||rect.height<1){box.style.display='none';return;}
    box.style.display='block';
    box.style.left=rect.left+'px';
    box.style.top=rect.top+'px';
    box.style.width=rect.width+'px';
    box.style.height=rect.height+'px';
  }
  function hideHoverBox(){
    if(hoverBoxEl)hoverBoxEl.style.display='none';
  }

  function onAnnotateMove(ev){
    if(!annotateActive)return;
    if(annotateHoverRaf)return;
    annotateHoverRaf=requestAnimationFrame(function(){
      annotateHoverRaf=0;
      if(!annotateActive)return;
      var payload=hoverPayloadFromPoint(ev.clientX,ev.clientY);
      updateHoverBox(payload.rect);
      reportToHost(payload);
    });
  }
  function onAnnotateClick(ev){
    if(!annotateActive)return;
    if(ev.defaultPrevented||ev.button!==0||ev.metaKey||ev.ctrlKey||ev.shiftKey||ev.altKey)return;
    ev.preventDefault();
    ev.stopPropagation();
    reportToHost(annotatePayloadFromPoint(ev.clientX,ev.clientY));
  }
  function applyAnnotateActive(on){
    if(on===annotateActive)return;
    annotateActive=!!on;
    if(annotateActive){
      try{document.documentElement.style.cursor='crosshair';}catch(_){}
      document.addEventListener('pointermove',onAnnotateMove,true);
      document.addEventListener('mousemove',onAnnotateMove,true);
      document.addEventListener('click',onAnnotateClick,true);
    }else{
      try{document.documentElement.style.cursor='';}catch(_){}
      hideHoverBox();
      document.removeEventListener('pointermove',onAnnotateMove,true);
      document.removeEventListener('mousemove',onAnnotateMove,true);
      document.removeEventListener('click',onAnnotateClick,true);
    }
    reportToHost({__er_annotate_mode_ack:true,active:annotateActive});
  }
  function setAnnotateActive(on){
    var want=!!on;
    function run(){applyAnnotateActive(want);}
    if(document.readyState==='loading'){
      document.addEventListener('DOMContentLoaded',run,{once:true});
      window.addEventListener('load',run,{once:true});
      return;
    }
    run();
  }

  function handleHostMessage(d){
    if(!d||typeof d!=='object')return;
    if(d.__er_set_annotate_mode===true||d.__er_set_annotate_mode===false){
      setAnnotateActive(!!d.__er_set_annotate_mode);
      return;
    }
    if(d.__er_hover===true){
      reportToHost(hoverPayloadFromPoint(d.x,d.y));
      return;
    }
    if(d.__er_query_rect===true){
      try{
        if(!d.selector){reportToHost({__er_query_rect_result:true,id:d.id,rect:null});return;}
        var hit2=deepQuerySelector(document,d.selector,0,0);
        if(!hit2){reportToHost({__er_query_rect_result:true,id:d.id,rect:null});return;}
        var r2=hit2.el.getBoundingClientRect();
        reportToHost({__er_query_rect_result:true,id:d.id,rect:{left:hit2.ox+r2.left,top:hit2.oy+r2.top,width:r2.width,height:r2.height}});
      }catch(_){reportToHost({__er_query_rect_result:true,id:d.id,rect:null});}
      return;
    }
    if(d.__er_reanchor===true){
      var items=Array.isArray(d.items)?d.items:[];
      var results=items.map(function(item){
        try{
          if(!item.selector)return{id:item.id,fresh:false};
          var hit3=deepQuerySelector(document,item.selector,0,0);
          if(!hit3)return{id:item.id,fresh:false};
          var r3=hit3.el.getBoundingClientRect();
          var nb=[hit3.ox+r3.left,hit3.oy+r3.top,r3.width,r3.height];
          var ob=item.box||[0,0,0,0];
          var ow=ob[2]||1,oh=ob[3]||1;
          var fresh=Math.abs(nb[2]-ob[2])/ow<=0.1&&Math.abs(nb[3]-ob[3])/oh<=0.1&&Math.abs(nb[0]-ob[0])<=20&&Math.abs(nb[1]-ob[1])<=20;
          return{id:item.id,fresh:fresh,new_box:nb};
        }catch(_){return{id:item.id,fresh:false};}
      });
      reportToHost({__er_reanchor_result:true,results:results});
    }
  }

  window.__er_setAnnotateMode=setAnnotateActive;
  window.__er_receiveHostMessage=handleHostMessage;
  window.addEventListener('message',function(ev){
    handleHostMessage(ev.data);
  });
})();"#;

pub const BROWSER_MESSAGE_EVENT: &str = "browser://message";
