use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

/// 캡처 웹뷰에 주입할 fetch/XHR 후킹 스크립트를 만든다.
/// 캡처는 Tauri IPC(`invoke("capture_push", ...)`)로 Rust에 직접 전달한다.
/// http://127.0.0.1 로 POST하던 예전 방식은 https 페이지에서 mixed content로 차단되므로 쓸 수 없다.
/// 세션 토큰이 박혀 있어 capture_push 가 현재 세션 신원을 검증한다.
pub fn hook_script(token: &str) -> String {
    // response_body 상한 8KB
    format!(
        r#"(function() {{
  var TOKEN = "{token}";
  var origFetch = window.fetch;
  var seq = 0;
  // 캡처 전달은 IPC로. invoke가 내부적으로 fetch를 쓰더라도, 아래 http(s) 스킴 필터가
  // Tauri IPC 트래픽(ipc:// 등)의 자기 재캡처/무한재귀를 막는다.
  function send(call) {{
    try {{
      if (window.__TAURI_INTERNALS__) {{
        window.__TAURI_INTERNALS__.invoke("capture_push", {{ token: TOKEN, call: call }}).catch(function(){{}});
      }}
    }} catch (e) {{}}
  }}
  function truncate(s) {{ return (typeof s === "string" && s.length > 8192) ? s.slice(0, 8192) : s; }}
  function headersToObj(h) {{
    var o = {{}};
    if (h && typeof h.forEach === "function") h.forEach(function(v, k) {{ o[k] = v; }});
    return o;
  }}

  window.fetch = function(input, init) {{
    var req;
    try {{ req = new Request(input, init); }} catch (e) {{ return origFetch.apply(this, arguments); }}
    // 사이트의 http(s) API 호출만 캡처. IPC(ipc://) 등 다른 스킴은 그대로 통과시켜 자기 재캡처를 막는다.
    if (!/^https?:/i.test(req.url)) return origFetch.call(this, req);
    var reqHeaders = headersToObj(req.headers);
    var id = "c" + (++seq);
    // Request 객체에 실린 body도 잡히도록 req.clone()에서 읽는다 (init.body만 보면 놓침). GET은 ""→null.
    var bodyPromise = req.clone().text().then(function(t) {{ return t && t.length ? t : null; }}).catch(function() {{ return null; }});
    // 원본 arguments 대신 정규화된 req를 넘겨 Request-first 스타일의 body 이중소비를 피한다.
    return origFetch.call(this, req).then(function(resp) {{
      try {{
        var clone = resp.clone();
        Promise.all([bodyPromise, clone.text().catch(function(){{ return null; }})]).then(function(arr) {{
          send({{ id: id, method: req.method, url: req.url, request_headers: reqHeaders,
                  request_body: arr[0], status: resp.status, response_body: truncate(arr[1]), timestamp: Date.now() }});
        }});
      }} catch (e) {{}}
      return resp;
    }});
  }};

  var XO = XMLHttpRequest.prototype.open;
  var XS = XMLHttpRequest.prototype.send;
  var XH = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.open = function(method, url) {{
    this.__cap = {{ method: method, url: url, headers: {{}} }};
    return XO.apply(this, arguments);
  }};
  XMLHttpRequest.prototype.setRequestHeader = function(k, v) {{
    if (this.__cap) this.__cap.headers[k] = v;
    return XH.apply(this, arguments);
  }};
  XMLHttpRequest.prototype.send = function(body) {{
    var self = this;
    if (self.__cap) {{
      self.addEventListener("loadend", function() {{
        try {{
          var abs;
          try {{ abs = new URL(self.__cap.url, location.href).href; }} catch (e) {{ abs = self.__cap.url; }}
          // 사이트의 http(s) API 호출만 캡처 (IPC 등 다른 스킴 제외).
          if (!/^https?:/i.test(abs)) return;
          // responseType이 text/''가 아니면 responseText 접근 자체가 예외를 던지므로 먼저 걸러낸다.
          var rt = (self.responseType === "" || self.responseType === "text") ? self.responseText : null;
          send({{ id: "c" + (++seq), method: self.__cap.method, url: abs, request_headers: self.__cap.headers,
                  request_body: body != null ? String(body) : null, status: self.status,
                  response_body: truncate(rt), timestamp: Date.now() }});
        }} catch (e) {{}}
      }});
    }}
    return XS.apply(this, arguments);
  }};
}})();"#
    )
}

