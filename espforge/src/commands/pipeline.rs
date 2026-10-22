//! The front half shared by `check`, `build` and `gen`: find the source, then
//! parse -> `# espforge` core -> options -> flag overrides -> resolve -> `$`
//! dependency graph. One function, so the commands cannot disagree.

use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_emit::{Hardware, I2cSpec, LedSpec, WifiSpec};
use espforge_model::Meta;
use espforge_options::{Answers, ResolvedOptions};
use std::path::{Path, PathBuf};

use super::answers::AnswerArgs;
use super::frontend::{self, Frontend};

// -------------------------------------------------------------------- source

pub struct Source {
    /// Name used in diagnostics: the path, or `embedded:<category>/<name>`.
    pub name: String,
    pub text: String,
    /// Directory of the `.espf` (None for embedded examples).
    pub dir: Option<PathBuf>,
    pub example: Option<&'static espforge_examples::Example>,
}

fn from_file(file: &Path) -> Result<Source, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read '{}': {e}", file.display()))?;
    let dir = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Source {
        name: file.to_string_lossy().into_owned(),
        text,
        dir: Some(dir),
        example: None,
    })
}

/// The single `*.espf` / `*.espf.txt` file in `dir`.
pub fn find_project(dir: &Path) -> Result<PathBuf, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".espf") || n.ends_with(".espf.txt"))
        .collect();
    names.sort();
    let at = |name: &str| {
        if dir == Path::new(".") {
            PathBuf::from(name)
        } else {
            dir.join(name)
        }
    };
    match names.len() {
        0 => Err(format!(
            "no .espf file in '{}'; give a FILE or --example <category/name>",
            dir.display()
        )),
        1 => Ok(at(&names[0])),
        _ => Err(format!(
            "several .espf files in '{}' ({}); give one explicitly",
            dir.display(),
            names.join(", ")
        )),
    }
}

/// FILE, `--example`, or (neither) the single `.espf` in the current directory.
pub fn resolve_source(file: Option<&Path>, example: Option<&str>) -> Result<Source, String> {
    match (file, example) {
        (Some(_), Some(_)) => Err("give either FILE or --example, not both".to_string()),
        (Some(f), None) => from_file(f),
        (None, None) => from_file(&find_project(Path::new("."))?),
        (None, Some(path)) => {
            let ex = espforge_examples::find(path).ok_or_else(|| {
                format!(
                    "no example '{path}'; run `espforge examples list` to see what is available"
                )
            })?;
            Ok(Source {
                name: format!("embedded:{path}"),
                text: ex.content.clone(),
                dir: None,
                example: Some(ex),
            })
        }
    }
}

// ---------------------------------------------------------------- validation

/// Why validation failed.
#[derive(Debug)]
pub enum Invalid {
    /// Located diagnostics (rendered as snippets).
    Spans(Vec<EspfError>),
    /// A problem with a flag or the like; no source location.
    Message(String),
}

impl From<Vec<EspfError>> for Invalid {
    fn from(e: Vec<EspfError>) -> Self {
        Invalid::Spans(e)
    }
}

impl From<String> for Invalid {
    fn from(m: String) -> Self {
        Invalid::Message(m)
    }
}

impl Invalid {
    pub fn report(&self, name: &str, src: &str) {
        match self {
            Invalid::Spans(errors) => frontend::report(name, src, errors),
            Invalid::Message(m) => eprintln!("❌ {m}"),
        }
    }
}

/// A fully validated spec: final settings (file + flag overrides), resolved
/// options and the bound `$` graph.
pub struct Checked<'a> {
    pub fe: Frontend<'a>,
    /// Core settings after flag overrides.
    pub meta: Meta,
    /// Option answers after flag overrides.
    pub answers: Answers,
    pub resolved: ResolvedOptions,
    pub ctx: RuntimeContext<'a>,
    /// `note: ...` lines for flags that changed something.
    pub notes: Vec<String>,
}

