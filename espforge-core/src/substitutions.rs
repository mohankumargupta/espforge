//! `env:NAME`: attribute values read from a file beside the spec.
//!
//! Any attribute in a `.espf` may be written exactly `env:NAME`. Before the
//! spec is lowered, `espforge` replaces it with the value of `NAME` from
//! `substitutions.env` (in the directory of the `.espf`) or from the process
//! environment; the environment wins, so CI can override a value without
//! editing the file. Substitution is one pass: a value that reads `env:OTHER`
//! is the literal text `env:OTHER`.
//!
//! The file is deliberately smaller than dotenv: `KEY=VALUE` lines, whole-line
//! `#` comments, and no expansion, `export` or multiline values. A line that
//! does not parse rejects the whole file rather than half-reading it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use espf::context::AttrOp;
use espf::parser::Parsed;
use espforge_diagnostics::{Diagnostic, codes};

/// The file read from the directory of the `.espf`.
pub const FILE_NAME: &str = "substitutions.env";

/// The values `substitutions.env` defines.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Table {
    values: HashMap<String, String>,
}

impl Table {
    /// Parse `KEY=VALUE` lines.
    ///
    /// `Err` is the 1-based line number and what was wrong with it. The first
    /// `=` splits, so a value may contain `=`; surrounding double quotes are
    /// stripped (the only way to keep spaces, or to start with `#`), and an
    /// unquoted value is trimmed. A repeated key takes its last value.
    pub fn parse(text: &str) -> Result<Self, (usize, String)> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut values = HashMap::new();
        for (index, line) in text.lines().enumerate() {
            let line_number = index + 1;
            // Indentation is layout, not part of a key; the value is untouched.
            let line = line.trim_start_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err((line_number, format!("expected KEY=VALUE, found `{line}`")));
            };
            if !is_name(key) {
                return Err((
                    line_number,
                    format!(
                        "`{key}` is not a variable name (letters, digits and `_`, \
                         not starting with a digit)"
                    ),
                ));
            }
            let value = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(quoted) => quoted,
                None => value.trim_matches([' ', '\t']),
            };
            values.insert(key.to_string(), value.to_string());
        }
        Ok(Self { values })
    }

    /// The value `name` was given, if the file defines it.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

