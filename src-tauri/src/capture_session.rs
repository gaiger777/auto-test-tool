use tauri::{AppHandle, WebviewUrl, WebviewWindowBuilder};

/// 캡처 웹뷰에 주입할 fetch/XHR 후킹 스크립트를 만든다.
/// 포트와 토큰이 박힌 스크립트가 캡처를 http://127.0.0.1:{port}/capture?token={token} 로 전송한다.
/// 전송은 단순 요청(text/plain 바디, 쿼리 토큰, 응답 무시)이라 CORS 프리플라이트가 없다.
pub fn hook_script(port: u16, token: &str) -> String {
    // response_body 상한 8KB
    format!(
        r#"(function() {{
  var ENDPOINT = "http://127.0.0.1:{port}/capture?token={token}";
  var seq = 0;
  function send(call) {{
    try {{ fetch(ENDPOINT, {{ method: "POST", body: JSON.stringify(call), keepalive: true }}).catch(function(){{}}); }} catch (e) {{}}
  }}
  function truncate(s) {{ return (typeof s === "string" && s.length > 8192) ? s.slice(0, 8192) : s; }}
  function headersToObj(h) {{
    var o = {{}};
    if (h && typeof h.forEach === "function") h.forEach(function(v, k) {{ o[k] = v; }});
    return o;
  }}

  var origFetch = window.fetch;
  window.fetch = function(input, init) {{
    var req;
    try {{ req = new Request(input, init); }} catch (e) {{ return origFetch.apply(this, arguments); }}
    var reqHeaders = headersToObj(req.headers);
    var id = "c" + (++seq);
    var bodyPromise = init && init.body != null ? Promise.resolve(String(init.body)) : Promise.resolve(null);
    return origFetch.apply(this, arguments).then(function(resp) {{
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
        var abs;
        try {{ abs = new URL(self.__cap.url, location.href).href; }} catch (e) {{ abs = self.__cap.url; }}
        send({{ id: "c" + (++seq), method: self.__cap.method, url: abs, request_headers: self.__cap.headers,
                request_body: body != null ? String(body) : null, status: self.status,
                response_body: truncate(typeof self.responseText === "string" ? self.responseText : null), timestamp: Date.now() }});
      }});
    }}
    return XS.apply(this, arguments);
  }};
}})();"#
    )
}

/// 대상 URL을 캡처 웹뷰 창("capture")으로 열고 후킹 스크립트를 주입한다.
pub fn open_capture_window(app: &AppHandle, url: &str, script: String) -> Result<(), String> {
    let parsed: tauri::Url = url.parse().map_err(|_| format!("잘못된 URL: {url}"))?;
    WebviewWindowBuilder::new(app, "capture", WebviewUrl::External(parsed))
        .title("캡처 세션")
        .initialization_script(&script)
        .build()
        .map_err(|e| format!("캡처 창 생성 실패: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_embeds_port_and_token() {
        let s = hook_script(54321, "secret-tok");
        assert!(s.contains("54321"));
        assert!(s.contains("secret-tok"));
    }

    #[test]
    fn script_hooks_fetch_and_xhr() {
        let s = hook_script(1, "t");
        assert!(s.contains("window.fetch"));
        assert!(s.contains("XMLHttpRequest.prototype.open"));
        assert!(s.contains("XMLHttpRequest.prototype.send"));
    }

    #[test]
    fn script_truncates_at_8kb() {
        let s = hook_script(1, "t");
        assert!(s.contains("8192"));
    }
}