pub fn validate<'a>(src: &'a str, flags: &AnswerArgs) -> Result<Checked<'a>, Invalid> {
    let fe = frontend::load(src)?;

    // Flags win over the file; collect a note when they change something.
    let mut meta = fe.meta.clone();
    let mut answers = fe.answers.clone();
    let mut notes = Vec::new();
    flags.apply_core(&mut meta, &mut notes)?;
    flags.apply_options(
        espforge_options::embedded_schema(),
        &mut answers,
        &mut notes,
    )?;

    let resolved = espforge_options::resolve(&meta, &answers, &fe.derived)?;

    // `$` dependency graph (all five sections exist after `load`).
    let mut ctx = RuntimeContext::new();
    ctx.resolve_and_bind(
        &fe.parsed.sections["peripherals"],
        &fe.parsed.sections["components"],
        &fe.parsed.sections["devices"],
    )?;

    Ok(Checked {
        fe,
        meta,
        answers,
        resolved,
        ctx,
        notes,
    })
}

/// The first section `build` cannot generate firmware for yet.
pub fn unsupported_section(c: &Checked) -> Option<&'static str> {
    ["peripherals", "components", "devices", "services"]
        .into_iter()
        .find(|n| c.fe.parsed.sections.get(*n).is_some_and(|v| !v.is_empty()))
}

fn is_rust_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `i2c0` -> `I2C0`, `i2c1` -> `I2C1`, `i2c2` -> `I2C2`; anything else is the
/// user's error (the esp-hal peripheral structs are named exactly this).
fn i2c_peripheral_name(id: &str) -> Result<String, String> {
    if id == "i2c0" || id == "i2c1" || id == "i2c2" {
        return Ok(id.to_ascii_uppercase());
    }
    Err(format!(
        "<i2c id=\"{id}\">: the id must be i2c0, i2c1 or i2c2"
    ))
}

/// A `frequency=` value like `100kHz` or `400kHz` -> kHz; anything else is
/// told off.
fn parse_khz(raw: &str, id: &str) -> Result<u32, String> {
    let t = raw.trim().to_ascii_lowercase();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u32 = num.parse().map_err(|_| {
        format!(
            "<i2c id=\"{id}\"> frequency=\"{raw}\" is not a frequency: use e.g. `100kHz`"
        )
    })?;
    match unit.trim() {
        "khz" => Ok(n),
        other => Err(format!(
            "<i2c id=\"{id}\"> frequency=\"{raw}\" unit `{other}` is not supported yet: use `kHz`"
        )),
    }
}

/// An `EspfError` as a one-line message (the hardware step reports plain
/// strings, like the rest of `build`'s "not supported yet" messages).
fn plain(e: EspfError) -> String {
    format!("{}: {}", e.title, e.label)
}

/// The `<wifi>` service. One network only: `network1.ssid` (required) and
/// `network1.password` (optional; empty or absent means an open network).
/// Both strings end up inside Rust string literals in `generated.rs`.
fn wifi_spec(n: &ParsedNode) -> Result<WifiSpec, String> {
    let id = n.id;
    let obj = n
        .to_object()
        .map_err(|errs| errs.into_iter().map(plain).collect::<Vec<_>>().join("; "))?;

    if let Some(key) = obj
        .fields
        .keys()
        .find(|k| !matches!(k.as_str(), "id" | "network1"))
    {
        return Err(format!(
            "<wifi id=\"{id}\"> has `{key}`, which is not supported yet (only network1.ssid and network1.password: one network)"
        ));
    }

    let net = obj
        .get_object("network1")
        .map_err(plain)?
        .ok_or_else(|| format!("<wifi id=\"{id}\"> needs network1.ssid=\"...\""))?;
    if let Some(key) = net
        .fields
        .keys()
        .find(|k| !matches!(k.as_str(), "ssid" | "password"))
    {
        return Err(format!(
            "<wifi id=\"{id}\"> has `network1.{key}`, which is not supported yet (only ssid and password)"
        ));
    }

    let ssid = net
        .get_str("ssid")
        .map_err(plain)?
        .ok_or_else(|| format!("<wifi id=\"{id}\"> needs network1.ssid=\"...\""))?
        .value
        .clone();
    let password = net
        .get_str("password")
        .map_err(plain)?
        .map(|p| p.value.clone())
        .unwrap_or_default();

    if ssid.is_empty() || ssid.len() > 32 {
        return Err(format!(
            "<wifi id=\"{id}\"> network1.ssid must be 1 to 32 bytes, got {}",
            ssid.len()
        ));
    }
    if !(password.is_empty() || (8..=63).contains(&password.len())) {
        return Err(format!(
            "<wifi id=\"{id}\"> network1.password must be empty (open network) or 8 to 63 characters, got {}",
            password.len()
        ));
    }
    for (what, value) in [("ssid", &ssid), ("password", &password)] {
        if value.contains('\\') {
            return Err(format!(
                "<wifi id=\"{id}\"> network1.{what} cannot contain a backslash"
            ));
        }
    }

    Ok(WifiSpec { ssid, password })
}

