use crate::matcher::json_path_str;
use crate::models::{Capture, Vars};
use std::collections::HashMap;

#[derive(Debug)]
pub struct HttpResult {
    pub status: u16,
    pub body: String,
}

/// HTTP 요청을 실행하고 상태코드와 바디를 돌려준다.
pub async fn execute(
    client: &reqwest::Client,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: Option<&str>,
) -> Result<HttpResult, String> {
    let m = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| format!("잘못된 HTTP 메서드: {method}"))?;
    let mut req = client.request(m, url);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    if let Some(b) = body {
        // 사용자가 헤더로 Content-Type을 지정했으면 기본값(JSON)을 덮어쓰지 않는다
        let has_content_type = headers.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
        if !has_content_type {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(b.to_string());
    }
    let resp = req.send().await.map_err(|e| format!("HTTP 요청 실패: {e}"))?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| format!("응답 읽기 실패: {e}"))?;
    Ok(HttpResult { status, body })
}

/// 응답 바디에서 JSONPath로 값을 뽑아 vars에 넣는다.
pub fn capture_vars(body: &str, captures: &[Capture], vars: &mut Vars) -> Result<(), String> {
    if captures.is_empty() {
        return Ok(());
    }
    let json: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("응답이 JSON이 아님: {e}"))?;
    for c in captures {
        let v = json_path_str(&json, &c.json_path)
            .map_err(|e| format!("캡처 '{}' 실패: {e}", c.var))?;
        vars.insert(c.var.clone(), v);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn sends_request_with_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/servers"))
            .and(header("X-Auth-Token", "tok"))
            .and(body_string(r#"{"server":{}}"#))
            .respond_with(ResponseTemplate::new(202).set_body_string(r#"{"server":{"id":"abc-123"}}"#))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let headers = HashMap::from([("X-Auth-Token".to_string(), "tok".to_string())]);
        let res = execute(&client, "POST", &format!("{}/servers", server.uri()), &headers, Some(r#"{"server":{}}"#))
            .await
            .unwrap();
        assert_eq!(res.status, 202);
        assert!(res.body.contains("abc-123"));
    }

    #[tokio::test]
    async fn respects_user_content_type() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let headers = HashMap::from([(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        )]);
        let res = execute(&client, "POST", &format!("{}/x", server.uri()), &headers, Some("a=1"))
            .await
            .unwrap();
        assert_eq!(res.status, 200);

        // 중복 헤더가 실리지 않았는지 실제 수신 요청으로 검증
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let values: Vec<_> = requests[0].headers.get_all("content-type").iter().collect();
        assert_eq!(values.len(), 1, "Content-Type 헤더가 정확히 1개여야 함");
        assert_eq!(values[0], "application/x-www-form-urlencoded");
    }

    #[tokio::test]
    async fn rejects_invalid_method() {
        let client = reqwest::Client::new();
        let err = execute(&client, "굿", "http://localhost", &HashMap::new(), None)
            .await
            .unwrap_err();
        assert!(err.contains("메서드"));
    }

    #[test]
    fn captures_variables_from_body() {
        let mut vars = Vars::new();
        let caps = vec![Capture { var: "server_id".into(), json_path: "$.server.id".into() }];
        capture_vars(r#"{"server":{"id":"abc-123"}}"#, &caps, &mut vars).unwrap();
        assert_eq!(vars["server_id"], "abc-123");
    }

    #[test]
    fn capture_fails_on_missing_path() {
        let mut vars = Vars::new();
        let caps = vec![Capture { var: "x".into(), json_path: "$.nope".into() }];
        assert!(capture_vars(r#"{}"#, &caps, &mut vars).is_err());
    }
}
