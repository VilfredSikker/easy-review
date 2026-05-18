#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod er_storage;
mod export;
mod pr_cache;
mod projects;
mod snapshot;
mod tabs;
mod terminal;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use commands::AppState;
use er_engine::app::App;
use er_engine::highlight::Highlighter;
use snapshot::{
    GithubStatusSnapshot, LoadingFlags, LoadingState, PrInfo, ProjectMeta, WatchStatusSnapshot,
    WatchStatusState,
};

/// Annotation content script injected into browser-view frames.
/// Handles hover queries, click reporting, re-anchoring, live rect queries,
/// and location change reporting back to the parent.
const FRAME_SCRIPT: &str = r#"(function(){
  if(window.__er_injected)return;
  window.__er_injected=true;

  // Bail in the Tauri main frame (tauri: protocol) — we only annotate child frames.
  // Using protocol check instead of window===window.top because WKWebView may not
  // correctly set up window.top for custom-scheme iframes.
  try{if(window.location.protocol==='tauri:')return;}catch(_){}

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

  // Report current page location to parent (so URL bar stays in sync when navigating
  // inside the proxied iframe).
  function reportLocation(){
    try{window.parent.postMessage({__er_location:true,href:window.location.href},'*');}catch(_){}
  }
  function reportReady(){
    try{window.parent.postMessage({__er_ready:true,href:window.location.href},'*');}catch(_){}
  }
  function proxyUrl(raw){
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

  window.addEventListener('message',function(ev){
    var d=ev.data;if(!d)return;

    // Walk into nested iframes: given (x,y) in the current document, return
    // {el, offsetLeft, offsetTop} where offsetLeft/Top is the cumulative iframe
    // position so rects can be translated back to the top-level viewport.
    function deepElementFromPoint(doc,x,y,ox,oy){
      var el=doc.elementFromPoint(x,y);
      if(!el)return null;
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

    // Find a selector in the current document or any nested iframe.
    // Returns {el, offsetLeft, offsetTop} or null.
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

    // Hover query: parent asks which element is under (x,y).
    if(d.__er_hover===true){
      try{
        var hit=deepElementFromPoint(document,d.x,d.y,0,0);
        if(!hit){window.parent.postMessage({__er_hover_result:true,selector:null,rect:null,element_context:null,dom_context:null},'*');return;}
        var r=hit.el.getBoundingClientRect();
        var selector=cssPath(hit.el);
        var rect={left:hit.ox+r.left,top:hit.oy+r.top,width:r.width,height:r.height};
        window.parent.postMessage({__er_hover_result:true,selector:selector,rect:rect,element_context:elementContext(hit.el),dom_context:domContext(hit.el,selector,rect)},'*');
      }catch(_){window.parent.postMessage({__er_hover_result:true,selector:null,rect:null,element_context:null,dom_context:null},'*');}
      return;
    }

    // Live rect query for an existing annotation pin.
    if(d.__er_query_rect===true){
      try{
        if(!d.selector){window.parent.postMessage({__er_query_rect_result:true,id:d.id,rect:null},'*');return;}
        var hit2=deepQuerySelector(document,d.selector,0,0);
        if(!hit2){window.parent.postMessage({__er_query_rect_result:true,id:d.id,rect:null},'*');return;}
        var r2=hit2.el.getBoundingClientRect();
        window.parent.postMessage({__er_query_rect_result:true,id:d.id,rect:{left:hit2.ox+r2.left,top:hit2.oy+r2.top,width:r2.width,height:r2.height}},'*');
      }catch(_){window.parent.postMessage({__er_query_rect_result:true,id:d.id,rect:null},'*');}
      return;
    }

    // Re-anchor: re-resolve selectors after page load to detect stale annotations.
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
      try{window.parent.postMessage({__er_reanchor_result:true,results:results},'*');}catch(_){}
    }
  });
})();"#;

/// Inject the annotation content script before `</head>` (or `</body>` as fallback).
fn inject_script(mut html: Vec<u8>) -> Vec<u8> {
    let tag = format!("<script type=\"text/javascript\">{}</script>", FRAME_SCRIPT);
    if let Some(pos) = find_ascii_case_insensitive(&html, b"</head>") {
        html.splice(pos..pos, tag.bytes());
        html
    } else if let Some(pos) = find_ascii_case_insensitive(&html, b"</body>") {
        html.splice(pos..pos, tag.bytes());
        html
    } else {
        html.extend_from_slice(tag.as_bytes());
        html
    }
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

fn is_html_content_type(content_type: Option<&str>) -> bool {
    content_type
        .map(|ct| ct.to_ascii_lowercase().contains("text/html"))
        .unwrap_or(false)
}

fn should_forward_header(name: &str, is_html: bool) -> bool {
    let name = name.to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "content-length"
            | "content-encoding"
            | "transfer-encoding"
            | "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "upgrade"
    ) {
        return false;
    }
    if is_html
        && matches!(
            name.as_str(),
            "content-security-policy"
                | "content-security-policy-report-only"
                | "x-content-security-policy"
                | "x-webkit-csp"
                | "x-frame-options"
        )
    {
        return false;
    }
    true
}