/// The hardware and services `build` can generate code for today: `<gpio>`
/// peripherals used by `<led>` components, `<i2c>` peripherals used by
/// `<i2c_component>` components, and one `<wifi>` service. Anything else is a
/// clear "not supported yet".
pub fn hardware(c: &Checked) -> Result<Hardware, String> {
    struct I2cBusDraft {
        id: String,
        peripheral: String,
        sda: u8,
        scl: u8,
        khz: u32,
        /// The component id riding on this bus, if any.
        used: Option<String>,
    }

    let sections = &c.fe.parsed.sections;
    if sections.get("devices").is_some_and(|v| !v.is_empty()) {
        return Err(
            "`# devices` is not supported by `build` yet (only <gpio> peripherals with <led> components, <i2c> peripherals with <i2c_component> components, and the <wifi> service)"
                .to_string(),
        );
    }

    // id -> (pin, direction)
    let mut gpios: Vec<(&str, u8, Option<&str>)> = Vec::new();
    // `<i2c>` peripherals, in spec order; filled in by the next loop.
    let mut i2c_buses: Vec<I2cBusDraft> = Vec::new();
    for n in sections
        .get("peripherals")
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if n.tag_name == "gpio" {
            let pin = n
                .get_attr("pin")
                .and_then(|p| p.parse::<u8>().ok())
                .ok_or_else(|| format!("<gpio id=\"{}\"> needs a numeric pin=\"...\"", n.id))?;
            gpios.push((n.id, pin, n.get_attr("direction")));
        } else if n.tag_name == "i2c" {
            let pin_attr = |what: &str| {
                n.get_attr(what)
                    .and_then(|p| p.parse::<u8>().ok())
                    .ok_or_else(|| format!("<i2c id=\"{}\"> needs a numeric {what}=\"...\"", n.id))
            };
            let khz = match n.get_attr("frequency") {
                None => 100, // esp-hal's default
                Some(f) => parse_khz(f, n.id)?,
            };
            i2c_buses.push(I2cBusDraft {
                id: n.id.to_string(),
                peripheral: i2c_peripheral_name(n.id)?,
                sda: pin_attr("sda")?,
                scl: pin_attr("scl")?,
                khz,
                used: None,
            });
        } else {
            return Err(format!(
                "<{}> in `# peripherals` is not supported by `build` yet (only <gpio> and <i2c>)",
                n.tag_name
            ));
        }
    }

    let mut leds: Vec<LedSpec> = Vec::new();
    for n in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
        if n.tag_name == "i2c_component" {
            let bus_ref = n
                .get_attr("i2c")
                .ok_or_else(|| format!("<i2c_component id=\"{}\"> needs i2c=\"$<i2c id>\"", n.id))?;
            // Note: unlike `<gpio>`, this must be an exact (non-prefixed) id;
            // `i2c="$i2c0"` and `i2c="i2c0"` are both accepted.
            let target = bus_ref.trim_start_matches('$');
            if !i2c_buses.iter().any(|b| b.id == target) {
                return Err(format!(
                    "<i2c_component id=\"{}\"> refers to `{target}`, which is not an <i2c> in `# peripherals`",
                    n.id
                ));
            }
            if let Some(other) = &i2c_buses
                .iter()
                .find(|b| b.id == target)
                .and_then(|b| b.used.as_deref())
            {
                return Err(format!(
                    "<i2c_component id=\"{other}\"> and <i2c_component id=\"{}\"> both use <i2c id=\"{target}\">",
                    n.id
                ));
            }
            let name = n.id.replace('-', "_");
            if !is_rust_ident(&name) {
                return Err(format!(
                    "<i2c_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    n.id
                ));
            }
            i2c_buses
                .iter_mut()
                .find(|b| b.id == target)
                .expect("checked above")
                .used = Some(name);
            continue;
        }
        if n.tag_name != "led" {
            return Err(format!(
                "<{}> in `# components` is not supported by `build` yet (only <led> and <i2c_component>)",
                n.tag_name
            ));
        }
        let gpio_ref = n
            .get_attr("gpio")
            .ok_or_else(|| format!("<led id=\"{}\"> needs gpio=\"$<gpio id>\"", n.id))?;
        let target = gpio_ref.trim_start_matches('$');
        let (_, pin, direction) = gpios
            .iter()
            .find(|(id, _, _)| *id == target)
            .ok_or_else(|| {
                format!(
                    "<led id=\"{}\"> refers to `{target}`, which is not a <gpio> in `# peripherals`",
                    n.id
                )
            })?;
        if matches!(direction, Some(d) if *d != "output") {
            return Err(format!(
                "<led id=\"{}\"> needs an output pin, but `{target}` has direction=\"{}\"",
                n.id,
                direction.unwrap_or("")
            ));
        }
        let active_low = match n.get_attr("active_low") {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                return Err(format!(
                    "<led id=\"{}\"> active_low=\"{other}\" must be \"true\" or \"false\"",
                    n.id
                ));
            }
        };
        let name = n.id.replace('-', "_");
        if !is_rust_ident(&name) {
            return Err(format!(
                "<led id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ));
        }
        if let Some(other) = leds.iter().find(|l| l.pin == *pin) {
            return Err(format!(
                "<led id=\"{}\"> and <led id=\"{}\"> both use GPIO{pin}",
                other.name, n.id
            ));
        }
        leds.push(LedSpec {
            name,
            pin: *pin,
            active_low,
        });
    }

    let mut wifi: Option<WifiSpec> = None;
    for n in sections.get("services").map(Vec::as_slice).unwrap_or(&[]) {
        if n.tag_name != "wifi" {
            return Err(format!(
                "<{}> in `# services` is not supported by `build` yet (only <wifi>)",
                n.tag_name
            ));
        }
        if wifi.is_some() {
            return Err("only one <wifi> is supported in `# services`".to_string());
        }
        wifi = Some(wifi_spec(n)?);
    }

    // A `<gpio>` that shares a pin with an I2C SDA/SCL would be a wiring bug;
    // refuse it the same way two `<led>`s on one pin are refused.
    for bus in &i2c_buses {
        for (id, pin, _) in &gpios {
            if *pin == bus.sda || *pin == bus.scl {
                return Err(format!(
                    "<gpio id=\"{id}\"> and <i2c id=\"{}\"> both use GPIO{pin}",
                    bus.id
                ));
            }
        }
    }

    let i2cs: Vec<I2cSpec> = i2c_buses
        .into_iter()
        // A bare `<i2c>` with no component generates nothing: the app can
        // only reach the bus through a component anyway.
        .filter_map(|b| {
            b.used.map(|name| I2cSpec {
                name,
                peripheral: b.peripheral,
                sda: b.sda,
                scl: b.scl,
                khz: b.khz,
            })
        })
        .collect();

    Ok(Hardware { leds, i2cs, wifi })
}

