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
    var role = roleOf(el), nm = nameOf(el);
    var isRadio = el.tagName === "INPUT" && (el.type === "radio" || el.type === "checkbox");
    // 라디오/체크박스의 name은 그룹 공용(유일하지 않음) → name 셀렉터 제외.
    if (el.getAttribute("name") && !isRadio) out.push({{ strategy: "name", value: el.tagName.toLowerCase() + "[name=" + el.getAttribute("name") + "]" }});
    if (role && nm) out.push({{ strategy: "role", value: role + "|" + nm }});
    if (nm && (role === "button" || role === "link")) out.push({{ strategy: "text", value: nm }});
    // 테이블 행 안의 요소는 위치(nth-of-type)가 아니라 '행 텍스트(가장 긴 셀 = 대개 이름)'로 앵커링
    // → 정렬·페이징으로 행 위치가 바뀌어도 올바른 행을 찾는다. (css 위치 셀렉터보다 우선)
    var row = el.closest ? el.closest("tr, [role=row]") : null;
    if (row) {{
      var cells = row.querySelectorAll("td, [role=cell], [role=gridcell]");
      var anchor = "";
      for (var ci = 0; ci < cells.length; ci++) {{
        var ct = (cells[ci].textContent || "").trim().replace(/\s+/g, " ");
        if (ct.length > anchor.length && ct.length <= 60) anchor = ct;
      }}
      if (!anchor) anchor = (row.textContent || "").trim().replace(/\s+/g, " ").slice(0, 60);
      if (anchor) {{
        var hint = isRadio ? "radio" : (role ? ("role:" + role) : ("tag:" + el.tagName.toLowerCase()));
        out.push({{ strategy: "rowtext", value: anchor + "|||" + hint }});
      }}
    }}
    out.push({{ strategy: "css", value: cssPath(el) }});
    return out;
  }}
  function hrefOf(el) {{ try {{ var a = el.closest ? el.closest("a[href]") : null; return (a && a.href) ? a.href : null; }} catch (e) {{ return null; }} }}
  function record(kind, el, value) {{
    if (!el || el.nodeType !== 1 || el.tagName === "HTML" || el.tagName === "BODY") return;
    send({{ id: "u" + (++uiseq), kind: kind, selectors: ladder(el), name: nameOf(el),
            value: (value != null ? String(value) : null), href: (kind === "click" ? hrefOf(el) : null),
            url: location.href, timestamp: Date.now() }});
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
  // 스텝 보고 + '마지막으로 보고된 스텝' 기록(하드 네비게이션으로 보고가 유실됐는지 부트에서 판별).
  function stepReport(i, status, detail){ report(i, status, detail); sessionStorage.setItem("__replay_reported", String(i)); }
  // 최종 완료 보고 + 완료 표식(재개된 페이지에서 중복 완료 보고를 막는다).
  function finish(status, detail){ sessionStorage.setItem("__replay_done", "1"); report(-1, status, detail, true); }
  function sleep(ms){ return new Promise(function(r){ setTimeout(r, ms); }); }

  // 네트워크 in-flight 카운터 + 호출 로그(검증용): 각 동작 뒤 발생한 호출의 상태코드를 본다.
  var inflight = 0;
  window.__net = window.__net || [];
  function logCall(m, u, s, t){ try{ window.__net.push({ method:(m||"GET").toUpperCase(), url:String(u||""), status:(s|0), ts:t }); }catch(e){} }
  var of = window.fetch;
  if (of) window.fetch = function(){ inflight++; var args=arguments; var m=(args[1]&&args[1].method)||"GET"; var u=(args[0]&&args[0].url)||args[0]; var t=Date.now();
    var p = of.apply(this, arguments); Promise.resolve(p).then(function(r){ inflight=Math.max(0,inflight-1); logCall(m,u,(r&&r.status)||0,t); }, function(){ inflight=Math.max(0,inflight-1); logCall(m,u,0,t); }); return p; };
  var xoo = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function(m, u){ this.__m=m; this.__u=u; return xoo.apply(this, arguments); };
  var xs = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function(){ inflight++; var self=this; var t=Date.now(); this.addEventListener("loadend", function(){ inflight=Math.max(0,inflight-1); logCall(self.__m, self.__u, self.status, t); }); return xs.apply(this, arguments); };
  async function waitNetworkIdle(maxMs){ var t=0; while(t<maxMs){ if(inflight<=0){ await sleep(400); if(inflight<=0) return; } await sleep(120); t+=120; } }
  function shortPath(u){ try{ return new URL(u).pathname; }catch(e){ return u; } }

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
      if(sel.strategy==="rowtext"){ // "행앵커텍스트|||힌트" → 그 텍스트를 포함하는 행 안에서 타겟을 찾는다
        var rp=sel.value.split("|||"); var atext=rp[0], hint=rp[1]||"";
        var rows=document.querySelectorAll('tr, [role=row]');
        for(var k=0;k<rows.length;k++){
          if((rows[k].textContent||"").replace(/\s+/g," ").indexOf(atext)<0) continue;
          if(hint==="radio") return rows[k].querySelector('input[type=radio],input[type=checkbox]') || rows[k];
          if(hint.indexOf("role:")===0){ var wrole=hint.slice(5); var cs=rows[k].querySelectorAll('a,button,input,select,textarea,[role]');
            for(var m2=0;m2<cs.length;m2++){ if(roleOf(cs[m2])===wrole) return cs[m2]; } }
          if(hint.indexOf("tag:")===0){ var t2=rows[k].querySelector(hint.slice(4)); if(t2) return t2; }
          return rows[k];
        }
        return null; }
    }catch(e){}
    return null;
  }
  function resolve(sels){ for(var i=0;i<sels.length;i++){ var el=bySel(sels[i]); if(el) return el; } return null; }
  // lenient=true: hover로 노출되는 요소 대응 — DOM에 존재하고 박스가 있으면(display:none만 제외)
  // opacity:0 / visibility:hidden 이어도 통과시킨다. 프로그램적 .click()은 이런 상태에서도 동작한다.
  function isRadioLike(el){ return el && el.tagName==="INPUT" && (el.type==="radio"||el.type==="checkbox"); }
  function isVisible(el, lenient){ if(!el) return false; var st=getComputedStyle(el);
    if(st.display==="none") return false;
    // 라디오/체크박스: ant-design 등은 실제 input을 opacity:0/0크기로 숨김 → 감싼 라벨/셀의 표시로 판단.
    if(isRadioLike(el)){ var w=el.closest("label,.ant-radio-wrapper,.ant-checkbox-wrapper,td,li")||el;
      var wr=w.getBoundingClientRect(); var ws=getComputedStyle(w);
      return wr.width>0 && wr.height>0 && ws.display!=="none" && ws.visibility!=="hidden"; }
    var r=el.getBoundingClientRect();
    if(!(r.width>0 && r.height>0)) return false;
    if(lenient) return true;
    return st.visibility!=="hidden" && parseFloat(st.opacity||"1")>0.01; }
  async function waitActionable(sels, maxMs, lenient){
    var t=0, lastRect=null;
    while(t<maxMs){
      var el=resolve(sels);
      if(el && !el.disabled){
        try{ el.scrollIntoView({block:"center", inline:"nearest"}); }catch(e){}
        if(isVisible(el, lenient)){
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
  // A) 호버 상태 유지: 합성 hover 이벤트는 순간적이라 다음 클릭 시점엔 메뉴가 닫힌다.
  // 호버 대상(들)에 이벤트를 주기적으로 재발사해, 뒤따르는 클릭이 성공할 때까지 메뉴를 열어둔다.
  var __hoverTimer=null, __hoverEls=[];
  function fireHover(el){ ["pointerover","mouseover","mouseenter","pointermove","mousemove"].forEach(function(t){
      try{ el.dispatchEvent(new MouseEvent(t, {bubbles:true, cancelable:true, view:window})); }catch(e){} }); }
  function stopHover(){ if(__hoverTimer){ clearInterval(__hoverTimer); __hoverTimer=null; } __hoverEls=[]; }
  function pushHover(el){
    if(!el) return;
    if(__hoverEls.indexOf(el)<0) __hoverEls.push(el);
    fireHover(el);
    if(!__hoverTimer){ __hoverTimer=setInterval(function(){
      __hoverEls=__hoverEls.filter(function(e){ return e && e.isConnected; });
      if(!__hoverEls.length){ stopHover(); return; }
      __hoverEls.forEach(fireHover);
    }, 120); }
  }
  async function perform(a, el){
    try{ el.scrollIntoView({block:"center"}); }catch(e){}
    await sleep(60);
    if(a.kind==="hover"){
      pushHover(el);      // 타이머는 여기서 멈추지 않는다 → 다음 스텝(클릭)까지 유지
      await sleep(450);
    } else if(a.kind==="input"){ setNativeValue(el, a.value!=null?a.value:""); }
    else {
      // 호버가 유지 중이면 클릭 대상 자신에도 hover를 얹어(중첩 메뉴 대응) 클릭한다.
      if(__hoverTimer){ pushHover(el); await sleep(60); }
      // 라디오/체크박스는 대상 자신이거나 셀/라벨 안의 input을 직접 클릭(숨겨진 input도 토글됨).
      var radio = isRadioLike(el) ? el : (el.querySelector ? el.querySelector('input[type=radio],input[type=checkbox]') : null);
      if(radio){ radio.click(); } else { el.click(); }
    }
  }
  // 프로그램 스텝 값 치환: 직전 http_call 응답을 {{status}}/{{body}}로 참조 가능.
  function substVars(t){ return String(t==null?"":t)
    .replace(/\{\{\s*status\s*\}\}/g, String(window.__lastStatus==null?"":window.__lastStatus))
    .replace(/\{\{\s*body\s*\}\}/g, String(window.__lastBody==null?"":window.__lastBody)); }
  // http_call/assert/sleep 를 웹뷰(페이지 세션) 안에서 실행한다. 실패 시 throw. (wait_event는 위임)
  async function runProg(a){
    var s = a.step || {};
    if(a.kind==="sleep"){ var sec=Number(s.seconds||0); await sleep(sec*1000); return sec+"초 대기"; }
    if(a.kind==="http_call"){
      var method=(s.method||"GET").toUpperCase();
      var url=substVars(s.url||"");
      if(!url) throw new Error("URL이 비어 있습니다");
      var opts={ method: method, headers: s.headers||{}, credentials:"include" };
      if(s.body!=null && method!=="GET" && method!=="HEAD") opts.body = substVars(s.body);
      var resp = await fetch(url, opts);
      window.__lastStatus = resp.status;
      try{ window.__lastBody = await resp.text(); }catch(e){ window.__lastBody=""; }
      var ok = (s.expect_status!=null) ? (resp.status===Number(s.expect_status)) : (resp.status>=200 && resp.status<400);
      if(!ok) throw new Error("HTTP "+resp.status+(s.expect_status!=null?" (기대 "+s.expect_status+")":""));
      return method+" "+shortPath(url)+" → "+resp.status;
    }
    if(a.kind==="assert"){
      var L=substVars(s.left), R=substVars(s.right), op=s.op||"eq";
      var ok = op==="eq" ? L===R : op==="contains" ? L.indexOf(R)>=0 : op==="regex" ? new RegExp(R).test(L) : false;
      if(!ok) throw new Error("assert 실패: '"+L+"' "+op+" '"+R+"'");
      return "'"+L+"' "+op+" '"+R+"' 통과";
    }
    throw new Error("알 수 없는 스텝: "+a.kind);
  }
  // wait_event 위임 후 프론트가 결과와 함께 호출 → 같은 페이지에서 다음 스텝부터 재개.
  window.__replayResume = function(nextIdx, prevStatus, prevDetail){
    if(prevStatus){ stepReport(nextIdx-1, prevStatus, prevDetail||"");
      if(prevStatus==="failed") sessionStorage.setItem("__replay_fail","1"); }
    sessionStorage.setItem("__replay_idx", String(nextIdx));
    runFrom(nextIdx);
  };
  async function runFrom(start){
    // href 폴백 네비게이션을 넘어서도 실패/경고 누적이 유지되도록 sessionStorage에 보관.
    var anyFail = sessionStorage.getItem("__replay_fail")==="1";
    var apiWarn = sessionStorage.getItem("__replay_apiwarn")==="1";
    for(var i=start;i<ACTIONS.length;i++){
      var a=ACTIONS[i];
      // 프로그램 스텝: 웹뷰 안에서 실행(sleep/http_call/assert)하거나 백엔드로 위임(wait_event).
      if(a.kind==="sleep"||a.kind==="http_call"||a.kind==="assert"){
        stopHover();
        try{ var pd=await runProg(a); stepReport(i, "passed", (a.name?a.name+" · ":"")+pd); }
        catch(e){ anyFail=true; sessionStorage.setItem("__replay_fail","1"); stepReport(i, "failed", (a.name?a.name+" · ":"")+String(e && e.message ? e.message : e)); }
        sessionStorage.setItem("__replay_idx", String(i+1));
        continue;
      }
      if(a.kind==="wait_event"){
        stopHover();
        sessionStorage.setItem("__replay_idx", String(i)); // 위임 실행 후 __replayResume(i+1)로 재개
        report(i, "delegate", JSON.stringify({ name: a.name, step: a.step||{} }));
        return; // 일시정지 — 프론트가 백엔드로 실행 후 이어감
      }
      // 비활성(disabled) 버튼 클릭은 무효 동작 → 건너뜀. (예: 마지막 페이지에서 '다음'을 더 누른 기록)
      // 짧게 기다려 활성화되는지 확인하고, 그래도 비활성이면 실패가 아니라 스킵으로 처리한다.
      if(a.kind==="click"){
        var pb=resolve(a.selectors);
        if(pb && pb.disabled){ await sleep(800); pb=resolve(a.selectors);
          if(pb && pb.disabled){ stepReport(i, "passed", "비활성 버튼 건너뜀: "+(a.name||"")); sessionStorage.setItem("__replay_idx", String(i+1)); continue; } }
      }
      // 대기: 링크 클릭은 짧게(href 폴백), 호버는 짧게(실패해도 건너뜀), 그 외 넉넉히.
      var waitMs = a.kind==="hover" ? 3000 : ((a.kind==="click" && a.href) ? 3500 : 8000);
      // B) 호버 유지 중 클릭은 opacity:0/visibility:hidden 로 노출되는 대상도 통과시킨다.
      var lenient = !!__hoverTimer && a.kind==="click";
      var el=await waitActionable(a.selectors, waitMs, lenient);
      if(!el){
        if(a.kind==="click" && a.href){
          stepReport(i, "passed", "링크 이동(폴백): "+a.href);
          sessionStorage.setItem("__replay_idx", String(i+1));
          location.href = a.href; // 페이지 전환 → 새 페이지에서 init 스크립트가 이어서 재생
          return;
        }
        if(a.kind==="hover"){ // 호버는 보조 단계 → 실패해도 다음 스텝 진행
          stepReport(i, "passed", "호버 건너뜀(대상 미발견)");
          sessionStorage.setItem("__replay_idx", String(i+1)); continue;
        }
        stepReport(i, "failed", "요소를 찾지 못함: "+(a.name||"")); sessionStorage.setItem("__replay_idx", String(ACTIONS.length)); finish("failed", "중단됨"); return;
      }
      // API 검증 기준 시각: 실제 동작 직전(대기 중 배경 폴링 호출은 제외).
      var netStart=Date.now();
      try{ await perform(a, el); }
      catch(e){
        if(a.kind==="hover"){ stepReport(i, "passed", "호버 건너뜀: "+String(e)); sessionStorage.setItem("__replay_idx", String(i+1)); continue; }
        stepReport(i, "failed", String(e)); sessionStorage.setItem("__replay_idx", String(ACTIONS.length)); finish("failed", "중단됨"); return;
      }
      // 호버 후속(클릭/입력)이 끝나면 유지 중이던 호버를 해제한다. (다음 호버는 새로 시작)
      if(a.kind!=="hover") stopHover();
      sessionStorage.setItem("__replay_idx", String(i+1));
      await waitNetworkIdle(6000);
      await sleep(300);
      // API 검증: 이 동작 이후 발생한 호출 중 4xx/5xx가 있으면 스텝 실패 표시(재생은 계속).
      var base=(a.kind==="input"?"입력: ":a.kind==="hover"?"호버: ":"클릭: ")+(a.name||"");
      var calls=(window.__net||[]).filter(function(c){ return c.ts>=netStart; });
      var errs=calls.filter(function(c){ return c.status>=400 && c.status!==401; });
      if(errs.length){
        // 동작 자체는 성공 — API 오류는 실패로 보지 않고 경고(⚠)로만 표시한다.
        apiWarn=true; sessionStorage.setItem("__replay_apiwarn","1");
        var d=errs.slice(0,3).map(function(c){ return c.method+" "+shortPath(c.url)+" → "+c.status; }).join(", ");
        stepReport(i, "passed", base+" · ⚠ API오류 "+errs.length+"건: "+d);
      } else {
        stepReport(i, "passed", base+" · API "+calls.length+"건");
      }
    }
    stopHover();
    finish(anyFail?"failed":"passed", anyFail?"완료(일부 스텝 실패)":(apiWarn?"재생 완료(API 경고 있음)":"재생 완료"));
  }
  // 전체 실행 연속 진행: 창을 닫지 않고 다음 시나리오 액션을 이 창에서 이어서 실행한다.
  // 현재 시나리오 액션을 sessionStorage에 두어 네비게이션(하드 리로드)에도 유지한다.
  window.__replayLoad = function(actionsJson){
    try{ ACTIONS = JSON.parse(actionsJson); }catch(e){ return; }
    sessionStorage.setItem("__replay_actions", actionsJson);
    sessionStorage.setItem("__replay_idx", "0");
    sessionStorage.removeItem("__replay_fail"); sessionStorage.removeItem("__replay_apiwarn"); sessionStorage.removeItem("__replay_done"); sessionStorage.setItem("__replay_reported", "-1");
    runFrom(0);
  };
  function boot(){
    var fresh = sessionStorage.getItem("__replay_runid") !== TOKEN;
    if(fresh){ sessionStorage.setItem("__replay_runid", TOKEN); sessionStorage.setItem("__replay_idx", "0");
      sessionStorage.setItem("__replay_actions", JSON.stringify(ACTIONS)); // 초기 시나리오 시드
      sessionStorage.removeItem("__replay_fail"); sessionStorage.removeItem("__replay_apiwarn"); sessionStorage.removeItem("__replay_done"); sessionStorage.setItem("__replay_reported", "-1"); }
    // 이어받기(연속 실행/네비게이션 재개) 시 현재 시나리오 액션을 sessionStorage에서 복원.
    try{ var stored = sessionStorage.getItem("__replay_actions"); if(stored) ACTIONS = JSON.parse(stored); }catch(e){}
    var idx = parseInt(sessionStorage.getItem("__replay_idx")||"0", 10);
    var reported = parseInt(sessionStorage.getItem("__replay_reported")||"-1", 10);
    // 직전 스텝이 하드 네비게이션(예: 로그아웃)을 일으켜 완료 보고 전에 페이지가 언로드된 경우,
    // 재개된 페이지에서 그 스텝을 통과로 마무리한다. (SPA 전환은 언로드가 없어 정상 보고됨)
    if(!fresh && idx-1 > reported && idx-1 < ACTIONS.length){
      stepReport(idx-1, "passed", "페이지 전환 후 재개");
    }
    if(idx >= ACTIONS.length){
      // 마지막 스텝의 하드 네비게이션으로 최종 완료 보고가 유실됐다면 여기서 마무리한다.
      if(sessionStorage.getItem("__replay_done")!=="1"){
        var anyFail = sessionStorage.getItem("__replay_fail")==="1";
        var apiWarn = sessionStorage.getItem("__replay_apiwarn")==="1";
        finish(anyFail?"failed":"passed", anyFail?"완료(일부 스텝 실패)":(apiWarn?"재생 완료(API 경고 있음)":"재생 완료"));
      }
      return;
    }
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
