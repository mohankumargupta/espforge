//! Option resolution.
//!
//! Steps, in order (each error carries a span; when the cause is a default
//! the span is the `<options>` element, or the `# espforge` header if there
//! is none):
//!
//! 1. explicit values (espf, then flags)
//! 2. defaults: `defaults="true"` (the default) fills the rest from the
//!    schema; `defaults="false"` leaves unspecified bool options off
//! 3. derived flags (`embassy` from the runtime, `wifi` from `# services`);
//!    users may not set these
//! 4. `requires` propagation: required flags are turned on; an error if one
//!    was explicitly (or by derivation) turned off
//! 5. chip compatibility: explicit/derived/implied incompatible options are
//!    errors; incompatible *defaults* are silently switched off
//! 6. `selection_group`: at most one option per group
//! 7. console vs chip: `jtag-serial` needs USB-Serial-JTAG

use crate::chips;
use crate::schema::{Kind, OptValue, OptionDef, Schema, embedded_schema};
use espf::context::ParsedNode;
use espf::diag::{EspfError, node_span};
use espf::value::Value;
use espforge_model::{Console, Meta, Runtime};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// Options that are computed, never user-set.
const DERIVED: [&str; 2] = ["embassy", "wifi"];

// ------------------------------------------------------------------ answers

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Given in the `.espf`; span of the value.
    Espf(Range<usize>),
    /// Given on the command line.
    Flag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub value: OptValue,
    pub origin: Origin,
}

/// What the user asked for explicitly: `<options .../>` plus flags.
#[derive(Debug, Clone)]
pub struct Answers {
    pub defaults: bool,
    pub values: BTreeMap<String, Answer>,
    /// Span used for errors caused by defaults.
    pub fallback_span: Range<usize>,
}

fn expected_kind(kind: &Kind) -> &'static str {
    match kind {
        Kind::Bool => "\"true\" or \"false\"",
        Kind::String => "a string",
        Kind::Choice(_) => "one of the listed choices",
        Kind::List => "a list (`key+=\"a\" key+=\"b\"`)",
    }
}

fn parse_value(def: &OptionDef, key: &str, value: &Value) -> Result<OptValue, EspfError> {
    match (&def.kind, value) {
        (Kind::Bool, Value::Str(s)) => match s.value.as_str() {
            "true" => Ok(OptValue::Bool(true)),
            "false" => Ok(OptValue::Bool(false)),
            other => Err(EspfError::at_span(
                s.span.clone(),
                format!("`{key}` is not a boolean"),
                format!("\"{other}\" must be \"true\" or \"false\""),
            )),
        },
        (Kind::String, Value::Str(s)) => Ok(OptValue::Str(s.value.clone())),
        (Kind::Choice(choices), Value::Str(s)) => {
            if choices.contains(&s.value) {
                Ok(OptValue::Str(s.value.clone()))
            } else {
                Err(EspfError::at_span(
                    s.span.clone(),
                    format!("`{key}` has an invalid value"),
                    format!("\"{}\" is not one of: {}", s.value, choices.join(", ")),
                ))
            }
        }
        (Kind::List, Value::Array(items)) => Ok(OptValue::List(
            items.iter().map(|i| i.value.clone()).collect(),
        )),
        (_, other) => Err(EspfError::at_span(
            other.span(),
            format!("`{key}` has the wrong type"),
            format!(
                "expected {}, found {}",
                expected_kind(&def.kind),
                other.describe()
            ),
        )),
    }
}

impl Answers {
    /// No explicit answers; `fallback` is where default-caused errors point.
    pub fn empty(fallback: Range<usize>) -> Self {
        Self {
            defaults: true,
            values: BTreeMap::new(),
            fallback_span: fallback,
        }
    }

