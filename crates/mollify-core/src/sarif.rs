//! SARIF 2.1.0 output for code-scanning platforms (GitHub, GitLab).

use mollify_types::{Confidence, Finding, Severity};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Serialize findings as a SARIF 2.1.0 log.
pub fn to_sarif(findings: &[Finding], tool_version: &str) -> Value {
    // Unique rule ids → rules array (deterministic order).
    let rule_ids: BTreeSet<&str> = findings.iter().map(|f| f.rule.as_str()).collect();
    let rules: Vec<Value> = rule_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "name": id,
                "shortDescription": { "text": id.replace('-', " ") }
            })
        })
        .collect();

    let results: Vec<Value> = findings
        .iter()
        .map(|f| {
            json!({
                "ruleId": f.rule,
                "level": level(f.severity),
                "message": { "text": format!("{} [{}]", f.reason, confidence_str(f.confidence)) },
                "partialFingerprints": { "mollifyFingerprint": f.fingerprint },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": f.location.path.as_str() },
                        "region": region(f)
                    }
                }]
            })
        })
        .collect();

    json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "mollify",
                    "informationUri": "https://github.com/FavioVazquez/mollify",
                    "version": tool_version,
                    "rules": rules
                }
            },
            "results": results
        }]
    })
}

fn level(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => "none",
    }
}

fn confidence_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Certain => "certain",
        Confidence::Likely => "likely",
        Confidence::Uncertain => "uncertain",
    }
}

fn region(f: &Finding) -> Value {
    let mut r = json!({ "startLine": f.location.line.max(1) });
    if let Some(end) = f.location.end_line {
        r["endLine"] = json!(end.max(f.location.line));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use mollify_types::{Category, Location};

    fn finding() -> Finding {
        Finding {
            fingerprint: "unused-export:abcd1234".into(),
            rule: "unused-export".into(),
            category: Category::DeadCode,
            severity: Severity::Warn,
            confidence: Confidence::Certain,
            attribution: None,
            reason: "function `x` is unused".into(),
            location: Location {
                path: "a.py".into(),
                line: 5,
                column: 0,
                end_line: Some(7),
            },
            actions: vec![],
        }
    }

    #[test]
    fn emits_valid_sarif_shape() {
        let s = to_sarif(&[finding()], "0.1.0");
        assert_eq!(s["version"], "2.1.0");
        assert_eq!(s["runs"][0]["tool"]["driver"]["name"], "mollify");
        assert_eq!(s["runs"][0]["results"][0]["ruleId"], "unused-export");
        assert_eq!(s["runs"][0]["results"][0]["level"], "warning");
        assert_eq!(
            s["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"]["startLine"],
            5
        );
    }
}
