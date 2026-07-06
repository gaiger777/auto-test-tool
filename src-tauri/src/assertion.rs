use crate::models::AssertOp;

pub fn check(left: &str, op: &AssertOp, right: &str) -> Result<(), String> {
    let ok = match op {
        AssertOp::Eq => left == right,
        AssertOp::Contains => left.contains(right),
        AssertOp::Regex => regex::Regex::new(right)
            .map_err(|e| format!("잘못된 정규식 '{right}': {e}"))?
            .is_match(left),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("assert 실패: '{left}' {op:?} '{right}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_passes_and_fails() {
        assert!(check("a", &AssertOp::Eq, "a").is_ok());
        assert!(check("a", &AssertOp::Eq, "b").is_err());
    }

    #[test]
    fn contains_works() {
        assert!(check("ACTIVE state", &AssertOp::Contains, "ACTIVE").is_ok());
        assert!(check("ERROR", &AssertOp::Contains, "ACTIVE").is_err());
    }

    #[test]
    fn regex_works_and_rejects_bad_pattern() {
        assert!(check("abc-123", &AssertOp::Regex, "^[a-z]+-\\d+$").is_ok());
        assert!(check("abc", &AssertOp::Regex, "[").is_err());
    }
}
