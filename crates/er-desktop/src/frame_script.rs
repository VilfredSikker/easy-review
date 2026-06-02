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

  // Forward host-level keyboard shortcuts (e.g. ⌘A for the AI palette) that
  // Desktop shortcuts while focus is in this native child webview (not parent UI).
  document.addEventListener('keydown',function(ev){
    if(!(ev.metaKey||ev.ctrlKey)||ev.altKey)return;
    var k=ev.key;
    if(ev.shiftKey&&(k==='b'||k==='B')){
      ev.preventDefault();ev.stopPropagation();
      reportToHost({__er_shortcut:'browser-fullscreen'});
      return;
    }
    if(ev.shiftKey&&(k==='e'||k==='E')){
      ev.preventDefault();ev.stopPropagation();
      reportToHost({__er_shortcut:'export-review'});
      return;
    }
    if(!ev.shiftKey&&(k==='b'||k==='B')){
      ev.preventDefault();ev.stopPropagation();
      reportToHost({__er_shortcut:'browser-cycle'});
      return;
    }
    if(!ev.shiftKey&&(k==='a'||k==='A')){
      ev.preventDefault();ev.stopPropagation();
      reportToHost({__er_shortcut:'ai-hub'});
    }
  },true);

  // Escape → host (close AI Hub / modals). Skip when the in-page composer is open.
  document.addEventListener('keydown',function(ev){
    if(ev.key!=='Escape')return;
    if(composerEl)return;
    ev.preventDefault();
    ev.stopPropagation();
    reportToHost({__er_shortcut:'dismiss-overlay'});
  },true);

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
  (function(){
    var _push=history.pushState;
    history.pushState=function(){_push.apply(this,arguments);reportLocation();};
    var _replace=history.replaceState;
    history.replaceState=function(){_replace.apply(this,arguments);reportLocation();};
  })();
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

  function isErOverlayTarget(el){
    if(!el||!el.closest)return false;
    if(el.closest('#__er_pins_layer,#__er_hover_box,[data-er-overlay]'))return true;
    if(composerEl&&composerEl.contains(el))return true;
    if(popoverEl&&popoverEl.contains(el))return true;
    return false;
  }
  function shouldIgnoreAnnotateEvent(ev){
    if(!annotateActive)return true;
    if(isErOverlayTarget(ev.target))return true;
    return false;
  }
  function onAnnotatePointerDown(ev){
    if(shouldIgnoreAnnotateEvent(ev))return;
    if(ev.defaultPrevented||ev.button!==0||ev.metaKey||ev.ctrlKey||ev.shiftKey||ev.altKey)return;
    ev.preventDefault();
    ev.stopPropagation();
    ev.stopImmediatePropagation();
  }
  function onAnnotateMove(ev){
    if(!annotateActive||composerEl)return;
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
    if(shouldIgnoreAnnotateEvent(ev))return;
    if(ev.defaultPrevented||ev.button!==0||ev.metaKey||ev.ctrlKey||ev.shiftKey||ev.altKey)return;
    ev.preventDefault();
    ev.stopPropagation();
    ev.stopImmediatePropagation();
    reportToHost(annotatePayloadFromPoint(ev.clientX,ev.clientY));
  }
  function applyAnnotateActive(on){
    if(on===annotateActive)return;
    annotateActive=!!on;
    if(annotateActive){
      try{document.documentElement.style.cursor='crosshair';}catch(_){}
      document.addEventListener('pointerdown',onAnnotatePointerDown,true);
      document.addEventListener('mousedown',onAnnotatePointerDown,true);
      document.addEventListener('pointermove',onAnnotateMove,true);
      document.addEventListener('mousemove',onAnnotateMove,true);
      document.addEventListener('click',onAnnotateClick,true);
    }else{
      try{document.documentElement.style.cursor='';}catch(_){}
      hideHoverBox();
      hideComposer(true);
      document.removeEventListener('pointerdown',onAnnotatePointerDown,true);
      document.removeEventListener('mousedown',onAnnotatePointerDown,true);
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

  var pinsLayerEl=null;
  var popoverTimer=0;
  var popoverEl=null;
  var pinsCache=[];
  var pinsRelayoutRaf=0;
  var pinsListenersBound=false;

  function scaleBox(box,viewport){
    var bx=box[0]||0,by=box[1]||0,bw=box[2]||0,bh=box[3]||0;
    var vw=viewport&&viewport[0]>0?viewport[0]:(window.innerWidth||1);
    var vh=viewport&&viewport[1]>0?viewport[1]:(window.innerHeight||1);
    var sx=(window.innerWidth||1)/vw;
    var sy=(window.innerHeight||1)/vh;
    return{left:bx*sx,top:by*sy,width:bw*sx,height:bh*sy,cx:bx*sx+bw*sx/2,cy:by*sy+bh*sy/2};
  }

  function ensurePinsLayer(){
    if(pinsLayerEl&&pinsLayerEl.isConnected)return pinsLayerEl;
    pinsLayerEl=document.createElement('div');
    pinsLayerEl.id='__er_pins_layer';
    pinsLayerEl.setAttribute('style','position:fixed;inset:0;pointer-events:none;overflow:visible;z-index:2147483645;');
    (document.documentElement||document.body).appendChild(pinsLayerEl);
    return pinsLayerEl;
  }

  function layoutPinItem(item){
    if(item.selector){
      try{
        var hit=deepQuerySelector(document,item.selector,0,0);
        if(hit){
          var r=hit.el.getBoundingClientRect();
          if(r.width>0||r.height>0){
            return{
              left:hit.ox+r.left,
              top:hit.oy+r.top,
              width:r.width,
              height:r.height,
              cx:hit.ox+r.left+r.width/2,
              cy:hit.oy+r.top+r.height/2
            };
          }
        }
      }catch(_){}
    }
    var box=item.box||[0,0,24,24];
    var vp=item.viewport||[window.innerWidth,window.innerHeight];
    var s=scaleBox(box,vp);
    if(s.width<1&&s.height<1){s.width=24;s.height=24;}
    return s;
  }

  function bindPinsRelayoutListeners(){
    if(pinsListenersBound)return;
    pinsListenersBound=true;
    window.addEventListener('scroll',schedulePinsRelayout,true);
    window.addEventListener('resize',schedulePinsRelayout);
  }

  function unbindPinsRelayoutListeners(){
    if(!pinsListenersBound)return;
    pinsListenersBound=false;
    window.removeEventListener('scroll',schedulePinsRelayout,true);
    window.removeEventListener('resize',schedulePinsRelayout);
  }

  function schedulePinsRelayout(){
    if(pinsRelayoutRaf)return;
    pinsRelayoutRaf=requestAnimationFrame(function(){
      pinsRelayoutRaf=0;
      renderPins();
    });
  }

  var COMPOSER_EST_W=272;
  var COMPOSER_EST_H=210;
  var POPOVER_EST_W=260;
  var POPOVER_EST_H=120;
  var OVERLAY_MARGIN=8;
  var OVERLAY_GAP=8;

  function viewportSize(){
    return{
      w:window.innerWidth||document.documentElement.clientWidth||800,
      h:window.innerHeight||document.documentElement.clientHeight||600
    };
  }

  /** Keep floating panels inside the review webview; flip left/up when near edges. */
  function clampOverlayPosition(s,el,estW,estH){
    var vp=viewportSize();
    var m=OVERLAY_MARGIN;
    var gap=OVERLAY_GAP;
    var w=el&&el.offsetWidth>0?el.offsetWidth:estW;
    var h=el&&el.offsetHeight>0?el.offsetHeight:estH;
    var maxW=Math.max(160,vp.w-m*2);
    if(el&&w>maxW){
      el.style.maxWidth=maxW+'px';
      w=el.offsetWidth>0?el.offsetWidth:maxW;
    }
    var rightLeft=s.left+s.width+gap;
    var leftLeft=s.left-w-gap;
    var left;
    if(rightLeft+w+m<=vp.w)left=rightLeft;
    else if(leftLeft>=m)left=leftLeft;
    else left=Math.max(m,Math.min(rightLeft,vp.w-w-m));
    left=Math.max(m,Math.min(left,vp.w-w-m));
    var belowTop=s.top+s.height+gap;
    var aboveTop=s.top-h-gap;
    var top;
    if(belowTop+h+m<=vp.h)top=belowTop;
    else if(aboveTop>=m)top=aboveTop;
    else top=Math.max(m,Math.min(belowTop,vp.h-h-m));
    top=Math.max(m,Math.min(top,vp.h-h-m));
    return{left:left,top:top};
  }

  function applyOverlayPosition(el,pos){
    el.style.left=pos.left+'px';
    el.style.top=pos.top+'px';
  }

  function positionComposerEl(){
    if(!composerEl||!composerState)return;
    var s=layoutPinItem({box:composerState.box,viewport:composerState.viewport,selector:composerState.selector});
    applyOverlayPosition(composerEl,clampOverlayPosition(s,composerEl,COMPOSER_EST_W,COMPOSER_EST_H));
  }

  function renderPins(){
    var layer=ensurePinsLayer();
    layer.innerHTML='';
    if(!pinsCache.length){
      if(!composerEl)hideComposer(true);
      unbindPinsRelayoutListeners();
      return;
    }
    bindPinsRelayoutListeners();
    for(var i=0;i<pinsCache.length;i++){
      var item=pinsCache[i];
      var s=layoutPinItem(item);
      var bbox=document.createElement('div');
      bbox.setAttribute('style','position:absolute;left:'+s.left+'px;top:'+s.top+'px;width:'+s.width+'px;height:'+s.height+'px;border:1.5px dashed rgba(249,115,22,0.6);background:rgba(249,115,22,0.05);border-radius:2px;box-sizing:border-box;');
      layer.appendChild(bbox);
      var pinWrap=document.createElement('div');
      pinWrap.setAttribute('style','position:absolute;left:'+s.cx+'px;top:'+s.cy+'px;transform:translate(-50%,-50%);pointer-events:auto;');
      var btn=document.createElement('button');
      btn.type='button';
      btn.textContent=String(item.index!=null?item.index:i+1);
      btn.title=(item.text||'').slice(0,500);
      var stale=!!item.stale;
      btn.setAttribute('style','width:24px;height:24px;border-radius:9999px;font-size:12px;font-weight:bold;display:flex;align-items:center;justify-content:center;cursor:default;box-shadow:0 1px 4px rgba(0,0,0,0.3);'+(stale?'background:transparent;color:rgb(252,211,77);border:2px dashed rgb(251,191,36);':'background:rgb(249,115,22);color:white;border:2px solid rgba(255,255,255,0.8);'));
      pinWrap.appendChild(btn);
      layer.appendChild(pinWrap);
      if(item.showTip){
        var tip=document.createElement('div');
        tip.setAttribute('style','position:absolute;left:'+(s.cx+14)+'px;top:'+(s.cy-8)+'px;max-width:16rem;padding:6px 8px;background:rgba(0,0,0,0.85);color:white;border:1px solid rgba(255,255,255,0.2);border-radius:6px;font-size:12px;line-height:1.35;pointer-events:none;box-shadow:0 4px 12px rgba(0,0,0,0.25);');
        var titleEl=document.createElement('div');
        titleEl.setAttribute('style','font-size:10px;color:rgb(254,215,170);font-family:ui-monospace,monospace;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:14rem;');
        titleEl.textContent=item.label||item.text||'Annotation';
        tip.appendChild(titleEl);
        if(item.text){
          var bodyEl=document.createElement('div');
          bodyEl.setAttribute('style','margin-top:2px;white-space:pre-wrap;display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden;');
          bodyEl.textContent=item.text;
          tip.appendChild(bodyEl);
        }
        layer.appendChild(tip);
      }
    }
    if(popoverEl)layer.appendChild(popoverEl);
    if(composerEl){
      layer.appendChild(composerEl);
      positionComposerEl();
    }
  }

  function clearPinsLayer(){
    hidePopover();
    hideComposer(true);
    pinsCache=[];
    unbindPinsRelayoutListeners();
    if(pinsLayerEl)pinsLayerEl.innerHTML='';
  }

  function syncPinsToPage(items){
    hidePopover();
    pinsCache=Array.isArray(items)?items:[];
    renderPins();
  }

  function hidePopover(){
    if(popoverTimer){clearTimeout(popoverTimer);popoverTimer=0;}
    if(popoverEl&&popoverEl.parentNode)popoverEl.parentNode.removeChild(popoverEl);
    popoverEl=null;
  }

  function showPopover(opts){
    hidePopover();
    var s=layoutPinItem({box:opts.box||[0,0,24,24],viewport:opts.viewport,selector:opts.selector});
    var layer=ensurePinsLayer();
    popoverEl=document.createElement('div');
    popoverEl.setAttribute('style','position:absolute;left:0;top:0;width:16rem;box-sizing:border-box;padding:8px 10px;background:rgba(0,0,0,0.92);color:white;border:1px solid rgba(255,255,255,0.25);border-radius:8px;font-size:12px;line-height:1.4;pointer-events:none;z-index:2147483647;box-shadow:0 8px 24px rgba(0,0,0,0.35);');
    var label=opts.element_context||opts.label;
    if(label){
      var h=document.createElement('div');
      h.setAttribute('style','font-size:10px;color:rgb(254,215,170);font-family:ui-monospace,monospace;margin-bottom:4px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;');
      h.textContent=label;
      popoverEl.appendChild(h);
    }
    var p=document.createElement('div');
    p.setAttribute('style','white-space:pre-wrap;');
    p.textContent=opts.text||'';
    popoverEl.appendChild(p);
    layer.appendChild(popoverEl);
    applyOverlayPosition(popoverEl,clampOverlayPosition(s,popoverEl,POPOVER_EST_W,POPOVER_EST_H));
    popoverTimer=setTimeout(hidePopover,8000);
  }

  var composerEl=null;
  var composerState=null;
  var composerViewportListenersBound=false;

  function bindComposerViewportListeners(){
    if(composerViewportListenersBound)return;
    composerViewportListenersBound=true;
    function relayoutComposer(){
      if(composerEl)positionComposerEl();
    }
    window.addEventListener('resize',relayoutComposer);
    window.addEventListener('scroll',relayoutComposer,true);
  }

  function hideComposer(notifyHost){
    var had=!!composerEl;
    if(composerEl&&composerEl.parentNode)composerEl.parentNode.removeChild(composerEl);
    composerEl=null;
    composerState=null;
    if(notifyHost&&had)reportToHost({__er_composer_cancel:true});
  }

  function submitComposer(){
    if(!composerState||!composerEl)return;
    var ta=composerEl.querySelector('textarea');
    var text=ta?ta.value.trim():'';
    if(!text){hideComposer(true);return;}
    reportToHost({
      __er_composer_submit:true,
      box:composerState.box,
      viewport:composerState.viewport,
      selector:composerState.selector||null,
      element_context:composerState.element_context||null,
      dom_context:composerState.dom_context||null,
      text:text
    });
    hideComposer();
  }

  function showComposer(opts){
    hideComposer();
    hidePopover();
    var s=layoutPinItem({box:opts.box||[0,0,24,24],viewport:opts.viewport,selector:opts.selector});
    composerState={
      box:opts.box||[0,0,24,24],
      viewport:opts.viewport||[window.innerWidth,window.innerHeight],
      selector:opts.selector||null,
      element_context:opts.element_context||opts.label||null,
      dom_context:opts.dom_context||null
    };
    var layer=ensurePinsLayer();
    composerEl=document.createElement('div');
    composerEl.setAttribute('data-er-overlay','composer');
    composerEl.setAttribute('style','position:absolute;left:0;top:0;width:16rem;box-sizing:border-box;padding:8px 10px;background:rgba(15,15,20,0.96);color:white;border:1px solid rgba(255,255,255,0.3);border-radius:8px;font-size:12px;line-height:1.4;pointer-events:auto;z-index:2147483647;box-shadow:0 8px 24px rgba(0,0,0,0.45);font-family:system-ui,sans-serif;');
    var label=opts.element_context||opts.label;
    if(label){
      var h=document.createElement('div');
      h.setAttribute('style','font-size:10px;color:rgb(254,215,170);font-family:ui-monospace,monospace;margin-bottom:6px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;');
      h.textContent=label;
      composerEl.appendChild(h);
    }
    var ta=document.createElement('textarea');
    ta.setAttribute('style','width:100%;min-height:4.5rem;resize:vertical;background:rgba(0,0,0,0.35);color:white;border:1px solid rgba(255,255,255,0.2);border-radius:4px;padding:6px;font-size:12px;outline:none;box-sizing:border-box;');
    ta.placeholder="What's wrong here?";
    ta.addEventListener('keydown',function(ev){
      if(ev.key==='Escape'){ev.preventDefault();ev.stopPropagation();hideComposer(true);return;}
      if(ev.key==='Enter'&&(ev.metaKey||ev.ctrlKey)){ev.preventDefault();ev.stopPropagation();submitComposer();}
    });
    composerEl.appendChild(ta);
    var row=document.createElement('div');
    row.setAttribute('style','display:flex;justify-content:flex-end;gap:8px;margin-top:8px;');
    var cancelBtn=document.createElement('button');
    cancelBtn.type='button';
    cancelBtn.textContent='Cancel';
    cancelBtn.setAttribute('style','font-size:11px;padding:4px 10px;border-radius:4px;border:none;background:rgba(255,255,255,0.1);color:white;cursor:pointer;');
    cancelBtn.addEventListener('click',function(ev){
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      hideComposer(true);
    });
    var saveBtn=document.createElement('button');
    saveBtn.type='button';
    saveBtn.textContent='Save';
    saveBtn.setAttribute('style','font-size:11px;padding:4px 10px;border-radius:4px;border:none;background:rgb(59,130,246);color:white;cursor:pointer;font-weight:600;');
    saveBtn.addEventListener('click',function(ev){
      ev.preventDefault();
      ev.stopPropagation();
      ev.stopImmediatePropagation();
      submitComposer();
    });
    row.appendChild(cancelBtn);
    row.appendChild(saveBtn);
    composerEl.appendChild(row);
    var hint=document.createElement('div');
    hint.setAttribute('style','font-size:10px;color:rgba(255,255,255,0.45);margin-top:6px;text-align:right;');
    hint.textContent='⌘↩ save · esc cancel';
    composerEl.appendChild(hint);
    layer.appendChild(composerEl);
    applyOverlayPosition(composerEl,clampOverlayPosition(s,composerEl,COMPOSER_EST_W,COMPOSER_EST_H));
    bindComposerViewportListeners();
    setTimeout(function(){try{ta.focus();}catch(_){}},0);
  }

  function handleHostMessage(d){
    if(!d||typeof d!=='object')return;
    if(d.__er_set_annotate_mode===true||d.__er_set_annotate_mode===false){
      setAnnotateActive(!!d.__er_set_annotate_mode);
      return;
    }
    if(d.__er_sync_pins===true){
      syncPinsToPage(Array.isArray(d.items)?d.items:[]);
      return;
    }
    if(d.__er_show_popover===true){
      showPopover(d);
      return;
    }
    if(d.__er_clear_pins===true){
      clearPinsLayer();
      return;
    }
    if(d.__er_show_composer===true){
      showComposer(d);
      return;
    }
    if(d.__er_hide_composer===true){
      hideComposer();
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
      return;
    }
  }

  window.__er_setAnnotateMode=setAnnotateActive;
  window.__er_receiveHostMessage=handleHostMessage;
  window.addEventListener('message',function(ev){
    handleHostMessage(ev.data);
  });
})();"#;

pub const BROWSER_MESSAGE_EVENT: &str = "browser://message";