/// 캡처 창에서 사용자의 UI 조작(클릭/입력)을 기록하는 스크립트를 만든다.
/// 각 요소에 대해 우선순위 셀렉터 사다리(testid→id→name→role→text→css)를 만들어
/// IPC(`invoke("ui_record", ...)`)로 전달한다. 재생 시 이 후보들을 순서대로 시도(자가치유)한다.
pub fn recorder_script(token: &str) -> String {
    format!(
        r##"(function() {{
  var TOKEN = "{token}";
  var uiseq = 0;
  function send(action) {{
    try {{
      if (window.__TAURI_INTERNALS__) {{
        window.__TAURI_INTERNALS__.invoke("ui_record", {{ token: TOKEN, action: action }}).catch(function(){{}});
      }}
    }} catch (e) {{}}
  }}
  function esc(s) {{ return (window.CSS && CSS.escape) ? CSS.escape(s) : String(s).replace(/[^a-zA-Z0-9_-]/g, "\\$&"); }}
  function stableId(id) {{ return id && !/^[0-9]/.test(id) && !/[0-9a-f]{{6,}}/i.test(id) && id.length < 40; }}
  function stableClass(c) {{ return c && !/^(css-|sc-|jss|makeStyles|_)/.test(c) && !/[0-9a-f]{{5,}}/i.test(c) && !/\d{{3,}}/.test(c); }}
  function nameOf(el) {{
    var a = (el.getAttribute("aria-label") || "").trim(); if (a) return a;
    if (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT")
      return (el.getAttribute("placeholder") || el.getAttribute("name") ||
              (el.labels && el.labels[0] && el.labels[0].textContent) || "").trim().slice(0, 60);
    return (el.textContent || el.value || "").trim().replace(/\s+/g, " ").slice(0, 60);
  }}
  function roleOf(el) {{
    var r = el.getAttribute("role"); if (r) return r;
    var t = el.tagName.toLowerCase();
    if (t === "button") return "button";
    if (t === "a" && el.hasAttribute("href")) return "link";
    if (t === "select") return "combobox";
    if (t === "textarea") return "textbox";
    if (t === "input") {{ var ty = (el.getAttribute("type") || "text").toLowerCase();
      if (ty === "checkbox") return "checkbox"; if (ty === "radio") return "radio";
      if (ty === "submit" || ty === "button") return "button"; return "textbox"; }}
    return "";
  }}
  function cssPath(el) {{
    var parts = [], node = el, depth = 0;
    while (node && node.nodeType === 1 && depth < 5) {{
      if (node.id && stableId(node.id)) {{ parts.unshift("#" + esc(node.id)); break; }}
      var sel = node.tagName.toLowerCase();
      var cls = Array.prototype.filter.call(node.classList || [], stableClass);
      if (cls.length) sel += "." + cls.slice(0, 2).map(esc).join(".");
      var p = node.parentElement;
      if (p) {{
        var same = Array.prototype.filter.call(p.children, function(c) {{ return c.tagName === node.tagName; }});
        if (same.length > 1) sel += ":nth-of-type(" + (Array.prototype.indexOf.call(p.children, node) + 1) + ")";
      }}
      parts.unshift(sel);
      node = node.parentElement; depth++;
    }}
    return parts.join(" > ");
  }}
  function ladder(el) {{
    var out = [];
    var tid = el.getAttribute("data-testid") || el.getAttribute("data-test") || el.getAttribute("data-cy");
    if (tid) out.push({{ strategy: "testid", value: tid }});
    if (el.id && stableId(el.id)) out.push({{ strategy: "id", value: el.id }});
    if (el.getAttribute("name")) out.push({{ strategy: "name", value: el.tagName.toLowerCase() + "[name=" + el.getAttribute("name") + "]" }});
    var role = roleOf(el), nm = nameOf(el);
    if (role && nm) out.push({{ strategy: "role", value: role + "|" + nm }});
    if (nm && (role === "button" || role === "link")) out.push({{ strategy: "text", value: nm }});
    out.push({{ strategy: "css", value: cssPath(el) }});
    return out;
  }}
  function record(kind, el, value) {{
    if (!el || el.nodeType !== 1 || el.tagName === "HTML" || el.tagName === "BODY") return;
    send({{ id: "u" + (++uiseq), kind: kind, selectors: ladder(el), name: nameOf(el),
            value: (value != null ? String(value) : null), url: location.href, timestamp: Date.now() }});
  }}
  // 클릭 캡처: click 이벤트가 정상이면 그걸 쓰고, hover 메뉴처럼 mousedown에서 이동/닫힘이
  // 일어나 click이 안 뜨는 경우를 위해 pointerdown 폴백(뒤이어 click이 오면 취소)을 둔다.
  var CLICKSEL = "a,button,[role=button],[role=link],[role=menuitem],[role=tab],[role=option],input,select,label,summary";
  function actionableOf(el) {{ return (el && el.closest) ? (el.closest(CLICKSEL) || el) : el; }}
  var __pd = null;
  document.addEventListener("pointerdown", function(e) {{
    var t = actionableOf(e.target);
    if (__pd) clearTimeout(__pd.timer);
    __pd = {{ t: t, timer: setTimeout(function() {{ if (__pd && __pd.t === t) {{ record("click", t, null); __pd = null; }} }}, 350) }};
  }}, true);
  document.addEventListener("click", function(e) {{
    if (__pd) {{ clearTimeout(__pd.timer); __pd = null; }}
    record("click", actionableOf(e.target), null);
  }}, true);
  // 입력: input을 디바운스로 잡아 blur 없이도 최종 값을 기록. 같은 값 중복은 건너뜀.
  // (비밀번호 값도 기록한다 — 로컬 테스트 재생을 위해. 저장 파일에 평문 포함되니 유의)
  var __timers = new WeakMap(), __lastVal = new WeakMap();
  function recInput(el) {{
    var v = el.value;
    if (__lastVal.get(el) === v) return;
    __lastVal.set(el, v);
    record("input", el, v);
  }}
  document.addEventListener("input", function(e) {{
    var el = e.target;
    if (!el || (el.tagName !== "INPUT" && el.tagName !== "TEXTAREA")) return;
    clearTimeout(__timers.get(el));
    __timers.set(el, setTimeout(function() {{ recInput(el); }}, 600));
  }}, true);
  document.addEventListener("change", function(e) {{
    var el = e.target;
    if (!el || (el.tagName !== "INPUT" && el.tagName !== "TEXTAREA" && el.tagName !== "SELECT")) return;
    clearTimeout(__timers.get(el));
    recInput(el);
  }}, true);
  // hover 메뉴 감지: 마우스 올린 직후 '클릭 가능한 항목이 있는' 메뉴가 나타나면 hover 스텝 기록.
  // (재생 시 그 요소에 hover를 쏴서 메뉴를 연 뒤 다음 클릭이 성공하게 함)
  var __lastOver = null, __lastHover = null;
  document.addEventListener("mouseover", function(e) {{ __lastOver = {{ el: e.target, t: Date.now() }}; }}, true);
  function recordHover(el) {{
    if (!el || el.nodeType !== 1 || el.tagName === "HTML" || el.tagName === "BODY") return;
    if (__lastHover && __lastHover.el === el && Date.now() - __lastHover.t < 1500) return;
    __lastHover = {{ el: el, t: Date.now() }};
    send({{ id: "u" + (++uiseq), kind: "hover", selectors: ladder(el), name: nameOf(el),
            value: null, url: location.href, timestamp: Date.now() }});
  }}
  try {{
    var __mo = new MutationObserver(function(muts) {{
      if (!__lastOver || Date.now() - __lastOver.t > 900) return;
      for (var i = 0; i < muts.length; i++) {{
        var added = muts[i].addedNodes;
        for (var j = 0; j < added.length; j++) {{
          var n = added[j];
          if (!n || n.nodeType !== 1) continue;
          var o = __lastOver.el;
          if (o && (o === n || (n.contains && n.contains(o)) || (o.contains && o.contains(n)))) continue;
          if (n.querySelector && n.querySelector("a,button,[role=menuitem],[role=link],[role=option]")) {{
            var trig = o.closest ? (o.closest("[role],a,button,li") || o) : o;
            recordHover(trig);
            return;
          }}
        }}
      }}
    }});
    __mo.observe(document.documentElement, {{ childList: true, subtree: true }});
  }} catch (e) {{}}
}})();"##
    )
}

