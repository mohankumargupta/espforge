//! The machine format: one JSON document per run, stable field names.
//!
//! Spans are byte offsets into the original source, as `[start, end)`, and the
//! file name is repeated per diagnostic so a consumer can annotate output
//! without knowing which file the tool read.

use serde_json::{Value, json};

use crate::Diagnostic;

/// Serialize diagnostics for `file_path` as pretty JSON.
///
/// `code` is `null` while Phase 2 increment 1 is in progress; the field exists
/// now so a consumer never has to guess whether codes will appear.
pub fn to_json(file_path: &str, diagnostics: &[Diagnostic]) -> String {
    let items: Vec<Value> = diagnostics
        .iter()
        .map(|d| diagnostic_value(file_path, d))
        .collect();
    serde_json::to_string_pretty(&json!({ "diagnostics": items }))
        .expect("a serde_json::Value is always serializable")
}

fn diagnostic_value(file_path: &str, d: &Diagnostic) -> Value {
    json!({
        "code": d.code.map(|code| code.as_str()),
        "severity": d.severity.as_str(),
        "message": d.message,
        "primary": {
            "file": file_path,
            "span": [d.primary.start, d.primary.end],
        },
        "labels": d
            .labels
            .iter()
            .map(|label| json!({
                "span": [label.span.start, label.span.end],
                "text": label.text,
            }))
            .collect::<Vec<_>>(),
        "help": d.help,
        "fixes": d
            .fixes
            .iter()
            .map(|fix| json!({
                "title": fix.title,
                "edits": fix
                    .edits
                    .iter()
                    .map(|edit| json!({
                        "span": [edit.span.start, edit.span.end],
                        "text": edit.text,
                    }))
                    .collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagCode, Edit, Fix, Label, Severity};

    #[test]
    fn the_document_has_the_documented_shape() {
        let d = Diagnostic::error(4..8, "pin conflict", "GPIO18 is taken")
            .with_code(DiagCode::new("E0701"))
            .with_help("pick another pin");

        let json = to_json("blink.espf", &[d]);
        let value: Value = serde_json::from_str(&json).unwrap();
        let diag = &value["diagnostics"][0];

        assert_eq!(diag["code"], "E0701");
        assert_eq!(diag["severity"], "error");
        assert_eq!(diag["message"], "pin conflict");
        assert_eq!(diag["primary"]["file"], "blink.espf");
        assert_eq!(diag["primary"]["span"], json!([4, 8]));
        assert_eq!(diag["labels"][0]["text"], "GPIO18 is taken");
        assert_eq!(diag["help"], "pick another pin");
        assert_eq!(diag["fixes"], json!([]));
    }

    #[test]
    fn an_unassigned_code_is_null_not_invented() {
        let d = Diagnostic::error(0..1, "m", "l");
        let json = to_json("x.espf", &[d]);
        let value: Value = serde_json::from_str(&json).unwrap();
        assert!(value["diagnostics"][0]["code"].is_null());
    }

    #[test]
    fn fixes_and_context_labels_are_preserved() {
        let d = Diagnostic {
            code: Some(DiagCode::new("E0701")),
            severity: Severity::Warning,
            message: "pin conflict".into(),
            primary: 4..8,
            labels: vec![
                Label {
                    span: 4..8,
                    text: "claimed here".into(),
                },
                Label {
                    span: 20..24,
                    text: "first claimed here".into(),
                },
            ],
            help: None,
            fixes: vec![Fix {
                title: "rename the second led".into(),
                edits: vec![Edit {
                    span: 24..27,
                    text: "red-led-2".into(),
                }],
            }],
        };

        let json = to_json("blink.espf", &[d]);
        let value: Value = serde_json::from_str(&json).unwrap();
        let diag = &value["diagnostics"][0];

        assert_eq!(diag["severity"], "warning");
        assert_eq!(diag["labels"][1]["span"], json!([20, 24]));
        assert_eq!(diag["fixes"][0]["title"], "rename the second led");
        assert_eq!(diag["fixes"][0]["edits"][0]["span"], json!([24, 27]));
        assert_eq!(diag["fixes"][0]["edits"][0]["text"], "red-led-2");
        assert!(diag["help"].is_null());
    }

    #[test]
    fn the_pretty_shape_is_pinned() {
        let d = Diagnostic::error(0..1, "boom", "here");
        assert_eq!(
            to_json("x.espf", &[d]),
            "{
  \"diagnostics\": [
    {
      \"code\": null,
      \"fixes\": [],
      \"help\": null,
      \"labels\": [
        {
          \"span\": [
            0,
            1
          ],
          \"text\": \"here\"
        }
      ],
      \"message\": \"boom\",
      \"primary\": {
        \"file\": \"x.espf\",
        \"span\": [
          0,
          1
        ]
      },
      \"severity\": \"error\"
    }
  ]
}"
        );
    }
}