    /// Read the `<options>` element (if any). `fallback` is the span to use
    /// when there is no element, typically the `# espforge` header.
    pub fn from_node(
        schema: &Schema,
        node: Option<&ParsedNode>,
        fallback: Range<usize>,
    ) -> Result<Self, Vec<EspfError>> {
        let mut me = Self::empty(fallback);
        let Some(node) = node else {
            return Ok(me);
        };
        me.fallback_span = node_span(node);
        let obj = node.to_object()?;
        let mut errors = Vec::new();

        match obj.get_bool("defaults") {
            Ok(Some(b)) => me.defaults = b.value,
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
        for (key, value) in &obj.fields {
            if key == "defaults" || key == "id" {
                continue;
            }
            let key_span = node
                .attributes
                .iter()
                .find(|a| a.key == key.as_str())
                .map(|a| a.key_offset..a.key_offset + a.key.len())
                .unwrap_or_else(|| value.span());
            let Some(def) = schema.get(key) else {
                errors.push(EspfError::at_span(
                    key_span,
                    format!("unknown option `{key}`"),
                    "this is not an option in options.kdl",
                ));
                continue;
            };
            match parse_value(def, key, value) {
                Ok(v) => {
                    me.values.insert(
                        key.clone(),
                        Answer {
                            value: v,
                            origin: Origin::Espf(value.span()),
                        },
                    );
                }
                Err(e) => errors.push(e),
            }
        }
        if errors.is_empty() {
            Ok(me)
        } else {
            errors.sort_by_key(|e| e.span.start);
            Err(errors)
        }
    }

    /// Apply a command-line flag (`--name value`); it wins over the espf.
    /// Returns the answer it replaced, so the caller can print a notice.
    pub fn apply_flag(
        &mut self,
        schema: &Schema,
        name: &str,
        raw: &str,
    ) -> Result<Option<Answer>, String> {
        let def = schema
            .get(name)
            .ok_or_else(|| format!("unknown option `{name}`"))?;
        let value = match &def.kind {
            Kind::Bool => match raw {
                "true" => OptValue::Bool(true),
                "false" => OptValue::Bool(false),
                _ => return Err(format!("`{name}` must be true or false, got \"{raw}\"")),
            },
            Kind::String => OptValue::Str(raw.to_string()),
            Kind::Choice(cs) if cs.iter().any(|c| c == raw) => OptValue::Str(raw.to_string()),
            Kind::Choice(cs) => {
                return Err(format!("`{name}` must be one of: {}", cs.join(", ")));
            }
            Kind::List => OptValue::List(raw.split(',').map(|s| s.trim().to_string()).collect()),
        };
        Ok(self.values.insert(
            name.to_string(),
            Answer {
                value,
                origin: Origin::Flag,
            },
        ))
    }
}

// ------------------------------------------------------------ derived inputs

/// Facts the derived flags and checks are computed from.
#[derive(Debug, Clone, Default)]
pub struct DerivedInputs {
    /// Tag and span of every element in `# services`.
    pub services: Vec<(String, Range<usize>)>,
    /// Span of `<console>` in `# espforge`, if present.
    pub console_span: Option<Range<usize>>,
}

impl DerivedInputs {
    pub fn from_nodes(espforge: &[ParsedNode], services: &[ParsedNode]) -> Self {
        Self {
            services: services
                .iter()
                .map(|n| (n.tag_name.to_string(), node_span(n)))
                .collect(),
            console_span: espforge
                .iter()
                .find(|n| n.tag_name == "console")
                .map(node_span),
        }
    }
}

// ------------------------------------------------------------------- results

/// Why an option has its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    Explicit(Origin),
    Default,
    /// Computed; optional span of what it was computed from, and a reason.
    Derived(Option<Range<usize>>, String),
    /// Turned on because another option requires it.
    Implied(String),
}

impl Why {
    fn span(&self) -> Option<Range<usize>> {
        match self {
            Why::Explicit(Origin::Espf(s)) => Some(s.clone()),
            Why::Derived(Some(s), _) => Some(s.clone()),
            _ => None,
        }
    }