/// 기록된 UI 동작을 재생 웹뷰("replay")에서 실행하는 플레이어 스크립트를 만든다.
/// 셀렉터 사다리를 순서대로 시도(자가치유)하고, actionability(보임·안정·활성)까지 대기한 뒤
/// 클릭/입력을 수행한다. 스텝 사이에 네트워크 idle을 기다리고, sessionStorage로 진행 상태를
/// 저장해 페이지 네비게이션을 넘어 재개한다. 결과는 IPC(ui_replay_step)로 보고한다.
/// (format! 대신 placeholder 치환 — JS 중괄호가 많아 이스케이프 회피)
pub fn player_script(token: &str, actions_json: &str) -> String {
    const BODY: &str = r#####"(function(){
  var TOKEN = "__TOKEN__";
  var ACTIONS = __ACTIONS__;
  function inv(cmd, args){ try{ if(window.__TAURI_INTERNALS__) return window.__TAURI_INTERNALS__.invoke(cmd, args); }catch(e){} return Promise.resolve(); }
  function report(index, status, detail, done){ inv("ui_replay_step", { token: TOKEN, result: { index: index, status: status, detail: (detail||""), done: !!done } }); }
  function sleep(ms){ return new Promise(function(r){ setTimeout(r, ms); }); }

  // 네트워크 in-flight 카운터
  var inflight = 0;
  var of = window.fetch;
  if (of) window.fetch = function(){ inflight++; var p = of.apply(this, arguments); Promise.resolve(p).then(function(){ inflight=Math.max(0,inflight-1); }, function(){ inflight=Math.max(0,inflight-1); }); return p; };
  var xs = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function(){ inflight++; this.addEventListener("loadend", function(){ inflight=Math.max(0,inflight-1); }); return xs.apply(this, arguments); };
  async function waitNetworkIdle(maxMs){ var t=0; while(t<maxMs){ if(inflight<=0){ await sleep(400); if(inflight<=0) return; } await sleep(120); t+=120; } }

  function vtext(el){ return (el.textContent||"").trim().replace(/\s+/g," "); }
  function roleOf(el){ var r=el.getAttribute("role"); if(r) return r; var t=el.tagName.toLowerCase();
    if(t==="button") return "button"; if(t==="a"&&el.hasAttribute("href")) return "link"; if(t==="select") return "combobox";
    if(t==="textarea") return "textbox"; if(t==="input"){ var ty=(el.getAttribute("type")||"text").toLowerCase();
      if(ty==="checkbox") return "checkbox"; if(ty==="radio") return "radio"; if(ty==="submit"||ty==="button") return "button"; return "textbox"; } return ""; }
  function nameOf(el){ var a=(el.getAttribute("aria-label")||"").trim(); if(a) return a;
    if(el.tagName==="INPUT"||el.tagName==="TEXTAREA"||el.tagName==="SELECT") return (el.getAttribute("placeholder")||el.getAttribute("name")||(el.labels&&el.labels[0]&&el.labels[0].textContent)||"").trim().slice(0,60);
    return vtext(el).slice(0,60) || (el.value||""); }
  function bySel(sel){
    try{
      if(sel.strategy==="testid") return document.querySelector('[data-testid="'+sel.value+'"],[data-test="'+sel.value+'"],[data-cy="'+sel.value+'"]');
      if(sel.strategy==="id") return document.getElementById(sel.value);
      if(sel.strategy==="name"||sel.strategy==="css") return document.querySelector(sel.value);
      if(sel.strategy==="role"){ var p=sel.value.split("|"); var role=p[0], nm=(p.slice(1).join("|"));
        var all=document.querySelectorAll('a,button,input,select,textarea,[role]');
        for(var i=0;i<all.length;i++){ if(roleOf(all[i])===role && nameOf(all[i])===nm) return all[i]; } return null; }
      if(sel.strategy==="text"){ var els=document.querySelectorAll('a,button,[role=button],summary,label');
        for(var j=0;j<els.length;j++){ if(vtext(els[j])===sel.value) return els[j]; } return null; }
    }catch(e){}
    return null;
  }
  function resolve(sels){ for(var i=0;i<sels.length;i++){ var el=bySel(sels[i]); if(el) return el; } return null; }
  function isVisible(el){ if(!el) return false; var r=el.getBoundingClientRect(); var st=getComputedStyle(el);
    return r.width>0 && r.height>0 && st.visibility!=="hidden" && st.display!=="none" && parseFloat(st.opacity||"1")>0.01; }
  async function waitActionable(sels, maxMs){
    var t=0, lastRect=null;
    while(t<maxMs){
      var el=resolve(sels);
      if(el && !el.disabled){
        try{ el.scrollIntoView({block:"center", inline:"nearest"}); }catch(e){}
        if(isVisible(el)){
          var r=el.getBoundingClientRect();
          if(lastRect && Math.abs(r.top-lastRect.top)<2 && Math.abs(r.left-lastRect.left)<2) return el;
          lastRect=r;
        } else lastRect=null;
      } else lastRect=null;
      await sleep(120); t+=120;
    }
    return null;
  }
  function setNativeValue(el, value){
    var proto = el.tagName==="TEXTAREA" ? window.HTMLTextAreaElement.prototype : (el.tagName==="SELECT" ? window.HTMLSelectElement.prototype : window.HTMLInputElement.prototype);
    var d = Object.getOwnPropertyDescriptor(proto, "value");
    if(d && d.set) d.set.call(el, value); else el.value = value;
    el.dispatchEvent(new Event("input", {bubbles:true}));
    el.dispatchEvent(new Event("change", {bubbles:true}));
  }
  async function perform(a, el){
    try{ el.scrollIntoView({block:"center"}); }catch(e){}
    await sleep(60);
    if(a.kind==="hover"){
      ["pointerover","mouseover","mouseenter","pointermove","mousemove"].forEach(function(t){
        try{ el.dispatchEvent(new MouseEvent(t, {bubbles:true, cancelable:true, view:window})); }catch(e){}
      });
      await sleep(450);
    } else if(a.kind==="input"){ setNativeValue(el, a.value!=null?a.value:""); }
    else { el.click(); }
  }
  async function runFrom(start){
    for(var i=start;i<ACTIONS.length;i++){
      var a=ACTIONS[i];
      var el=await waitActionable(a.selectors, 8000);
      if(!el){ report(i, "failed", "요소를 찾지 못함: "+(a.name||"")); sessionStorage.setItem("__replay_idx", String(ACTIONS.length)); report(-1, "failed", "중단됨", true); return; }
      try{ await perform(a, el); report(i, "passed", (a.kind==="input"?"입력: ":"클릭: ")+(a.name||"")); }
      catch(e){ report(i, "failed", String(e)); sessionStorage.setItem("__replay_idx", String(ACTIONS.length)); report(-1, "failed", "중단됨", true); return; }
      sessionStorage.setItem("__replay_idx", String(i+1));
      await waitNetworkIdle(6000);
      await sleep(300);
    }
    report(-1, "passed", "재생 완료", true);
  }
  function boot(){
    if(sessionStorage.getItem("__replay_runid") !== TOKEN){ sessionStorage.setItem("__replay_runid", TOKEN); sessionStorage.setItem("__replay_idx", "0"); }
    var idx = parseInt(sessionStorage.getItem("__replay_idx")||"0", 10);
    if(idx >= ACTIONS.length) return;
    setTimeout(function(){ runFrom(idx); }, 700);
  }
  if(document.readyState==="complete"||document.readyState==="interactive") boot();
  else window.addEventListener("DOMContentLoaded", boot);
})();"#####;
    BODY.replace("__TOKEN__", token).replace("__ACTIONS__", actions_json)
}