/// Resolve every `env:NAME` in `parsed`, in place.
///
/// `dir` is the directory of the `.espf`; `None` (an embedded example) reads no
/// file, only `env`. Nothing is read when the spec holds no marker at all, so a
/// project that does not use substitutions neither needs the file nor breaks
/// because of a malformed one. Diagnostics point at the attribute that holds
/// the marker; the `.env` file has no source map, so a bad line is named by
/// file and line in the message.
pub fn resolve(
    parsed: &mut Parsed<'_>,
    dir: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<Diagnostic> {
    // The first marker is where a file-level error is reported: it is the use
    // that made the file matter.
    let Some(first) = first_marker(parsed) else {
        return Vec::new();
    };

    let mut errors = Vec::new();
    let table = match dir {
        None => Table::default(),
        Some(dir) => load(dir, &first, &mut errors),
    };
    if !errors.is_empty() {
        return errors;
    }

    for nodes in parsed.sections.values_mut() {
        for node in nodes.iter_mut() {
            let mut substituted_id: Option<Cow<'_, str>> = None;
            for attr in node.attributes.iter_mut() {
                let Some(name) = attr.val.strip_prefix("env:") else {
                    continue;
                };
                // `env: NAME` is literal text: a marker has no space in it.
                if name.starts_with([' ', '\t']) {
                    continue;
                }
                let span = attr.val_offset..attr.val_offset + attr.val.len();
                if !is_name(name) {
                    errors.push(Diagnostic::error(
                        codes::SUBSTITUTION_INVALID_NAME,
                        span,
                        "invalid substitution",
                        format!(
                            "`env:{name}` is not `env:NAME`: NAME is letters, digits and `_`, \
                             not starting with a digit"
                        ),
                    ));
                    continue;
                }
                let Some(value) = env(name).or_else(|| table.get(name).map(str::to_string)) else {
                    errors.push(Diagnostic::error(
                        codes::SUBSTITUTION_UNDEFINED,
                        span,
                        format!("`env:{name}` is not defined"),
                        format!("not defined in `{FILE_NAME}` or the environment"),
                    ));
                    continue;
                };
                if attr.key == "id" && attr.op == AttrOp::Set {
                    substituted_id = Some(Cow::Owned(value.clone()));
                }
                attr.val = Cow::Owned(value);
            }
            // The id is stored beside the attributes; it has to follow a
            // substituted one.
            if let Some(id) = substituted_id {
                node.id = id;
            }
        }
    }

    errors.sort_by_key(|error| error.primary.start);
    errors
}

/// `substitutions.env` in `dir`, or an empty table when there is no such file.
fn load(dir: &Path, first: &Range<usize>, errors: &mut Vec<Diagnostic>) -> Table {
    let path = dir.join(FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Table::default(),
        Err(error) => {
            errors.push(file_error(
                first.clone(),
                format!("cannot read `{FILE_NAME}`: {error}"),
            ));
            return Table::default();
        }
    };
    match Table::parse(&text) {
        Ok(table) => table,
        Err((line, problem)) => {
            errors.push(file_error(
                first.clone(),
                format!("{FILE_NAME}:{line}: {problem}"),
            ));
            Table::default()
        }
    }
}

/// A bad line in the values file, reported at the spec value that needed it.
fn file_error(span: Range<usize>, detail: String) -> Diagnostic {
    Diagnostic::error(
        codes::SUBSTITUTION_FILE,
        span,
        format!("`{FILE_NAME}` is invalid"),
        detail,
    )
}

/// The span of the first `env:` marker in the spec, by file offset.
fn first_marker(parsed: &Parsed<'_>) -> Option<Range<usize>> {
    parsed
        .sections
        .values()
        .flatten()
        .flat_map(|node| node.attributes.iter())
        .filter(|attr| is_marker(&attr.val))
        .map(|attr| attr.val_offset..attr.val_offset + attr.val.len())
        .min_by_key(|span| span.start)
}

/// Whether a value is a substitution marker rather than literal text.
fn is_marker(value: &str) -> bool {
    value
        .strip_prefix("env:")
        .is_some_and(|name| !name.starts_with([' ', '\t']))
}

/// `[A-Za-z_][A-Za-z0-9_]*`.
fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spec that uses `env:` for a pin, an id, and a project name.
    const SPEC: &str = "\
# espforge
  <project name=\"env:NAME\" />
  <chip type=\"esp32c3\" />
  <runtime type=\"blocking\" />
# peripherals
  <gpio id=\"g\" pin=\"env:LED_PIN\" direction=\"output\" />
# devices
  <led id=\"env:LED_ID\" gpio=\"$g\" />
# services
";

    fn parsed(src: &str) -> Parsed<'_> {
        espf::parser::parse_espf(src)
    }

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn the_file_is_a_small_dotenv() {
        let table = Table::parse(
            "\u{feff}# a comment\r\n\r\nWIFI_SSID=MyNetwork\r\n  LED_PIN=18\n\
             WIFI_PASSWORD=\"correct horse  battery staple\"\n\
             TOKEN=a=b#c\nQUOTED_HASH=\"#not-a-comment\"\nDUP=first\nDUP=last\n",
        )
        .expect("the file parses");
        assert_eq!(table.get("WIFI_SSID"), Some("MyNetwork"));
        assert_eq!(table.get("LED_PIN"), Some("18"));
        assert_eq!(
            table.get("WIFI_PASSWORD"),
            Some("correct horse  battery staple"),
            "quotes are stripped, spaces inside them kept"
        );
        assert_eq!(table.get("TOKEN"), Some("a=b#c"), "the first `=` splits");
        assert_eq!(table.get("QUOTED_HASH"), Some("#not-a-comment"));
        assert_eq!(table.get("DUP"), Some("last"));
    }

    /// `LED_PIN = 18` is not `KEY=VALUE` under the rules: the key is what is
    /// left of the first `=`, spaces included. Loud beats guessing.
    #[test]
    fn a_line_that_is_not_key_value_names_its_line() {
        let err = Table::parse("# c\nnot a pair\n").unwrap_err();
        assert_eq!(err.0, 2);
        assert!(err.1.contains("expected KEY=VALUE"), "{}", err.1);

        let err = Table::parse("ok=1\n1BAD=2\n").unwrap_err();
        assert_eq!(err.0, 2);
        assert!(err.1.contains("`1BAD` is not a variable name"), "{}", err.1);
    }

    #[test]
    fn every_attribute_is_substituted_and_ids_follow() {
        let mut spec = parsed(SPEC);
        let errors = resolve(
            &mut spec,
            None,
            &lookup(&[("NAME", "blink"), ("LED_PIN", "18"), ("LED_ID", "red-led")]),
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(spec.sections["espforge"][0].get_attr("name"), Some("blink"));
        assert_eq!(spec.sections["peripherals"][0].get_attr("pin"), Some("18"));
        assert_eq!(spec.sections["devices"][0].id, "red-led");
    }

    #[test]
    fn a_missing_name_is_an_error_at_the_value() {
        let mut spec = parsed(SPEC);
        let errors = resolve(&mut spec, None, &lookup(&[("LED_PIN", "18")]));
        let undefined: Vec<_> = errors
            .iter()
            .filter(|error| error.code == codes::SUBSTITUTION_UNDEFINED)
            .collect();
        assert_eq!(undefined.len(), 2, "{errors:?}");
        assert_eq!(undefined[0].message, "`env:NAME` is not defined");
        assert_eq!(&SPEC[undefined[0].primary.clone()], "env:NAME");
    }

    #[test]
    fn a_marker_without_a_name_is_invalid() {
        for value in ["env:", "env:1bad", "env:WIFI-SSID"] {
            let src = SPEC.replace("env:LED_PIN", value);
            let mut spec = parsed(&src);
            let errors = resolve(&mut spec, None, &lookup(&[("NAME", "p"), ("LED_ID", "l")]));
            assert_eq!(errors.len(), 1, "{value}: {errors:?}");
            assert_eq!(errors[0].code, codes::SUBSTITUTION_INVALID_NAME);
            assert_eq!(&src[errors[0].primary.clone()], value);
        }
    }

    /// `env: NAME` and anything that merely contains `env:` are literals, not
    /// markers: the lookup is never consulted for them.
    #[test]
    fn a_value_that_only_resembles_a_marker_is_literal() {
        for literal in ["env: NAME", "network-env:NAME", "my env:NAME network"] {
            let src = SPEC.replace("env:LED_PIN", literal);
            let mut spec = parsed(&src);
            let errors = resolve(&mut spec, None, &lookup(&[("NAME", "p"), ("LED_ID", "l")]));
            assert_eq!(errors.len(), 0, "{literal}: {errors:?}");
            assert_eq!(
                spec.sections["peripherals"][0].get_attr("pin"),
                Some(literal)
            );
        }
    }

    /// A resolved value is final: a value that itself reads `env:` is not
    /// resolved again (no cycles, one meaning for `env:`).
    #[test]
    fn substitution_is_one_pass() {
        let src = SPEC.replace("env:LED_PIN", "env:A");
        let mut spec = parsed(&src);
        let errors = resolve(
            &mut spec,
            None,
            &lookup(&[("A", "env:B"), ("NAME", "p"), ("LED_ID", "l")]),
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            spec.sections["peripherals"][0].get_attr("pin"),
            Some("env:B")
        );
    }

    #[test]
    fn the_environment_wins_over_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE_NAME),
            "LED_PIN=11\nLED_ID=from-file\nNAME=file\n",
        )
        .unwrap();
        let mut spec = parsed(SPEC);
        let errors = resolve(&mut spec, Some(dir.path()), &lookup(&[("LED_PIN", "18")]));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            spec.sections["peripherals"][0].get_attr("pin"),
            Some("18"),
            "the environment overrides the file"
        );
        assert_eq!(spec.sections["devices"][0].id, "from-file");
    }

    #[test]
    fn a_bad_file_line_is_reported_at_the_first_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "LED_PIN=18\nnonsense\n").unwrap();
        let mut spec = parsed(SPEC);
        let errors = resolve(&mut spec, Some(dir.path()), &lookup(&[]));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].code, codes::SUBSTITUTION_FILE);
        assert_eq!(errors[0].message, "`substitutions.env` is invalid");
        assert!(
            errors[0].labels[0].text.starts_with("substitutions.env:2:"),
            "{:?}",
            errors[0].labels[0].text
        );
        assert_eq!(&SPEC[errors[0].primary.clone()], "env:NAME");
    }

    /// The file is read only when the spec uses it: a malformed one is not a
    /// problem for a project that does not substitute anything.
    #[test]
    fn a_project_without_markers_never_reads_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "nonsense\n").unwrap();
        let src = SPEC
            .replace("env:NAME", "p")
            .replace("env:LED_PIN", "18")
            .replace("env:LED_ID", "l");
        let mut spec = parsed(&src);
        let errors = resolve(&mut spec, Some(dir.path()), &lookup(&[]));
        assert!(errors.is_empty(), "{errors:?}");
    }

    /// An embedded example has no directory: only the environment is read.
    #[test]
    fn an_example_reads_only_the_environment() {
        let mut spec = parsed(SPEC);
        let errors = resolve(
            &mut spec,
            None,
            &lookup(&[("NAME", "p"), ("LED_PIN", "18"), ("LED_ID", "l")]),
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(spec.sections["peripherals"][0].get_attr("pin"), Some("18"));
    }
}