/// Whether `build` can turn this spec into firmware. `build` fails on `Err`;
/// `check` reports it as a warning.
pub fn buildable(c: &Checked) -> Result<(), String> {
    hardware(c).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    const WITH_LED: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />\n# components\n  <led id=\"red-led\" gpio=\"$gpio18\" active_low=\"true\" />\n# devices\n# services\n";

    const WITH_WIFI: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"embassy\" />\n# peripherals\n# components\n# devices\n# services\n  <wifi id=\"wifi0\" network1.ssid=\"Wokwi-GUEST\" />\n";

    fn hello() -> &'static str {
        &espforge_examples::find("01.Basics/helloworld")
            .unwrap()
            .content
    }

    #[test]
    fn find_project_wants_exactly_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_project(dir.path()).is_err());
        std::fs::write(dir.path().join("a.espf"), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        assert_eq!(find_project(dir.path()).unwrap(), dir.path().join("a.espf"));
        std::fs::write(dir.path().join("b.espf.txt"), "").unwrap();
        let err = find_project(dir.path()).unwrap_err();
        assert!(
            err.contains("a.espf") && err.contains("b.espf.txt"),
            "{err}"
        );
    }

    #[test]
    fn file_and_example_together_are_rejected() {
        assert!(resolve_source(Some(Path::new("x.espf")), Some("01.Basics/helloworld")).is_err());
        assert!(resolve_source(None, Some("no/such")).is_err());
    }

    #[test]
    fn valid_spec_passes_and_is_buildable() {
        let c = validate(hello(), &AnswerArgs::default())
            .ok()
            .expect("hello validates");
        assert!(c.notes.is_empty());
        assert!(buildable(&c).is_ok());
        assert!(hardware(&c).unwrap().leds.is_empty());
        assert!(hardware(&c).unwrap().wifi.is_none());
    }

    #[test]
    fn flags_override_and_leave_a_note() {
        let flags = AnswerArgs {
            chip: Some("esp32s3".into()),
            ..Default::default()
        };
        let c = validate(hello(), &flags).ok().expect("validates");
        assert_eq!(c.meta.chip, "esp32s3");
        assert_eq!(c.fe.meta.chip, "esp32c3", "the file's own value is kept");
        assert_eq!(c.notes.len(), 1);
    }

    #[test]
    fn bad_flags_are_messages_not_spans() {
        let flags = AnswerArgs {
            chip: Some("esp99".into()),
            ..Default::default()
        };
        assert!(matches!(
            validate(hello(), &flags).err(),
            Some(Invalid::Message(_))
        ));
    }

    #[test]
    fn graph_errors_are_reported_with_spans() {
        let src = WITH_BUS.replace("$i2c0", "$nope");
        match validate(&src, &AnswerArgs::default()).err() {
            Some(Invalid::Spans(errs)) => assert_eq!(errs[0].title, "unknown peripheral"),
            _ => panic!("expected a located error"),
        }
    }

    const I2C_400K: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" frequency=\"400kHz\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    #[test]
    fn unsupported_hardware_is_valid_but_not_buildable() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" />\n# components\n# devices\n# services\n";
        let c = validate(src, &AnswerArgs::default()).ok().expect("graph is fine");
        assert_eq!(unsupported_section(&c), Some("peripherals"));
        let err = buildable(&c).unwrap_err();
        assert!(err.contains("<spi>"), "{err}");
    }

    #[test]
    fn an_i2c_bus_with_a_component_becomes_hardware() {
        let c = validate(WITH_BUS, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert!(buildable(&c).is_ok());
        assert_eq!(
            hardware(&c).unwrap().i2cs,
            [I2cSpec {
                name: "c".into(),
                peripheral: "I2C0".into(),
                sda: 4,
                scl: 5,
                khz: 100,
            }]
        );
    }

    #[test]
    fn i2c_frequency_is_parsed() {
        let c = validate(I2C_400K, &AnswerArgs::default()).ok().expect("valid");
        let spec = &hardware(&c).unwrap().i2cs[0];
        assert_eq!(spec.khz, 400);

        let mhz = I2C_400K.replace("400kHz", "400MHZ");
        let c = validate(&mhz, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("not supported yet"), "{err}");
    }

    #[test]
    fn an_i2c_component_needs_a_declared_i2c() {
        // The ref passes graph resolution (`$pin7` is a peripheral) but the
        // component then points at a gpio, not an `<i2c>`.
        let src = WITH_BUS
            .replace("<i2c id=", "<gpio id=")
            .replace(" sda=\"4\" scl=\"5\"", " pin=\"4\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("not an <i2c>"), "{err}");
    }

    #[test]
    fn two_i2c_components_on_one_bus_are_refused() {
        let src = WITH_BUS.replace(
            "# devices",
            "  <i2c_component id=\"c2\" i2c=\"$i2c0\" address=\"0x49\" />\n# devices",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(
            err.contains("<i2c_component id=\"c\">") && err.contains("both use"),
            "{err}"
        );
    }

    #[test]
    fn a_gpio_sharing_a_pin_with_i2c_is_refused() {
        let src = WITH_BUS.replace(
            "# components",
            "  <gpio id=\"gpio5\" pin=\"5\" direction=\"output\" />\n# components",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("both use GPIO5"), "{err}");
    }

    #[test]
    fn a_bare_i2c_without_a_component_generates_nothing() {
        let src = WITH_BUS.replace(
            "# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n",
            "# components\n",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap().i2cs.is_empty());
    }

    #[test]
    fn i2c_ids_must_be_i2c0_to_i2c2() {
        let src = WITH_BUS
            .replace("<i2c id=\"i2c0\"", "<i2c id=\"bus0\"")
            .replace("i2c=\"$i2c0\"", "i2c=\"$bus0\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("i2c0, i2c1 or i2c2"), "{err}");
    }

    #[test]
    fn gpio_with_a_led_is_buildable_and_becomes_hardware() {
        let c = validate(WITH_LED, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        let hw = hardware(&c).unwrap();
        assert_eq!(
            hw.leds,
            [LedSpec {
                name: "red_led".into(),
                pin: 18,
                active_low: true
            }]
        );
    }

    #[test]
    fn two_leds_on_one_pin_are_refused() {
        let src = WITH_LED.replace(
            "# devices",
            "  <led id=\"other\" gpio=\"$gpio18\" />\n# devices",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("both use GPIO18"));
    }

    #[test]
    fn a_led_on_an_input_pin_is_refused() {
        let src = WITH_LED.replace("output", "input");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("needs an output pin"));
    }

    // ---- wifi service -------------------------------------------------------

    #[test]
    fn a_wifi_service_is_buildable_and_becomes_hardware() {
        let c = validate(WITH_WIFI, &AnswerArgs::default())
            .ok()
            .expect("embassy + wifi validates");
        let hw = hardware(&c).unwrap();
        assert_eq!(
            hw.wifi,
            Some(WifiSpec {
                ssid: "Wokwi-GUEST".into(),
                password: String::new(),
            })
        );
        assert!(hw.leds.is_empty());
    }

    #[test]
    fn wifi_needs_the_embassy_runtime() {
        let src = WITH_WIFI.replace("embassy", "blocking");
        assert!(matches!(
            validate(&src, &AnswerArgs::default()).err(),
            Some(Invalid::Spans(_))
        ));
    }

    #[test]
    fn wifi_password_rules() {
        let with = |attrs: &str| {
            WITH_WIFI.replace(
                "network1.ssid=\"Wokwi-GUEST\"",
                &format!("network1.ssid=\"home\" {attrs}"),
            )
        };

        let src = with("network1.password=\"short\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("8 to 63"));

        let src = with("network1.password=\"longenough\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert_eq!(hardware(&c).unwrap().wifi.unwrap().password, "longenough");

        let src = with("network1.password=\"back\\slash1\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("backslash"));
    }

    #[test]
    fn only_one_network_is_supported() {
        let src = WITH_WIFI.replace(
            "network1.ssid=\"Wokwi-GUEST\"",
            "network1.ssid=\"a\" network2.ssid=\"b\"",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("network2"));
    }

    #[test]
    fn wifi_without_an_ssid_is_refused() {
        let src = WITH_WIFI.replace(" network1.ssid=\"Wokwi-GUEST\"", "");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("network1.ssid"));
    }

    #[test]
    fn other_services_are_not_buildable_yet() {
        let src = WITH_WIFI.replace(
            "<wifi id=\"wifi0\" network1.ssid=\"Wokwi-GUEST\" />",
            "<ntp id=\"n\" servers+=\"pool.ntp.org\" />",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("<ntp>"));
    }
}
