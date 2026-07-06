use serde_json::Value;

/// JSONPath로 값을 찾아 문자열로 돌려준다. 문자열이면 그대로, 아니면 JSON 직렬화.
pub fn json_path_str(value: &Value, path: &str) -> Result<String, String> {
    let p = serde_json_path::JsonPath::parse(path)
        .map_err(|e| format!("잘못된 JSONPath '{path}': {e}"))?;
    let node = p
        .query(value)
        .exactly_one()
        .map_err(|_| format!("JSONPath '{path}' 결과가 정확히 1개가 아님"))?;
    Ok(match node {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// notification 이벤트가 event_type과 (이미 렌더링된) 조건들에 모두 일치하는가.
/// conditions: (json_path, 기대값) 쌍
pub fn matches(event: &Value, event_type: &str, conditions: &[(String, String)]) -> bool {
    if event.get("event_type").and_then(|v| v.as_str()) != Some(event_type) {
        return false;
    }
    conditions
        .iter()
        .all(|(path, expected)| json_path_str(event, path).as_deref() == Ok(expected))
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
        assert!(matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn rejects_wrong_event_type() {
        assert!(!matches(&event(), "compute.instance.delete.end", &[]));
    }

    #[test]
    fn rejects_condition_mismatch() {
        let conds = vec![("$.payload.instance_id".to_string(), "other".to_string())];
        assert!(!matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn rejects_missing_path() {
        let conds = vec![("$.payload.nope".to_string(), "x".to_string())];
        assert!(!matches(&event(), "compute.instance.create.end", &conds));
    }

    #[test]
    fn json_path_str_stringifies_non_strings() {
        let v = json!({"n": 42});
        assert_eq!(json_path_str(&v, "$.n").unwrap(), "42");
    }
}
