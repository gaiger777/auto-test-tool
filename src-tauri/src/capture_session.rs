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
