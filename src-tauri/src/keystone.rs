use std::sync::Mutex;

pub struct KeystoneAuth {
    pub auth_url: String, // 예: http://keystone:5000 (경로 /v3/auth/tokens 는 코드가 붙임)
    pub user_name: String,
    pub user_domain: String,
    pub password: String,
    pub project_name: String,
    pub project_domain: String,
}

pub struct KeystoneClient {
    client: reqwest::Client,
    auth: KeystoneAuth,
    cached: Mutex<Option<String>>,
}

impl KeystoneClient {
    pub fn new(client: reqwest::Client, auth: KeystoneAuth) -> Self {
        Self { client, auth, cached: Mutex::new(None) }
    }

    /// 캐시된 토큰이 있으면 재사용, 없으면 발급.
    pub async fn get_token(&self) -> Result<String, String> {
        if let Some(t) = self.cached.lock().unwrap().clone() {
            return Ok(t);
        }
        self.issue_token().await
    }

    /// 캐시를 버리고 강제 재발급 (401 재시도용).
    pub async fn refresh_token(&self) -> Result<String, String> {
        *self.cached.lock().unwrap() = None;
        self.issue_token().await
    }

    async fn issue_token(&self) -> Result<String, String> {
        let body = serde_json::json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {"user": {
                        "name": self.auth.user_name,
                        "domain": {"name": self.auth.user_domain},
                        "password": self.auth.password
                    }}
                },
                "scope": {"project": {
                    "name": self.auth.project_name,
                    "domain": {"name": self.auth.project_domain}
                }}
            }
        });
        let url = format!("{}/v3/auth/tokens", self.auth.auth_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Keystone 접속 실패: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Keystone 인증 실패: HTTP {}", resp.status().as_u16()));
        }
        let token = resp
            .headers()
            .get("X-Subject-Token")
            .and_then(|v| v.to_str().ok())
            .ok_or("응답에 X-Subject-Token 헤더가 없음")?
            .to_string();
        *self.cached.lock().unwrap() = Some(token.clone());
        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn auth(url: &str) -> KeystoneAuth {
        KeystoneAuth {
            auth_url: url.to_string(),
            user_name: "admin".into(),
            user_domain: "Default".into(),
            password: "pw".into(),
            project_name: "admin".into(),
            project_domain: "Default".into(),
        }
    }

    #[tokio::test]
    async fn issues_and_caches_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(201).insert_header("X-Subject-Token", "tok-1"))
            .expect(1) // 캐시 덕에 딱 1번만 호출돼야 함
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        assert_eq!(ks.get_token().await.unwrap(), "tok-1");
        assert_eq!(ks.get_token().await.unwrap(), "tok-1");
    }

    #[tokio::test]
    async fn refresh_reissues_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(201).insert_header("X-Subject-Token", "tok-2"))
            .expect(2)
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        ks.get_token().await.unwrap();
        assert_eq!(ks.refresh_token().await.unwrap(), "tok-2");
    }

    #[tokio::test]
    async fn reports_auth_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/auth/tokens"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let ks = KeystoneClient::new(reqwest::Client::new(), auth(&server.uri()));
        assert!(ks.get_token().await.unwrap_err().contains("401"));
    }
}