#[derive(Clone, Debug)]
struct ProxyHeader {
    name: String,
    value: String,
}

fn filtered_proxy_headers(headers: &[ProxyHeader], is_html: bool) -> Vec<ProxyHeader> {
    headers
        .iter()
        .filter(|h| should_forward_header(&h.name, is_html))
        .cloned()
        .collect()
}

fn upstream_url_for_proxy(uri: &tauri::http::Uri, upstream_scheme: &str) -> String {
    let authority = uri.authority().map(|a| a.as_str()).unwrap_or("localhost");
    let path = uri.path();
    let path = if path.is_empty() { "/" } else { path };
    match uri.query() {
        Some(q) => format!("{}://{}{}?{}", upstream_scheme, authority, path, q),
        None => format!("{}://{}{}", upstream_scheme, authority, path),
    }
}

const PROXY_HTML_SIZE_LIMIT: usize = 10 * 1024 * 1024; // 10 MB
                                                       // Vite/SvelteKit dev-server JS dependency chunks can be large, especially when
                                                       // source maps or prebundled dependencies are served through the dev server.
                                                       // Non-HTML assets are still bounded to avoid unbounded memory use during proxying.
const PROXY_ASSET_SIZE_LIMIT: usize = 128 * 1024 * 1024; // 128 MB

fn proxy_size_limit(is_html: bool) -> usize {
    if is_html {
        return std::env::var("ER_PROXY_HTML_LIMIT_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(PROXY_HTML_SIZE_LIMIT);
    }
    std::env::var("ER_PROXY_ASSET_LIMIT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(PROXY_ASSET_SIZE_LIMIT)
}

fn oversized_response(
    bytes: usize,
    size_limit: usize,
    is_html: bool,
) -> tauri::http::Response<Vec<u8>> {
    let (content_type, body) = if is_html {
        (
            "text/html",
            format!(
                "<html><body><p>Response too large ({} bytes exceeds {} byte limit).</p></body></html>",
                bytes, size_limit
            ),
        )
    } else {
        (
            "text/plain",
            format!(
                "Response too large ({} bytes exceeds {} byte limit).",
                bytes, size_limit
            ),
        )
    };
    tauri::http::Response::builder()
        .status(413)
        .header("Content-Type", content_type)
        .body(body.into_bytes())
        .unwrap()
}

/// Read at most `limit` bytes from `reader`. Returns `Ok(bytes)` on success,
/// `Err(bytes_read)` if the limit was exceeded.
/// Read at most `limit` bytes from `reader`.
/// Returns `Ok(bytes)` on clean EOF within the cap.
/// Returns `Err(bytes_so_far)` if the limit is exceeded.
/// Propagates I/O errors (e.g. connection reset) as `Err(bytes_so_far)`.
fn read_bounded(mut reader: impl std::io::Read, limit: usize) -> Result<Vec<u8>, usize> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > limit {
                    return Err(buf.len() + n);
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(_) => return Err(buf.len()),
        }
    }
    Ok(buf)
}

