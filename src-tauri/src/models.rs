use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type Vars = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StepDef {
    pub name: String,
    #[serde(default)]
    pub cleanup: bool,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    HttpCall {
        method: String,
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        expect_status: Option<u16>,
        #[serde(default)]
        captures: Vec<Capture>,
    },
    WaitEvent {
        event_type: String,
        #[serde(default)]
        conditions: Vec<Condition>,
        timeout_secs: u64,
    },
    Assert {
        left: String,
        op: AssertOp,
        right: String,
    },
    Sleep {
        seconds: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Capture {
    pub var: String,
    pub json_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Condition {
    pub json_path: String,
    pub equals: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AssertOp {
    Eq,
    Contains,
    Regex,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_roundtrips_through_json() {
        let json = r#"{
          "name": "인스턴스 생성 테스트",
          "steps": [
            {
              "name": "서버 생성",
              "type": "http_call",
              "method": "POST",
              "url": "{{base_url.nova}}/servers",
              "headers": {"X-Auth-Token": "{{auth_token}}"},
              "body": "{\"server\": {}}",
              "expect_status": 202,
              "captures": [{"var": "server_id", "json_path": "$.server.id"}]
            },
            {
              "name": "생성 완료 대기",
              "type": "wait_event",
              "event_type": "compute.instance.create.end",
              "conditions": [{"json_path": "$.payload.instance_id", "equals": "{{server_id}}"}],
              "timeout_secs": 300
            },
            {
              "name": "서버 삭제",
              "cleanup": true,
              "type": "http_call",
              "method": "DELETE",
              "url": "{{base_url.nova}}/servers/{{server_id}}"
            },
            {"name": "검증", "type": "assert", "left": "{{server_id}}", "op": "regex", "right": "^[0-9a-f-]+$"},
            {"name": "잠깐 대기", "type": "sleep", "seconds": 3}
          ]
        }"#;
        let s: Scenario = serde_json::from_str(json).unwrap();
        assert_eq!(s.steps.len(), 5);
        assert!(s.steps[2].cleanup);
        assert!(!s.steps[0].cleanup);
        let back = serde_json::to_string(&s).unwrap();
        let s2: Scenario = serde_json::from_str(&back).unwrap();
        assert_eq!(s, s2);
    }
}