/// 대상 URL을 캡처 웹뷰 창("capture")으로 열고 후킹 스크립트를 주입한다.
pub fn open_capture_window(app: &AppHandle, url: &str, script: String) -> Result<tauri::WebviewWindow, String> {
    let parsed: tauri::Url = url.parse().map_err(|_| format!("잘못된 URL: {url}"))?;
    let window = WebviewWindowBuilder::new(app, "capture", WebviewUrl::External(parsed))
        .title("캡처 세션")
        .initialization_script(&script)
        .build()
        .map_err(|e| format!("캡처 창 생성 실패: {e}"))?;
    Ok(window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_embeds_token() {
        let s = hook_script("secret-tok");
        assert!(s.contains("secret-tok"));
    }

    #[test]
    fn script_sends_capture_via_ipc() {
        // mixed content 회귀: 캡처는 http POST가 아니라 IPC(invoke)로 나가야 https 페이지에서 안 막힌다
        let s = hook_script("t");
        assert!(s.contains("__TAURI_INTERNALS__"));
        assert!(s.contains(r#"invoke("capture_push""#));
        // 예전 localhost POST 방식이 남아있지 않아야 한다
        assert!(!s.contains("127.0.0.1"));
    }

    #[test]
    fn script_ignores_non_http_schemes() {
        // IPC(ipc://) 등 비-http 요청을 캡처에서 제외해 자기 재캡처를 막는다
        let s = hook_script("t");
        assert!(s.contains("/^https?:/i"));
    }

    #[test]
    fn script_hooks_fetch_and_xhr() {
        let s = hook_script("t");
        assert!(s.contains("window.fetch"));
        assert!(s.contains("XMLHttpRequest.prototype.open"));
        assert!(s.contains("XMLHttpRequest.prototype.send"));
    }

    #[test]
    fn script_truncates_at_8kb() {
        let s = hook_script("t");
        assert!(s.contains("8192"));
    }

    #[test]
    fn script_guards_response_type_before_reading_text() {
        // I1 회귀: responseType 체크 후에만 responseText 접근
        let s = hook_script("t");
        assert!(s.contains("self.responseType"));
    }

    #[test]
    fn script_reads_fetch_body_from_request_clone() {
        // I2 회귀: Request 객체 body도 잡히도록 req.clone()에서 읽음
        let s = hook_script("t");
        assert!(s.contains("req.clone().text()"));
    }
}