fn proxied_response(
    uri: &tauri::http::Uri,
    upstream_scheme: &str,
) -> tauri::http::Response<Vec<u8>> {
    let target = upstream_url_for_proxy(uri, upstream_scheme);
    eprintln!("[erp] request: {} -> {}", uri, target);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();
    let result = agent.get(&target).set("Accept-Encoding", "identity").call();
    let resp = match result {
        Ok(resp) => resp,
        Err(ureq::Error::Status(_, resp)) => resp,
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("timed out") || msg.contains("TimedOut") {
                504
            } else {
                502
            };
            let label = if status == 504 {
                "Request timed out"
            } else {
                "Connection failed"
            };
            eprintln!("[erp] connection error ({}): {}", status, e);
            return tauri::http::Response::builder()
                .status(status)
                .header("Content-Type", "text/html")
                .body(format!("<html><body><p>{}: {}</p></body></html>", label, e).into_bytes())
                .unwrap();
        }
    };

    let status = resp.status();
    let ct = resp.header("Content-Type").map(str::to_string);
    let is_html = is_html_content_type(ct.as_deref());
    let headers = resp
        .headers_names()
        .into_iter()
        .flat_map(|name| {
            resp.all(&name).into_iter().map(move |value| ProxyHeader {
                name: name.clone(),
                value: value.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let size_limit = proxy_size_limit(is_html);
    let bounded = read_bounded(resp.into_reader(), size_limit);
    let mut body = match bounded {
        Ok(b) => b,
        Err(bytes) => {
            eprintln!(
                "[erp] response too large: {} bytes exceeds limit {}",
                bytes, size_limit
            );
            return oversized_response(bytes, size_limit, is_html);
        }
    };
    if is_html {
        body = inject_script(body);
    }

    eprintln!(
        "[erp] response: status={} content-type={:?} is_html={}",
        status, ct, is_html
    );
    let mut builder = tauri::http::Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*");
    for h in filtered_proxy_headers(&headers, is_html) {
        builder = builder.header(&h.name, &h.value);
    }
    if is_html
        && !headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("cache-control"))
    {
        builder = builder.header("Cache-Control", "no-cache");
    }
    builder.body(body).unwrap_or_else(|_| {
        tauri::http::Response::builder()
            .status(500)
            .body(vec![])
            .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_read_under_limit_succeeds() {
        let data = b"hello world";
        let result = read_bounded(std::io::Cursor::new(data), 100);
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn bounded_read_at_limit_succeeds() {
        let data = vec![0u8; 100];
        let result = read_bounded(std::io::Cursor::new(data.clone()), 100);
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn bounded_read_over_limit_returns_err() {
        let data = vec![0u8; 101];
        let result = read_bounded(std::io::Cursor::new(data), 100);
        assert!(result.is_err());
    }

    #[test]
    fn injects_before_head_case_insensitively() {
        let out = inject_script(b"<HTML><HEAD></HEAD><body></body></HTML>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("<script type=\"text/javascript\">"));
        assert!(
            s.find("<script").unwrap() < s.find("</HEAD>").unwrap(),
            "script should be inserted before uppercase head close: {s}"
        );
    }

    #[test]
    fn injects_before_body_case_insensitively() {
        let out = inject_script(b"<html><BODY><main></main></BODY></html>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.find("<script").unwrap() < s.find("</BODY>").unwrap(),
            "script should be inserted before uppercase body close: {s}"
        );
    }

    #[test]
    fn strips_frame_blocking_headers_for_html() {
        let headers = vec![
            ProxyHeader {
                name: "Content-Type".into(),
                value: "text/html".into(),
            },
            ProxyHeader {
                name: "Content-Security-Policy".into(),
                value: "default-src 'self'".into(),
            },
            ProxyHeader {
                name: "X-Frame-Options".into(),
                value: "DENY".into(),
            },
            ProxyHeader {
                name: "Cache-Control".into(),
                value: "max-age=60".into(),
            },
        ];
        let names = filtered_proxy_headers(&headers, true)
            .into_iter()
            .map(|h| h.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Content-Type", "Cache-Control"]);
    }

    #[test]
    fn keeps_non_html_body_unmodified_and_headers_unstripped() {
        let body = b"body { color: red; }".to_vec();
        assert_eq!(body, b"body { color: red; }".to_vec());
        let headers = vec![
            ProxyHeader {
                name: "Content-Type".into(),
                value: "text/css".into(),
            },
            ProxyHeader {
                name: "Content-Security-Policy".into(),
                value: "default-src 'self'".into(),
            },
        ];
        let names = filtered_proxy_headers(&headers, false)
            .into_iter()
            .map(|h| h.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Content-Type", "Content-Security-Policy"]);
    }

    #[test]
    fn asset_size_limit_exceeds_observed_vite_chunks() {
        // Observed Vite dep chunks that triggered reload loops were ~5.2 MB
        // and later ~26.2 MB.
        assert!(PROXY_ASSET_SIZE_LIMIT > 25 * 1024 * 1024);
        assert!(PROXY_ASSET_SIZE_LIMIT >= 26_219_024);
    }

    #[test]
    fn oversized_html_response_uses_text_html() {
        let resp = oversized_response(99, 50, true);
        assert_eq!(resp.status(), 413);
        let ct = resp
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "text/html");
    }

    #[test]
    fn oversized_non_html_response_uses_text_plain() {
        let resp = oversized_response(99, 50, false);
        assert_eq!(resp.status(), 413);
        let ct = resp
            .headers()
            .get("Content-Type")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(ct, "text/plain");
        let body = String::from_utf8(resp.body().clone()).unwrap();
        assert!(!body.contains("<html>"));
    }

    #[test]
    fn upstream_url_preserves_proxy_scheme_intent() {
        let http_uri: tauri::http::Uri = "erp://localhost:6006/iframe.html".parse().unwrap();
        assert_eq!(
            upstream_url_for_proxy(&http_uri, "http"),
            "http://localhost:6006/iframe.html"
        );
        let https_uri: tauri::http::Uri = "erps://google.com/search?q=x".parse().unwrap();
        assert_eq!(
            upstream_url_for_proxy(&https_uri, "https"),
            "https://google.com/search?q=x"
        );
    }
}

fn main() {
    let mut app = App::new_with_args(&[]).unwrap_or_else(|e| {
        eprintln!("er-desktop: failed to init engine: {e}"); // before logger is up
        std::process::exit(1);
    });

    // Auto-register the current repo on launch.
    {
        let root = app.tab().repo_root.clone();
        if !root.is_empty() {
            let _ = projects::auto_register(&root);
        }
    }

    // Restore persisted tab list, if present. Failures are non-fatal: we
    // simply keep the default single-tab launch.
    if let Some(file) = tabs::load_tabs() {
        let mut rebuilt: Vec<er_engine::app::TabState> = Vec::new();
        for d in &file.tabs {
            match tabs::rebuild_tab(d) {
                Ok(t) => rebuilt.push(t),
                Err(e) => log::warn!(
                    "er-desktop: skipping persisted tab {:?} ({}): {e}",
                    d.kind,
                    d.repo_root
                ),
            }
        }
        if !rebuilt.is_empty() {
            app.tabs = rebuilt;
            let clamped = file.active_idx.min(app.tabs.len() - 1);
            app.active_tab = clamped;
        }
    }

    let pr_cache: Arc<Mutex<HashMap<String, Vec<PrInfo>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pr_cache_fetched_at: Arc<Mutex<HashMap<String, u64>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let pr_open_cache: Arc<
        Mutex<HashMap<commands::PrOpenCacheKey, commands::PrOpenCacheEntry>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    let meta_cache: Arc<Mutex<HashMap<String, ProjectMeta>>> = Arc::new(Mutex::new(HashMap::new()));
    let gh_user: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let gh_status_cache: Arc<Mutex<HashMap<(String, String, u64), GithubStatusSnapshot>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let loading: LoadingState = Arc::new(Mutex::new(LoadingFlags::default()));
    let watch_status: WatchStatusState = Arc::new(Mutex::new(WatchStatusSnapshot::default()));

    // Resolve the gh user login once at launch in a background thread.
    // Don't block startup; if it fails, leave as None.
    {
        let gh_user_bg = Arc::clone(&gh_user);
        std::thread::spawn(move || {
            if let Ok(out) = std::process::Command::new("gh")
                .args(["api", "user", "--jq", ".login"])
                .output()
            {
                if out.status.success() {
                    let login = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !login.is_empty() {
                        if let Ok(mut g) = gh_user_bg.lock() {
                            *g = Some(login);
                        }
                    }
                }
            }
        });
    }

    // Capture the launch-time active root for the first meta-refresh iteration.
    let launch_root: String = {
        let initial = app.tab().repo_root.clone();
        initial
    };

    let gh_status_in_flight: Arc<Mutex<std::collections::HashSet<(String, String, u64)>>> =
        Arc::new(Mutex::new(std::collections::HashSet::new()));
    let app_arc: Arc<Mutex<App>> = Arc::new(Mutex::new(app));
    let terminals: Arc<Mutex<HashMap<String, terminal::PtySession>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let terminals_for_exit = Arc::clone(&terminals);
    let desktop_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(0));
    let last_sent_revision: Arc<std::sync::atomic::AtomicU64> =
        Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let state = AppState {
        app: Arc::clone(&app_arc),
        highlighter: Mutex::new(Highlighter::new()),
        pr_cache: Arc::clone(&pr_cache),
        pr_cache_fetched_at: Arc::clone(&pr_cache_fetched_at),
        pr_open_cache: Arc::clone(&pr_open_cache),
        meta_cache: Arc::clone(&meta_cache),
        gh_user: Arc::clone(&gh_user),
        terminals: Arc::clone(&terminals),
        pending_ai_replies: Arc::new(Mutex::new(HashMap::new())),
        gh_status_cache: Arc::clone(&gh_status_cache),
        loading: Arc::clone(&loading),
        gh_status_in_flight: Arc::clone(&gh_status_in_flight),
        desktop_revision: Arc::clone(&desktop_revision),
        last_sent_revision: Arc::clone(&last_sent_revision),
        watch_status: Arc::clone(&watch_status),
    };

    match pr_cache::load_persisted_pr_cache() {
        Ok(Some((cached_prs, cached_fetched_at))) => {
            if let Ok(mut g) = pr_cache.lock() {
                *g = cached_prs;
            }
            if let Ok(mut g) = pr_cache_fetched_at.lock() {
                *g = cached_fetched_at;
            }
            desktop_revision.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("failed to load persisted PR cache: {e}");
        }
    }

    // Startup GitHub-status kick. Remote-PR tabs have enough metadata to fetch
    // immediately; local-branch PR matching waits until the sidebar lazily loads
    // that repo's PR cache.
    {
        let gh_status_startup = Arc::clone(&gh_status_cache);
        let app_startup = Arc::clone(&app_arc);
        let pr_cache_startup = Arc::clone(&pr_cache);
        let desktop_rev_startup = Arc::clone(&desktop_revision);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let key: Option<(String, String, u64)> = match app_startup.lock() {
                Ok(g) => {
                    let tab = g.tab();
                    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), n))
                    } else {
                        let branch = tab
                            .local_branch_view
                            .as_deref()
                            .unwrap_or(&tab.current_branch)
                            .to_string();
                        pr_cache_startup.lock().ok().and_then(|cache| {
                            cache.iter().find_map(|(slug, prs)| {
                                prs.iter()
                                    .filter(|p| p.head_ref == branch)
                                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                                    .and_then(|p| {
                                        slug.split_once('/')
                                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                                    })
                            })
                        })
                    }
                }
                Err(_) => None,
            };
            if let Some((owner, repo, number)) = key {
                if let Some(snap) = commands::fetch_github_status(&owner, &repo, number) {
                    if let Ok(mut g) = gh_status_startup.lock() {
                        g.insert((owner, repo, number), snap);
                    }
                    desktop_rev_startup.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        });
    }

    // Spawn background remote-PR diff refresh: every 45s, if the active tab
    // points at a remote PR, re-fetch its diff. Lets `gh pr diff` updates land
    // without user action. Local-branch tabs update via the engine's notify
    // watcher; remote PRs have no filesystem surface to watch.
    let remote_app = Arc::clone(&app_arc);
    let remote_loading = Arc::clone(&loading);
    let remote_desktop_rev = Arc::clone(&desktop_revision);
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(45));
        // Use try_lock so a busy app skips this cycle instead of blocking.
        let mut guard = match remote_app.try_lock() {
            Ok(g) => g,
            Err(_) => continue,
        };
        if guard.tab().is_remote() {
            if let Ok(mut f) = remote_loading.lock() {
                f.gh_status = true;
            }
            remote_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let t = std::time::Instant::now();
            if let Err(e) = guard.tab_mut().refresh_diff() {
                log::error!("remote PR refresh failed: {e}");
            } else {
                log::info!(
                    "remote PR diff refresh done in {}ms",
                    t.elapsed().as_millis()
                );
            }
            if let Ok(mut f) = remote_loading.lock() {
                f.gh_status = false;
            }
            remote_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });

    // Spawn background GitHub-status refresh: every 30s, identify the active
    // tab's PR identity (briefly locking app), then shell out to gh and update
    // the cache. NEVER holds the app mutex while running gh.
    // Shares gh_status_in_flight with kick_github_status_refresh to avoid
    // duplicate concurrent fetches when a branch switch fires during this loop.
    {
        let gh_status_bg = Arc::clone(&gh_status_cache);
        let app_bg = Arc::clone(&app_arc);
        let pr_cache_bg = Arc::clone(&pr_cache);
        let gh_status_loading = Arc::clone(&loading);
        let gh_status_in_flight_bg = Arc::clone(&gh_status_in_flight);
        let gh_status_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || loop {
            // Snapshot identity in a short critical section.
            let key: Option<(String, String, u64)> = match app_bg.lock() {
                Ok(g) => {
                    let tab = g.tab();
                    // Remote PR tab — use remote_repo + pr_number directly.
                    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), n))
                    } else {
                        // Working-tree or local-branch tab — look up by head ref.
                        let branch = tab
                            .local_branch_view
                            .as_deref()
                            .unwrap_or(&tab.current_branch)
                            .to_string();
                        pr_cache_bg.lock().ok().and_then(|cache| {
                            cache.iter().find_map(|(slug, prs)| {
                                prs.iter()
                                    .filter(|p| p.head_ref == branch)
                                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                                    .and_then(|p| {
                                        slug.split_once('/')
                                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                                    })
                            })
                        })
                    }
                }
                Err(_) => None,
            };
            // Lock released. Register in-flight and shell out — skip if already fetching.
            if let Some((owner, repo, number)) = key {
                let registered = gh_status_in_flight_bg
                    .lock()
                    .map(|mut s| s.insert((owner.clone(), repo.clone(), number)))
                    .unwrap_or(false);
                if registered {
                    if let Ok(mut f) = gh_status_loading.lock() {
                        f.gh_status = true;
                    }
                    gh_status_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Some(snap) = commands::fetch_github_status(&owner, &repo, number) {
                        if let Ok(mut g) = gh_status_bg.lock() {
                            g.insert((owner.clone(), repo.clone(), number), snap);
                        }
                    }
                    if let Ok(mut f) = gh_status_loading.lock() {
                        f.gh_status = false;
                    }
                    gh_status_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let _ = gh_status_in_flight_bg
                        .lock()
                        .map(|mut s| s.remove(&(owner, repo, number)));
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(30));
        });
    }

    // Spawn background GitHub comment sync. Three-phase design so network I/O
    // never holds the app mutex:
    //   Phase 1 (brief lock) — snapshot identity + files from active tab.
    //   Phase 2 (no lock)   — fetch comments, review threads, PR overview.
    //   Phase 3 (brief lock) — apply results to the correct tab.
    {
        let comments_app = Arc::clone(&app_arc);
        let comments_pr_cache = Arc::clone(&pr_cache);
        let comments_loading = Arc::clone(&loading);
        let comments_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(45));

            // Phase 1: brief lock — snapshot identity + files, then release.
            let ctx = {
                let guard = match comments_app.try_lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let tab = guard.tab();
                let identity: Option<(String, String, u64)> =
                    if let (Some(slug), Some(n)) = (tab.remote_repo.as_ref(), tab.pr_number) {
                        slug.split_once('/')
                            .map(|(o, r)| (o.to_string(), r.to_string(), n))
                    } else {
                        let branch = tab
                            .local_branch_view
                            .as_deref()
                            .unwrap_or(&tab.current_branch)
                            .to_string();
                        comments_pr_cache.lock().ok().and_then(|cache| {
                            cache.iter().find_map(|(slug, prs)| {
                                prs.iter()
                                    .filter(|p| p.head_ref == branch)
                                    .min_by_key(|p| if p.state == "OPEN" { 0 } else { 1 })
                                    .and_then(|p| {
                                        slug.split_once('/')
                                            .map(|(o, r)| (o.to_string(), r.to_string(), p.number))
                                    })
                            })
                        })
                    };
                // Snapshot all data needed for the fetch — releases lock after this block.
                identity.map(|(owner, repo, number)| {
                    guard.snapshot_for_comment_sync(owner, repo, number)
                })
            };

            // Phase 2: network I/O — no lock held.
            if let Some(ctx) = ctx {
                if let Ok(mut f) = comments_loading.lock() {
                    f.gh_comments = true;
                }
                comments_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let t = std::time::Instant::now();
                match er_engine::app::fetch_comment_sync_data(&ctx) {
                    Ok(result) => {
                        // Phase 3: brief lock — apply pre-fetched results to the correct tab.
                        match comments_app.lock() {
                            Ok(mut g) => g.apply_comment_sync_result(result),
                            Err(e) => log::error!("comment sync apply lock failed: {e}"),
                        }
                        log::info!("gh comment sync done in {}ms", t.elapsed().as_millis());
                    }
                    Err(e) => log::error!("github comment sync failed: {e}"),
                }
                if let Ok(mut f) = comments_loading.lock() {
                    f.gh_comments = false;
                }
                comments_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
    }

    // Spawn the desktop active-branch watcher. Follows the currently active
    // tab's local-branch checkout (project root or linked worktree) and
    // refreshes the diff when tracked files in that checkout change.
    {
        let watcher_app = Arc::clone(&app_arc);
        let watcher_status = Arc::clone(&watch_status);
        let watcher_desktop_rev = Arc::clone(&desktop_revision);
        std::thread::spawn(move || {
            use er_engine::watch::{FileWatcher, WatchEvent};
            use std::path::Path;
            use std::sync::mpsc;

            let (tx, rx) = mpsc::channel::<WatchEvent>();
            // Held only for its Drop side effect: dropping stops the watcher.
            #[allow(unused_assignments)]
            let mut watcher: Option<FileWatcher> = None;
            let mut current_key: Option<(String, String)> = None;
            let poll_interval = std::time::Duration::from_millis(400);

            loop {
                let desired = match watcher_app.lock() {
                    Ok(g) => desired_local_branch_watch(&g),
                    Err(_) => None,
                };

                if desired != current_key {
                    watcher = None; // drop old watcher
                    if let Some((ref branch, ref root_path)) = desired {
                        match FileWatcher::new(Path::new(root_path), 500, tx.clone()) {
                            Ok(w) => {
                                watcher = Some(w);
                                if let Ok(mut s) = watcher_status.lock() {
                                    *s = WatchStatusSnapshot {
                                        active: true,
                                        branch: Some(branch.clone()),
                                        root_path: Some(root_path.clone()),
                                    };
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "er-desktop: active-branch watcher failed for {root_path}: {e}"
                                );
                                if let Ok(mut s) = watcher_status.lock() {
                                    *s = WatchStatusSnapshot::default();
                                }
                            }
                        }
                    } else if let Ok(mut s) = watcher_status.lock() {
                        *s = WatchStatusSnapshot::default();
                    }

                    // Mirror checkout root onto the active tab so refresh_diff
                    // uses the working-tree helper.
                    if let Ok(mut g) = watcher_app.lock() {
                        let checkout_root = desired.as_ref().map(|(_, r)| r.clone());
                        let active_branch = desired.as_ref().map(|(b, _)| b.clone());
                        let tab = g.tab_mut();
                        if tab.local_branch_view == active_branch {
                            tab.local_branch_checkout_root = checkout_root;
                        }
                    }
                    current_key = desired;
                    watcher_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }

                // Drain any pending watch events. Coalesce — we only need to
                // know "something changed" to trigger one refresh.
                let mut got_event = false;
                loop {
                    match rx.recv_timeout(poll_interval) {
                        Ok(WatchEvent::FilesChanged(_)) => {
                            got_event = true;
                            // Keep draining without blocking.
                            while let Ok(WatchEvent::FilesChanged(_)) = rx.try_recv() {}
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }

                if got_event {
                    if let Some((ref watched_branch, _)) = current_key {
                        let mut refreshed = false;
                        if let Ok(mut g) = watcher_app.lock() {
                            if g.tab().local_branch_view.as_deref() == Some(watched_branch.as_str())
                            {
                                if let Err(e) = g.tab_mut().refresh_diff_quick() {
                                    log::error!("active-branch watcher refresh failed: {e}");
                                } else {
                                    refreshed = true;
                                }
                            }
                        }
                        if refreshed {
                            watcher_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }
        });
    }

    // Fetch PRs once at startup. Manual refresh is available via the
    // `refresh_pr_list` command. No background loop — on-demand only.
    let bg_cache = Arc::clone(&pr_cache);
    let bg_fetched_at = Arc::clone(&pr_cache_fetched_at);
    let bg_loading = Arc::clone(&loading);
    let bg_desktop_rev = Arc::clone(&desktop_revision);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = true;
            }
            bg_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            pr_cache::refresh_pr_cache(&bg_cache, &bg_fetched_at).await;
            if let Ok(mut f) = bg_loading.lock() {
                f.pr_list = false;
            }
            bg_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
    });

    // Spawn background meta-cache refresh: keeps per-project git metadata
    // (branches, worktrees, current/base branch) fresh without ever taking
    // the AppState.app mutex.
    let bg_meta = Arc::clone(&meta_cache);
    let meta_desktop_rev = Arc::clone(&desktop_revision);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        rt.block_on(async move {
            // First pass uses launch-time root.
            meta_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            snapshot::refresh_meta_cache(&launch_root, &bg_meta);
            meta_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                // Re-derive active root from the persistent projects file —
                // avoids touching AppState.app.
                let active_root =
                    active_root_from_projects().unwrap_or_else(|| launch_root.clone());
                meta_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                snapshot::refresh_meta_cache(&active_root, &bg_meta);
                meta_desktop_rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
    });

    let persist_app = Arc::clone(&app_arc);
    let tauri_app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(state)
        // `erp://host/path` proxies `http://host/path`; `erps://host/path`
        // proxies `https://host/path`. HTML responses get the annotation script.
        .register_uri_scheme_protocol("erp", |_app, request| {
            proxied_response(request.uri(), "http")
        })
        .register_uri_scheme_protocol("erps", |_app, request| {
            proxied_response(request.uri(), "https")
        })
        .setup(|app| {
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("Easy Review")
            .inner_size(1400.0, 900.0)
            .min_inner_size(900.0, 600.0)
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .initialization_script(FRAME_SCRIPT)
            .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_window_drag,
            commands::get_snapshot,
            commands::toggle_panel,
            commands::select_file,
            commands::next_file,
            commands::prev_file,
            commands::jump_to_unreviewed,
            commands::set_mode,
            commands::toggle_reviewed,
            commands::mark_reviewed,
            commands::unmark_reviewed,
            commands::open_in_editor,
            commands::open_source,
            commands::open_url_in_browser,
            commands::reveal_er_folder,
            commands::list_review_revisions,
            commands::read_review_json,
            commands::next_hunk,
            commands::prev_hunk,
            commands::toggle_compacted,
            commands::set_filter,
            commands::clear_filter,
            commands::add_comment,
            commands::add_question,
            commands::reply_to_thread,
            commands::delete_thread,
            commands::resolve_thread,
            commands::refresh_diff,
            commands::force_refresh_diff,
            commands::refresh_github_status,
            commands::pull_github_comments,
            commands::push_github_comments,
            commands::submit_github_review,
            commands::run_ai_review,
            commands::run_ai_validate,
            commands::set_ai_model,
            commands::list_ai_providers,
            commands::set_ai_selection,
            commands::promote_to_comment,
            commands::ask_ai,
            commands::open_pr_url,
            commands::open_remote_pr,
            commands::open_worktree,
            commands::dismiss_finding,
            commands::promote_finding_to_comment,
            commands::reply_to_finding,
            commands::export_review,
            commands::export_review_to_file,
            commands::export_to_agent,
            commands::open_commit_composer,
            commands::select_commit,
            commands::poll,
            commands::open_local_branch,
            commands::open_pr_branch,
            commands::open_pr_review,
            commands::refresh_pr_list,
            commands::dismiss_remote_pr,
            commands::track_pr,
            commands::untrack_pr,
            commands::list_available_prs,
            commands::set_active_project,
            commands::add_tracked_branch,
            commands::remove_tracked_branch,
            commands::list_available_branches,
            commands::open_project_branch,
            commands::new_tab,
            commands::close_tab,
            commands::select_tab,
            commands::reorder_tabs,
            commands::add_ui_annotation,
            commands::delete_ui_annotation,
            commands::clear_ui_annotations,
            commands::list_ui_annotations,
            commands::update_ui_annotation_anchors,
            commands::save_annotation_screenshot,
            commands::read_annotation_screenshot,
            commands::terminal_spawn,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::detect_dev_url,
            commands::set_diff_source,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application");

    tauri_app.run(move |_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            // Best-effort persistence of the open tab list. We must not block
            // exit on save errors — log and move on.
            if let Ok(guard) = persist_app.lock() {
                let descriptors: Vec<tabs::TabDescriptor> =
                    guard.tabs.iter().map(tabs::descriptor_from_tab).collect();
                let active = guard.active_tab;
                drop(guard);
                if let Err(e) = tabs::save_tabs(&descriptors, active) {
                    log::error!("er-desktop: failed to save tabs: {e}");
                }
            }
            // Kill any live shell sessions. Drop runs PtySession::drop on
            // each, which kills + reaps the child.
            if let Ok(mut map) = terminals_for_exit.lock() {
                map.clear();
            }
        }
    });
}

/// Compute the desired (branch, checkout_root) for the active local-branch
/// tab. Returns None unless the active tab is a local-branch view (not a
/// remote PR, not a local PR ref) AND that branch is checked out somewhere
/// (project root via HEAD, or a linked worktree).
fn desired_local_branch_watch(app: &App) -> Option<(String, String)> {
    let tab = app.tab();
    let branch = tab.local_branch_view.clone()?;
    if tab.remote_repo.is_some() || tab.pr_head_ref.is_some() {
        return None;
    }

    // Project root checkout?
    let head_out = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&tab.repo_root)
        .output()
        .ok()?;
    if head_out.status.success() {
        let head = String::from_utf8_lossy(&head_out.stdout).trim().to_string();
        if head == branch {
            return Some((branch, tab.repo_root.clone()));
        }
    }

    // Linked worktree checkout?
    let worktrees = er_engine::git::list_worktrees(&tab.repo_root).ok()?;
    let wt = worktrees.into_iter().find(|w| w.branch == branch)?;
    Some((branch, wt.path))
}

/// Read the persisted active project's root path, if any.
fn active_root_from_projects() -> Option<String> {
    let file = projects::load();
    let active_id = file.active_id.as_ref()?;
    file.projects
        .iter()
        .find(|p| &p.id == active_id)
        .map(|p| p.root_path.clone())
        .filter(|s| !s.is_empty())
}
