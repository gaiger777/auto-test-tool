use serde_json::Value;
use serde_json_path::JsonPath;

/// 사용자가 `$` 없이 `payload.hostname` / `hostname` 처럼 넣어도 되도록 `$.`를 보정한다.
fn norm_path(p: &str) -> String {
    let t = p.trim();
    if t.starts_with('$') {
        t.to_string()
    } else {
        format!("$.{}", t.trim_start_matches('.'))
    }
}

/// JSONPath로 값을 찾아 문자열로 돌려준다. 문자열이면 그대로, 아니면 JSON 직렬화.
pub fn json_path_str(value: &Value, path: &str) -> Result<String, String> {
    let np = norm_path(path);
    let p = JsonPath::parse(&np).map_err(|e| format!("잘못된 JSONPath '{path}': {e}"))?;
    let node = p
        .query(value)
        .exactly_one()
        .map_err(|_| format!("JSONPath '{path}' 결과가 정확히 1개가 아님"))?;
    Ok(match node {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// 스텝 시작 시 1회 파싱해두는 wait_event 조건.
#[derive(Debug)]
pub struct CompiledCondition {
    path: JsonPath,
    expected: String,
}

/// (json_path 원문, 기대값) 쌍들을 파싱한다. 문법 오류는 즉시 Err — 스텝 시작 시점에 표면화된다.
pub fn compile_conditions(conds: &[(String, String)]) -> Result<Vec<CompiledCondition>, String> {
    conds
        .iter()
        .map(|(p, expected)| {
            let path = JsonPath::parse(&norm_path(p)).map_err(|e| format!("잘못된 JSONPath '{p}': {e}"))?;
            Ok(CompiledCondition {
                path,
                expected: expected.clone(),
            })
        })
        .collect()
}

/// notification 이벤트가 event_type과 사전 컴파일된 조건들에 모두 일치하는가.
pub fn matches(event: &Value, event_type: &str, conditions: &[CompiledCondition]) -> bool {
    if event.get("event_type").and_then(|v| v.as_str()) != Some(event_type) {
        return false;
    }
    conditions.iter().all(|c| {
        c.path
            .query(event)
            .exactly_one()
            .map(|node| match node {
                Value::String(s) => s == &c.expected,
                // clippy 제안(*other == c.expected)은 비문자열 Value에서 항상 false가 되므로 적용 금지
                #[allow(clippy::cmp_owned)]
                other => other.to_string() == c.expected,
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> Value {
        json!({
            "event_type": "compute.instance.create.end",
            "payload": {"instance_id": "abc-123", "state": "active"}
        })
    }

    #[test]
    fn matches_event_type_and_conditions() {
        let conds = vec![("$.payload.instance_id".to_string(), "abc-123".to_string())];
        let compiled = compile_conditions(&conds).unwrap();
        assert!(matches(&event(), "compute.instance.create.end", &compiled));
    }

    #[test]
    fn rejects_wrong_event_type() {
        assert!(!matches(&event(), "compute.instance.delete.end", &[]));
    }

    #[test]
    fn rejects_condition_mismatch() {
        let conds = vec![("$.payload.instance_id".to_string(), "other".to_string())];
        let compiled = compile_conditions(&conds).unwrap();
        assert!(!matches(&event(), "compute.instance.create.end", &compiled));
    }

    #[test]
    fn rejects_missing_path() {
        let conds = vec![("$.payload.nope".to_string(), "x".to_string())];
        let compiled = compile_conditions(&conds).unwrap();
        assert!(!matches(&event(), "compute.instance.create.end", &compiled));
    }

    #[test]
    fn json_path_str_stringifies_non_strings() {
        let v = json!({"n": 42});
        assert_eq!(json_path_str(&v, "$.n").unwrap(), "42");
    }

    #[test]
    fn compile_rejects_invalid_path() {
        let err = compile_conditions(&[("not a path".into(), "x".into())]).unwrap_err();
        assert!(err.contains("not a path"));
    }

    #[test]
    fn json_path_str_errors_on_invalid_path() {
        assert!(json_path_str(&json!({}), "not a path").is_err());
    }

    #[test]
    fn matches_non_string_condition_value() {
        let e = serde_json::json!({"event_type": "t", "payload": {"n": 42}});
        let conds = compile_conditions(&[("$.payload.n".to_string(), "42".to_string())]).unwrap();
        assert!(matches(&e, "t", &conds));
    }
}