    fn describe(&self) -> String {
        match self {
            Why::Explicit(Origin::Espf(_)) => "set in <options>".into(),
            Why::Explicit(Origin::Flag) => "set by a command-line flag".into(),
            Why::Default => "its default".into(),
            Why::Derived(_, reason) => reason.clone(),
            Why::Implied(by) => format!("implied by `{by}`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub value: OptValue,
    pub why: Why,
}

impl Resolved {
    pub fn is_on(&self) -> bool {
        self.value.is_on()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedOptions {
    entries: BTreeMap<String, Resolved>,
}

impl ResolvedOptions {
    pub fn get(&self, name: &str) -> Option<&Resolved> {
        self.entries.get(name)
    }
    pub fn value(&self, name: &str) -> Option<&OptValue> {
        self.entries.get(name).map(|r| &r.value)
    }
    pub fn is_on(&self, name: &str) -> bool {
        self.entries.get(name).is_some_and(Resolved::is_on)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Resolved)> {
        self.entries.iter()
    }
}

// ------------------------------------------------------------------ resolver

/// Resolve against the embedded `options.kdl`.
pub fn resolve(
    core: &Meta,
    answers: &Answers,
    derived: &DerivedInputs,
) -> Result<ResolvedOptions, Vec<EspfError>> {
    resolve_with(embedded_schema(), core, answers, derived)
}

pub fn resolve_with(
    schema: &Schema,
    core: &Meta,
    answers: &Answers,
    derived: &DerivedInputs,
) -> Result<ResolvedOptions, Vec<EspfError>> {
    let anchor = answers.fallback_span.clone();
    let mut errors: Vec<EspfError> = Vec::new();
    let mut map: BTreeMap<String, Resolved> = BTreeMap::new();

    // 1. explicit values.
    for (name, a) in &answers.values {
        map.insert(
            name.clone(),
            Resolved {
                value: a.value.clone(),
                why: Why::Explicit(a.origin.clone()),
            },
        );
    }

    // 2. defaults.
    for def in &schema.options {
        if map.contains_key(&def.name) {
            continue;
        }
        let value = if answers.defaults {
            def.default.clone()
        } else if def.kind == Kind::Bool {
            Some(OptValue::Bool(false))
        } else {
            None
        };
        if let Some(value) = value {
            map.insert(
                def.name.clone(),
                Resolved {
                    value,
                    why: Why::Default,
                },
            );
        }
    }

    // 3. derived flags; users may not set them.
    let embassy_on = core.runtime == Runtime::Embassy;
    let wifi_service = derived.services.iter().find(|(t, _)| t == "wifi");
    let derived_flags: [(&str, bool, Option<Range<usize>>, String); 2] = [
        (
            "embassy",
            embassy_on,
            None,
            if embassy_on {
                "derived from <runtime type=\"embassy\" />".into()
            } else {
                "off because <runtime type=\"blocking\" />".into()
            },
        ),
        (
            "wifi",
            wifi_service.is_some(),
            wifi_service.map(|(_, s)| s.clone()),
            if wifi_service.is_some() {
                "derived from <wifi> in # services".into()
            } else {
                "off because there is no <wifi> in # services".into()
            },
        ),
    ];
    for (name, on, span, reason) in derived_flags {
        debug_assert!(DERIVED.contains(&name));
        if let Some(Resolved {
            why: Why::Explicit(origin),
            ..
        }) = map.get(name)
        {
            let at = match origin {
                Origin::Espf(s) => s.clone(),
                Origin::Flag => anchor.clone(),
            };
            errors.push(EspfError::at_span(
                at,
                format!("`{name}` cannot be set in <options>"),
                format!("it is derived, not chosen: {reason}; remove it"),
            ));
        }
        map.insert(
            name.to_string(),
            Resolved {
                value: OptValue::Bool(on),
                why: Why::Derived(span, reason),
            },
        );
    }

    // 4. requires propagation (to a fixpoint).
    let mut reported: BTreeSet<(String, String)> = BTreeSet::new();
    loop {
        let mut changed = false;
        let on: Vec<String> = map
            .iter()
            .filter(|(_, r)| r.is_on())
            .map(|(n, _)| n.clone())
            .collect();
        for name in on {
            let Some(def) = schema.get(&name) else {
                continue;
            };
            for req in &def.requires.options {
                let a = map[&name].clone();
                let existing = map.get(req).cloned();
                match existing {
                    Some(b) if b.is_on() => {}
                    Some(b) if !matches!(b.why, Why::Default | Why::Implied(_)) => {
                        if reported.insert((name.clone(), req.clone())) {
                            let span = match &b.why {
                                Why::Explicit(Origin::Espf(s)) => s.clone(),
                                _ => a.why.span().unwrap_or_else(|| anchor.clone()),
                            };
                            errors.push(EspfError::at_span(
                                span,
                                format!("`{name}` requires `{req}`"),
                                format!(
                                    "`{name}` is on ({}), but `{req}` is off ({})",
                                    a.why.describe(),
                                    b.why.describe()
                                ),
                            ));
                        }
                    }
                    _ => {
                        map.insert(
                            req.clone(),
                            Resolved {
                                value: OptValue::Bool(true),
                                why: Why::Implied(name.clone()),
                            },
                        );
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // 5. chip compatibility.
    let names: Vec<String> = map.keys().cloned().collect();
    for name in names {
        let Some(def) = schema.get(&name) else {
            continue;
        };
        if def.compat.allows(&core.chip) {
            continue;
        }
        let r = map[&name].clone();
        if !r.is_on() {
            continue;
        }
        if r.why == Why::Default {
            // Hidden for this chip: switch the default off.
            map.insert(
                name,
                Resolved {
                    value: OptValue::Bool(false),
                    why: Why::Default,
                },
            );
        } else {
            let chips = def.compat.chips.clone().unwrap_or_default();
            errors.push(EspfError::at_span(
                r.why.span().unwrap_or_else(|| anchor.clone()),
                format!("`{name}` is not available for {}", core.chip),
                format!(
                    "`{name}` is on ({}); it supports only: {}",
                    r.why.describe(),
                    chips.join(", ")
                ),
            ));
        }
    }

    // 6. selection groups: at most one option on per group.
    let mut groups: BTreeMap<&str, Vec<&OptionDef>> = BTreeMap::new();
    for def in &schema.options {
        if let Some(g) = &def.selection_group {
            groups.entry(g.as_str()).or_default().push(def);
        }
    }
    for (group, members) in groups {
        let active = |map: &BTreeMap<String, Resolved>| -> Vec<String> {
            members
                .iter()
                .filter(|d| map.get(&d.name).is_some_and(Resolved::is_on))
                .map(|d| d.name.clone())
                .collect()
        };
        let mut on = active(&map);
        if on.len() > 1 {
            // A default must not fight a deliberate choice: drop defaults
            // when something else was explicitly or implicitly chosen.
            let any_deliberate = on.iter().any(|n| !matches!(map[n].why, Why::Default));
            if any_deliberate {
                for n in &on {
                    if map[n].why == Why::Default {
                        map.insert(
                            n.clone(),
                            Resolved {
                                value: OptValue::Bool(false),
                                why: Why::Default,
                            },
                        );
                    }
                }
                on = active(&map);
            }
        }
        if on.len() > 1 {
            let span = on
                .iter()
                .rev()
                .find_map(|n| map[n].why.span())
                .unwrap_or_else(|| anchor.clone());
            errors.push(EspfError::at_span(
                span,
                format!(
                    "options {} are mutually exclusive",
                    on.iter()
                        .map(|n| format!("`{n}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                format!("they are all in selection group `{group}`; pick one"),
            ));
        }
    }

    // 7. console vs chip.
    if core.console == Console::JtagSerial && !chips::has_usb_serial_jtag(&core.chip) {
        errors.push(EspfError::at_span(
            derived
                .console_span
                .clone()
                .unwrap_or_else(|| anchor.clone()),
            format!("{} has no USB-Serial-JTAG", core.chip),
            "this chip has no native USB serial; use <console type=\"uart\" />",
        ));
    }

    if errors.is_empty() {
        Ok(ResolvedOptions { entries: map })
    } else {
        errors.sort_by_key(|e| e.span.start);
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::load;
    use crate::section::core_from_section;
    use espf::parser::parse_espf;

    const SCHEMA: &str = r#"
required "chip"
options {
    option "unstable-hal" {}
    option "alloc" {}
    option "embassy" selection_group="base" { requires "unstable-hal" }
    option "wifi" {
        requires "alloc" "unstable-hal" "embassy"
        compatible { chip "esp32" "esp32c3" }
    }
    option "log" default=#true {}
    option "only-esp32" { compatible { chip "esp32" } }
    option "dflt-esp32" default=#true { compatible { chip "esp32" } }
    option "g1" selection_group="grp" {}
    option "g2" selection_group="grp" {}
    option "mode" kind="choice" default="fast" { choices "fast" "slow" }
}
"#;

    fn file(chip: &str, runtime: &str, extra_espforge: &str, services: &str) -> String {
        format!(
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"{chip}\" />\n  <runtime type=\"{runtime}\" />\n{extra_espforge}# peripherals\n# components\n# devices\n# services\n{services}"
        )
    }

    fn run(src: &str) -> Result<ResolvedOptions, Vec<EspfError>> {
        let schema = load(SCHEMA).unwrap();
        let p = parse_espf(src);
        let header = p.header_offsets["espforge"];
        let nodes = &p.sections["espforge"];
        let (core, opt) = core_from_section(header, nodes).unwrap();
        let answers = Answers::from_node(&schema, opt, header..header)?;
        let derived = DerivedInputs::from_nodes(nodes, &p.sections["services"]);
        resolve_with(&schema, &core, &answers, &derived)
    }

    #[test]
    fn defaults_true_fills_from_schema() {
        let r = run(&file("esp32c3", "blocking", "", "")).unwrap();
        assert!(r.is_on("log"));
        assert!(!r.is_on("alloc"));
        assert_eq!(r.value("mode"), Some(&OptValue::Str("fast".into())));
        assert_eq!(r.get("log").unwrap().why, Why::Default);
    }

    #[test]
    fn defaults_false_turns_bools_off() {
        let r = run(&file(
            "esp32c3",
            "blocking",
            "  <options defaults=\"false\" />\n",
            "",
        ))
        .unwrap();
        assert!(!r.is_on("log"));
        assert!(r.value("mode").is_none());
    }

    #[test]
    fn explicit_beats_default() {
        let r = run(&file(
            "esp32c3",
            "blocking",
            "  <options log=\"false\" mode=\"slow\" />\n",
            "",
        ))
        .unwrap();
        assert!(!r.is_on("log"));
        assert_eq!(r.value("mode"), Some(&OptValue::Str("slow".into())));
    }

    #[test]
    fn bad_option_values_and_names_are_located() {
        let src = file(
            "esp32c3",
            "blocking",
            "  <options log=\"maybe\" nope=\"1\" mode=\"warp\" />\n",
            "",
        );
        let errs = run(&src).unwrap_err();
        assert_eq!(errs.len(), 3, "{errs:?}");
        let texts: Vec<&str> = errs.iter().map(|e| &src[e.span.clone()]).collect();
        assert!(
            texts.contains(&"maybe") && texts.contains(&"nope") && texts.contains(&"warp"),
            "{texts:?}"
        );
    }

    #[test]
    fn derived_flags_follow_runtime_and_services() {
        let r = run(&file(
            "esp32c3",
            "embassy",
            "",
            "  <wifi id=\"w\" network1.ssid=\"x\" />\n",
        ))
        .unwrap();
        assert!(r.is_on("embassy") && r.is_on("wifi"));
        // requires propagated
        assert!(r.is_on("alloc") && r.is_on("unstable-hal"));
        assert!(matches!(&r.get("alloc").unwrap().why, Why::Implied(by) if by == "wifi"));
        let r = run(&file("esp32c3", "blocking", "", "")).unwrap();
        assert!(!r.is_on("embassy") && !r.is_on("wifi"));
    }

    #[test]
    fn users_cannot_set_derived_flags() {
        let src = file("esp32c3", "embassy", "  <options embassy=\"true\" />\n", "");
        let errs = run(&src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("`embassy` cannot be set"));
        assert_eq!(&src[errs[0].span.clone()], "true");
    }

    #[test]
    fn requires_error_when_user_turned_the_required_flag_off() {
        let src = file(
            "esp32c3",
            "embassy",
            "  <options alloc=\"false\" />\n",
            "  <wifi id=\"w\" />\n",
        );
        let errs = run(&src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(
            errs[0].title.contains("`wifi` requires `alloc`"),
            "{}",
            errs[0].title
        );
        assert_eq!(&src[errs[0].span.clone()], "false");
        assert!(errs[0].label.contains("wifi") && errs[0].label.contains("alloc"));
    }

    #[test]
    fn wifi_with_blocking_runtime_names_both() {
        let src = file("esp32c3", "blocking", "", "  <wifi id=\"w\" />\n");
        let errs = run(&src).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.title.contains("`wifi` requires `embassy`")),
            "{errs:?}"
        );
        let e = errs.iter().find(|e| e.title.contains("embassy")).unwrap();
        assert_eq!(&src[e.span.clone()], "<wifi");
    }

    #[test]
    fn incompatible_explicit_option_is_an_error() {
        let src = file(
            "esp32c3",
            "blocking",
            "  <options only-esp32=\"true\" />\n",
            "",
        );
        let errs = run(&src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(
            errs[0]
                .title
                .contains("`only-esp32` is not available for esp32c3")
        );
        assert_eq!(&src[errs[0].span.clone()], "true");
        // explicitly off is fine
        assert!(
            run(&file(
                "esp32c3",
                "blocking",
                "  <options only-esp32=\"false\" />\n",
                ""
            ))
            .is_ok()
        );
    }

    #[test]
    fn incompatible_default_is_silently_off() {
        let r = run(&file("esp32c3", "blocking", "", "")).unwrap();
        assert!(!r.is_on("dflt-esp32"));
        let r = run(&file("esp32", "blocking", "", "")).unwrap();
        assert!(r.is_on("dflt-esp32"));
    }

    #[test]
    fn selection_group_is_exclusive() {
        let src = file(
            "esp32c3",
            "blocking",
            "  <options g1=\"true\" g2=\"true\" />\n",
            "",
        );
        let errs = run(&src).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("mutually exclusive"));
        assert!(errs[0].title.contains("`g1`") && errs[0].title.contains("`g2`"));
        assert!(
            run(&file(
                "esp32c3",
                "blocking",
                "  <options g1=\"true\" />\n",
                ""
            ))
            .is_ok()
        );
    }

    #[test]
    fn jtag_serial_needs_usb_serial_jtag() {
        let bad = file(
            "esp32",
            "blocking",
            "  <console type=\"jtag-serial\" />\n",
            "",
        );
        let errs = run(&bad).unwrap_err();
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].title.contains("no USB-Serial-JTAG"));
        assert_eq!(&bad[errs[0].span.clone()], "<console");
        assert!(
            run(&file(
                "esp32c3",
                "blocking",
                "  <console type=\"jtag-serial\" />\n",
                ""
            ))
            .is_ok()
        );
    }

    #[test]
    fn flags_override_the_espf() {
        let schema = load(SCHEMA).unwrap();
        let mut a = Answers::empty(0..0);
        a.apply_flag(&schema, "log", "false").unwrap();
        assert_eq!(a.values["log"].origin, Origin::Flag);
        let prev = a.apply_flag(&schema, "log", "true").unwrap();
        assert!(prev.is_some());
        assert!(a.apply_flag(&schema, "nope", "1").is_err());
        assert!(a.apply_flag(&schema, "log", "maybe").is_err());
    }

    #[test]
    fn embedded_schema_resolves_for_helloworld() {
        let src = file(
            "esp32c3",
            "blocking",
            "  <options defaults=\"true\" />\n",
            "",
        );
        let p = parse_espf(&src);
        let header = p.header_offsets["espforge"];
        let nodes = &p.sections["espforge"];
        let (core, opt) = core_from_section(header, nodes).unwrap();
        let answers = Answers::from_node(embedded_schema(), opt, header..header).unwrap();
        let derived = DerivedInputs::from_nodes(nodes, &p.sections["services"]);
        let r = resolve(&core, &answers, &derived).unwrap();
        assert!(!r.is_on("embassy"));
    }
}
