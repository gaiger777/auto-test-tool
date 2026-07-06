use crate::models::Vars;

/// 문자열 안의 {{var}} 를 vars 값으로 치환한다. 미정의 변수는 에러.
pub fn render(input: &str, vars: &Vars) -> Result<String, String> {
    let re = regex::Regex::new(r"\{\{\s*([\w.]+)\s*\}\}").unwrap();
    let mut missing: Option<String> = None;
    let out = re
        .replace_all(input, |caps: &regex::Captures| {
            let key = &caps[1];
            match vars.get(key) {
                Some(v) => v.clone(),
                None => {
                    missing = Some(format!("정의되지 않은 변수: {key}"));
                    String::new()
                }
            }
        })
        .into_owned();
    match missing {
        Some(e) => Err(e),
        None => Ok(out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars() -> Vars {
        HashMap::from([
            ("server_id".to_string(), "abc-123".to_string()),
            ("base_url.nova".to_string(), "http://nova:8774/v2.1".to_string()),
        ])
    }

    #[test]
    fn substitutes_variables() {
        assert_eq!(
            render("{{base_url.nova}}/servers/{{server_id}}", &vars()).unwrap(),
            "http://nova:8774/v2.1/servers/abc-123"
        );
    }

    #[test]
    fn allows_whitespace_inside_braces() {
        assert_eq!(render("{{ server_id }}", &vars()).unwrap(), "abc-123");
    }

    #[test]
    fn errors_on_undefined_variable() {
        let err = render("{{nope}}", &vars()).unwrap_err();
        assert!(err.contains("nope"));
    }

    #[test]
    fn passes_through_plain_text() {
        assert_eq!(render("no vars here", &vars()).unwrap(), "no vars here");
    }
}
