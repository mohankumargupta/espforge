//! The front half shared by `check`, `build` and `gen`: find the source, then
//! parse -> `# espforge` core -> options -> flag overrides -> resolve -> `$`
//! dependency graph. One function, so the commands cannot disagree.

use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_emit::{
    ButtonSpec, Hardware, I2cSpec, LedSpec, SpiBusSpec, SpiSpec, UartSpec, WifiSpec,
};
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

/// The implicit way to name a `<uart>`'s physical peripheral: `uart0` ->
/// `UART0`, `uart1` -> `UART1`, ...; anything else is the user's error (the
/// esp-hal peripheral structs are named exactly this).
fn uart_peripheral_name(id: &str) -> Result<String, String> {
    if matches!(id, "uart0" | "uart1" | "uart2" | "uart3") {
        return Ok(id.to_ascii_uppercase());
    }
    Err(format!(
        "<uart id=\"{id}\">: the id must be uart0, uart1, uart2 or uart3 (or give uart=\"UART1\" explicitly)"
    ))
}

/// An explicit `uart="UART1"` attribute on `<uart>`: the physical
/// `Peripherals.UART*` peripheral, case-insensitive. With it, the `<uart>`
/// id stops being encoded as `uartN` and is a plain reference name.
fn uart_attr_peripheral(raw: &str, id: &str) -> Result<String, String> {
    let upper = raw.trim().to_ascii_uppercase();
    if matches!(upper.as_str(), "UART0" | "UART1" | "UART2" | "UART3") {
        return Ok(upper);
    }
    Err(format!(
        "<uart id=\"{id}\"> uart=\"{raw}\" must be UART0, UART1, UART2 or UART3"
    ))
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
/// told off. `tag` is the element name used in the diagnostics (`i2c`, `spi`).
fn parse_khz(raw: &str, tag: &str, id: &str) -> Result<u32, String> {
    let t = raw.trim().to_ascii_lowercase();
    let split = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let n: u32 = num.parse().map_err(|_| {
        format!("<{tag} id=\"{id}\"> frequency=\"{raw}\" is not a frequency: use e.g. `100kHz`")
    })?;
    match unit.trim() {
        "khz" => Ok(n),
        other => Err(format!(
            "<{tag} id=\"{id}\"> frequency=\"{raw}\" unit `{other}` is not supported yet: use `kHz`"
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
/// peripherals used by `<led>` and `<button>` components, `<i2c>` peripherals
/// used by `<i2c_component>` components, `<spi>` peripherals used by
/// `<spi_component>` components, and one `<wifi>` service. Anything else is a
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

    struct SpiBusDraft {
        /// The `<spi>` id (`spi2`), which is also the peripheral name.
        id: String,
        sclk: u8,
        mosi: u8,
        miso: u8,
        khz: u32,
        mode: u8,
        /// Whether any `<spi_component>` rides on this bus.
        used: bool,
    }

    struct SpiDeviceDraft {
        /// The `<spi_component>` id as a Rust identifier.
        name: String,
        /// The bus id it rides on.
        bus: String,
        cs: u8,
    }

    /// The `<uart_component>` riding on a `<uart>`: its fields are what the
    /// generated code ends up with (framing lives on the component).
    struct UartCompDraft {
        name: String,
        baud: u32,
        parity: String,
        stop_bits: String,
    }

    struct UartDraft {
        /// The `<uart>` id (`uart1`), which is also the peripheral id.
        id: String,
        peripheral: String,
        tx: u8,
        rx: u8,
        /// The component riding on this uart, if any.
        component: Option<UartCompDraft>,
    }

    let sections = &c.fe.parsed.sections;
    if sections.get("devices").is_some_and(|v| !v.is_empty()) {
        return Err(
            "`# devices` is not supported by `build` yet (only <gpio> peripherals with <led> and <button> components, <i2c> peripherals with <i2c_component> components, <spi> peripherals with <spi_component> components, and the <wifi> service)"
                .to_string(),
        );
    }

    // id -> (pin, direction)
    let mut gpios: Vec<(&str, u8, Option<&str>)> = Vec::new();
    // `<i2c>` peripherals, in spec order; filled in by the next loop.
    let mut i2c_buses: Vec<I2cBusDraft> = Vec::new();
    // `<spi>` peripherals, in spec order.
    let mut spi_buses: Vec<SpiBusDraft> = Vec::new();
    // `<uart>` peripherals, in spec order.
    let mut uarts: Vec<UartDraft> = Vec::new();
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
                Some(f) => parse_khz(f, "i2c", n.id)?,
            };
            i2c_buses.push(I2cBusDraft {
                id: n.id.to_string(),
                peripheral: i2c_peripheral_name(n.id)?,
                sda: pin_attr("sda")?,
                scl: pin_attr("scl")?,
                khz,
                used: None,
            });
        } else if n.tag_name == "spi" {
            // The runtime `SpiBus::build` is typed on `SPI2`; every chip
            // espforge supports exposes it (GPIO matrix: any pin works).
            if n.id != "spi2" {
                return Err(format!("<spi id=\"{}\">: the id must be spi2", n.id));
            }
            let pin_attr = |what: &str| {
                n.get_attr(what)
                    .and_then(|p| p.parse::<u8>().ok())
                    .ok_or_else(|| format!("<spi id=\"{}\"> needs a numeric {what}=\"...\"", n.id))
            };
            let mode = match n.get_attr("mode") {
                None => 0,
                Some("0") => 0,
                Some("1") => 1,
                Some("2") => 2,
                Some("3") => 3,
                Some(other) => {
                    return Err(format!(
                        "<spi id=\"{}\"> mode=\"{other}\" must be 0, 1, 2 or 3",
                        n.id
                    ));
                }
            };
            let khz = match n.get_attr("frequency") {
                // The `SpiConfig` default in espforge-runtime (1 MHz).
                None => 1000,
                Some(f) => parse_khz(f, "spi", n.id)?,
            };
            spi_buses.push(SpiBusDraft {
                id: n.id.to_string(),
                sclk: pin_attr("sclk")?,
                mosi: pin_attr("mosi")?,
                miso: pin_attr("miso")?,
                khz,
                mode,
                used: false,
            });
        } else if n.tag_name == "uart" {
            let peripheral = match n.get_attr("uart") {
                // Explicit physical peripheral (`uart="UART1"`); the id is
                // then just a reference name.
                Some(raw) => uart_attr_peripheral(raw, n.id)?,
                // Backwards compatible: the id itself names the peripheral.
                None => uart_peripheral_name(n.id)?,
            };
            let pin_attr = |what: &str| {
                n.get_attr(what)
                    .and_then(|p| p.parse::<u8>().ok())
                    .ok_or_else(|| format!("<uart id=\"{}\"> needs a numeric {what}=\"...\"", n.id))
            };
            uarts.push(UartDraft {
                id: n.id.to_string(),
                peripheral,
                tx: pin_attr("tx")?,
                rx: pin_attr("rx")?,
                component: None,
            });
        } else {
            return Err(format!(
                "<{}> in `# peripherals` is not supported by `build` yet (only <gpio>, <i2c>, <spi> and <uart>)",
                n.tag_name
            ));
        }
    }

    // name, bus id, cs pin
    let mut spi_devices: Vec<SpiDeviceDraft> = Vec::new();
    let mut leds: Vec<LedSpec> = Vec::new();
    let mut buttons: Vec<ButtonSpec> = Vec::new();
    for n in sections.get("components").map(Vec::as_slice).unwrap_or(&[]) {
        if n.tag_name == "i2c_component" {
            let bus_ref = n.get_attr("i2c").ok_or_else(|| {
                format!("<i2c_component id=\"{}\"> needs i2c=\"$<i2c id>\"", n.id)
            })?;
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
        if n.tag_name == "spi_component" {
            let bus_ref = n.get_attr("spi").ok_or_else(|| {
                format!("<spi_component id=\"{}\"> needs spi=\"$<spi id>\"", n.id)
            })?;
            let target = bus_ref.trim_start_matches('$');
            if !spi_buses.iter().any(|b| b.id == target) {
                return Err(format!(
                    "<spi_component id=\"{}\"> refers to `{target}`, which is not an <spi> in `# peripherals`",
                    n.id
                ));
            }
            let cs = n
                .get_attr("cs")
                .and_then(|p| p.parse::<u8>().ok())
                .ok_or_else(|| {
                    format!("<spi_component id=\"{}\"> needs a numeric cs=\"...\"", n.id)
                })?;
            let name = n.id.replace('-', "_");
            if !is_rust_ident(&name) {
                return Err(format!(
                    "<spi_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    n.id
                ));
            }
            if let Some(other) = spi_devices.iter().find(|d| d.cs == cs) {
                return Err(format!(
                    "<spi_component id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{cs}",
                    other.name, name
                ));
            }
            spi_buses
                .iter_mut()
                .find(|b| b.id == target)
                .expect("checked above")
                .used = true;
            spi_devices.push(SpiDeviceDraft {
                name,
                bus: target.to_string(),
                cs,
            });
            continue;
        }
        if n.tag_name == "uart_component" {
            let uart_ref = n.get_attr("uart").ok_or_else(|| {
                format!("<uart_component id=\"{}\"> needs uart=\"$<uart id>\"", n.id)
            })?;
            let target = uart_ref.trim_start_matches('$');
            let draft = uarts
                .iter_mut()
                .find(|u| u.id == target)
                .ok_or_else(|| {
                    format!(
                        "<uart_component id=\"{}\"> refers to `{target}`, which is not an <uart> in `# peripherals`",
                        n.id
                    )
                })?;
            if let Some(other) = &draft.component {
                return Err(format!(
                    "<uart_component id=\"{}\"> and <uart_component id=\"{}\"> both use <uart id=\"{target}\">",
                    other.name,
                    n.id.replace('-', "_")
                ));
            }
            let baud_raw = n.get_attr("baud_rate").ok_or_else(|| {
                format!("<uart_component id=\"{}\"> needs baud_rate=\"...\"", n.id)
            })?;
            let baud: u32 = baud_raw.parse().map_err(|_| {
                format!(
                    "<uart_component id=\"{}\"> baud_rate=\"{baud_raw}\" is not a number",
                    n.id
                )
            })?;
            if baud == 0 {
                return Err(format!(
                    "<uart_component id=\"{}\"> baud_rate=\"{baud_raw}\" must be greater than 0",
                    n.id
                ));
            }
            let parity = match n.get_attr("parity") {
                None | Some("none") => "None".to_string(),
                Some("even") => "Even".to_string(),
                Some("odd") => "Odd".to_string(),
                Some(other) => {
                    return Err(format!(
                        "<uart_component id=\"{}\"> parity=\"{other}\" must be \"none\", \"even\" or \"odd\"",
                        n.id
                    ));
                }
            };
            let stop_bits = match n.get_attr("stop_bits") {
                None | Some("1") => "_1".to_string(),
                Some("2") => "_2".to_string(),
                Some(other) => {
                    return Err(format!(
                        "<uart_component id=\"{}\"> stop_bits=\"{other}\" must be \"1\" or \"2\"",
                        n.id
                    ));
                }
            };
            let name = n.id.replace('-', "_");
            if !is_rust_ident(&name) {
                return Err(format!(
                    "<uart_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    n.id
                ));
            }
            draft.component = Some(UartCompDraft {
                name,
                baud,
                parity,
                stop_bits,
            });
            continue;
        }
        if n.tag_name == "button" {
            let gpio_ref = n
                .get_attr("gpio")
                .ok_or_else(|| format!("<button id=\"{}\"> needs gpio=\"$<gpio id>\"", n.id))?;
            let target = gpio_ref.trim_start_matches('$');
            let (_, pin, direction) = gpios
                .iter()
                .find(|(id, _, _)| *id == target)
                .ok_or_else(|| {
                    format!(
                        "<button id=\"{}\"> refers to `{target}`, which is not a <gpio> in `# peripherals`",
                        n.id
                    )
                })?;
            if matches!(direction, Some(d) if *d != "input") {
                return Err(format!(
                    "<button id=\"{}\"> needs an input pin, but `{target}` has direction=\"{}\"",
                    n.id,
                    direction.unwrap_or("")
                ));
            }
            let pull_up = match n.get_attr("pull_up") {
                None | Some("false") => false,
                Some("true") => true,
                Some(other) => {
                    return Err(format!(
                        "<button id=\"{}\"> pull_up=\"{other}\" must be \"true\" or \"false\"",
                        n.id
                    ));
                }
            };
            let name = n.id.replace('-', "_");
            if !is_rust_ident(&name) {
                return Err(format!(
                    "<button id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                    n.id
                ));
            }
            if let Some(other) = buttons.iter().find(|b| b.pin == *pin) {
                return Err(format!(
                    "<button id=\"{}\"> and <button id=\"{}\"> both use GPIO{pin}",
                    other.name, n.id
                ));
            }
            if leds.iter().any(|l| l.pin == *pin) {
                return Err(format!(
                    "<led id=\"{}\"> and <button id=\"{}\">: one pin cannot be both input and output",
                    n.id, name
                ));
            }
            buttons.push(ButtonSpec {
                name,
                pin: *pin,
                pull_up,
            });
            continue;
        }
        if n.tag_name != "led" {
            return Err(format!(
                "<{}> in `# components` is not supported by `build` yet (only <led>, <button>, <i2c_component>, <spi_component> and <uart_component>)",
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
        if let Some(b) = buttons.iter().find(|b| b.pin == *pin) {
            return Err(format!(
                "<button id=\"{}\"> and <led id=\"{}\">: one pin cannot be both input and output",
                b.name, n.id
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

    // The same for the three SPI bus pins, and between all buses so no pin is
    // ever owned by two things the generated code drives.
    // A led sharing a pin with an SPI bus pin is a wiring bug just the same.
    let spi_bus_pins = |b: &SpiBusDraft| [b.sclk, b.mosi, b.miso];
    for bus in &spi_buses {
        for pin in spi_bus_pins(bus) {
            for (id, gpio, _) in &gpios {
                if *gpio == pin {
                    return Err(format!(
                        "<gpio id=\"{id}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                        bus.id
                    ));
                }
            }
            for led in &leds {
                if led.pin == pin {
                    return Err(format!(
                        "<led id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                        led.name, bus.id
                    ));
                }
            }
            for button in &buttons {
                if button.pin == pin {
                    return Err(format!(
                        "<button id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                        button.name, bus.id
                    ));
                }
            }
            for i2c in &i2c_buses {
                if pin == i2c.sda || pin == i2c.scl {
                    return Err(format!(
                        "<i2c id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                        i2c.id, bus.id
                    ));
                }
            }
            for other in &spi_buses {
                if other.id != bus.id && spi_bus_pins(other).contains(&pin) {
                    return Err(format!(
                        "<spi id=\"{}\"> and <spi id=\"{}\"> both use GPIO{pin}",
                        bus.id, other.id
                    ));
                }
            }
            // A device's chip-select may not be a bus pin of its own bus
            // either (that would hold CS low permanently).
            for dev in &spi_devices {
                if dev.bus == bus.id && pin == dev.cs {
                    return Err(format!(
                        "<spi_component id=\"{}\">'s cs and <spi id=\"{}\"> both use GPIO{pin}",
                        dev.name, bus.id
                    ));
                }
            }
        }
    }

    // A chip-select may not clash with anything the wiring owns either: the
    // device would be selected (or a bus pin driven) as soon as the frame
    // begins.
    for dev in &spi_devices {
        let pin = dev.cs;
        for (id, gpio, _) in &gpios {
            if *gpio == pin {
                return Err(format!(
                    "<gpio id=\"{id}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                    dev.name
                ));
            }
        }
        for led in &leds {
            if led.pin == pin {
                return Err(format!(
                    "<led id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                    led.name, dev.name
                ));
            }
        }
        for button in &buttons {
            if button.pin == pin {
                return Err(format!(
                    "<button id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                    button.name, dev.name
                ));
            }
        }
        for i2c in &i2c_buses {
            if pin == i2c.sda || pin == i2c.scl {
                return Err(format!(
                    "<i2c id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                    i2c.id, dev.name
                ));
            }
        }
        for bus in &spi_buses {
            if spi_bus_pins(bus).contains(&pin) {
                return Err(format!(
                    "<spi id=\"{}\"> and <spi_component id=\"{}\"> both use GPIO{pin}",
                    bus.id, dev.name
                ));
            }
        }
    }

    // Two `<uart>`s claiming one physical peripheral would clash at init:
    // `Peripherals.UART1` (say) can only be consumed once.
    for i in 0..uarts.len() {
        for other in &uarts[..i] {
            if other.peripheral == uarts[i].peripheral {
                return Err(format!(
                    "<uart id=\"{}\"> and <uart id=\"{}\"> both use {}",
                    other.id, uarts[i].id, uarts[i].peripheral
                ));
            }
        }
    }

    // UART pins are owned wiring, exactly like the I2C and SPI pins above:
    // nothing else may drive or listen on a UART's TX/RX, and full-duplex TX
    // and RX may not even share a pin with each other.
    for uart in &uarts {
        if uart.tx == uart.rx {
            return Err(format!(
                "<uart id=\"{}\"> tx and rx cannot both be GPIO{}",
                uart.id, uart.tx
            ));
        }
        for pin in [uart.tx, uart.rx] {
            for (id, gpio, _) in &gpios {
                if *gpio == pin {
                    return Err(format!(
                        "<gpio id=\"{id}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        uart.id
                    ));
                }
            }
            for led in &leds {
                if led.pin == pin {
                    return Err(format!(
                        "<led id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        led.name, uart.id
                    ));
                }
            }
            for button in &buttons {
                if button.pin == pin {
                    return Err(format!(
                        "<button id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        button.name, uart.id
                    ));
                }
            }
            for i2c in &i2c_buses {
                if pin == i2c.sda || pin == i2c.scl {
                    return Err(format!(
                        "<i2c id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        i2c.id, uart.id
                    ));
                }
            }
            for bus in &spi_buses {
                if spi_bus_pins(bus).contains(&pin) {
                    return Err(format!(
                        "<spi id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        bus.id, uart.id
                    ));
                }
            }
            for dev in &spi_devices {
                if dev.cs == pin {
                    return Err(format!(
                        "<spi_component id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        dev.name, uart.id
                    ));
                }
            }
            for other in &uarts {
                if other.id == uart.id {
                    continue;
                }
                if other.tx == pin || other.rx == pin {
                    return Err(format!(
                        "<uart id=\"{}\"> and <uart id=\"{}\"> both use GPIO{pin}",
                        uart.id, other.id
                    ));
                }
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

    let spis: Vec<SpiSpec> = spi_devices
        .iter()
        .map(|d| SpiSpec {
            name: d.name.clone(),
            bus: d.bus.clone(),
            cs: d.cs,
        })
        .collect();

    let spi_bus_specs: Vec<SpiBusSpec> = spi_buses
        .into_iter()
        // A bare `<spi>` with no component generates nothing: the app can only
        // reach the bus through a component anyway.
        .filter(|b| b.used)
        .map(|b| SpiBusSpec {
            id: b.id.clone(),
            peripheral: "SPI2".to_string(),
            sclk: b.sclk,
            mosi: b.mosi,
            miso: b.miso,
            khz: b.khz,
            mode: b.mode,
        })
        .collect();

    let uart_specs: Vec<UartSpec> = uarts
        .into_iter()
        // A bare `<uart>` with no component generates nothing: the app can
        // only reach the uart through a component anyway.
        .filter_map(|u| {
            u.component.map(|c| UartSpec {
                name: c.name,
                peripheral: u.peripheral,
                tx: u.tx,
                rx: u.rx,
                baud: c.baud,
                parity: c.parity,
                stop_bits: c.stop_bits,
            })
        })
        .collect();

    Ok(Hardware {
        leds,
        buttons,
        i2cs,
        spi_buses: spi_bus_specs,
        spis,
        uarts: uart_specs,
        wifi,
    })
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

    const WITH_SPI: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />\n# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n# devices\n# services\n";

    const WITH_UART: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <uart id=\"uart1\" tx=\"6\" rx=\"5\" />\n# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n# devices\n# services\n";

    #[test]
    fn unsupported_hardware_is_valid_but_not_buildable() {
        let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <pwm id=\"pwm0\" />\n# components\n# devices\n# services\n";
        let c = validate(src, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert_eq!(unsupported_section(&c), Some("peripherals"));
        let err = buildable(&c).unwrap_err();
        assert!(err.contains("<pwm>"), "{err}");
    }

    #[test]
    fn a_uart_with_a_component_becomes_hardware() {
        let c = validate(WITH_UART, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert!(buildable(&c).is_ok());
        assert_eq!(
            hardware(&c).unwrap().uarts,
            [UartSpec {
                name: "serial".into(),
                peripheral: "UART1".into(),
                tx: 6,
                rx: 5,
                baud: 9600,
                parity: "None".into(),
                stop_bits: "_1".into(),
            }]
        );
        // Framing attributes flow through too.
        let src = WITH_UART.replace(
            "baud_rate=\"9600\"",
            "baud_rate=\"115200\" parity=\"even\" stop_bits=\"2\"",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let spec = &hardware(&c).unwrap().uarts[0];
        assert_eq!(
            (spec.baud, spec.parity.as_str(), spec.stop_bits.as_str()),
            (115200, "Even", "_2")
        );
    }

    #[test]
    fn a_bare_uart_generates_nothing() {
        let src = WITH_UART.replace(
            "# components\n  <uart_component id=\"serial\" uart=\"$uart1\" baud_rate=\"9600\" />\n",
            "# components\n",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap().uarts.is_empty());
    }

    #[test]
    fn uart_wiring_conflicts_are_refused() {
        // tx and rx on the same pin.
        let err = validate(
            &WITH_UART.replace("tx=\"6\" rx=\"5\"", "tx=\"5\" rx=\"5\""),
            &AnswerArgs::default(),
        )
        .ok()
        .and_then(|c| buildable(&c).err())
        .expect("tx==rx is a wiring bug");
        assert!(err.contains("tx and rx"), "{err}");

        // A second uart sharing the rx pin.
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"uart2\" tx=\"7\" rx=\"5\" />\n# components",
        );
        // `buildable` re-runs `hardware`, so re-validate for the error text.
        let err = validate(&src, &AnswerArgs::default())
            .ok()
            .and_then(|c| buildable(&c).err())
            .expect("shared rx is a wiring bug");
        assert!(err.contains("both use GPIO5"), "{err}");

        // A led on the tx pin.
        let src = WITH_UART.replace(
            "tx=\"6\" rx=\"5\" />\n# components",
            "tx=\"6\" rx=\"5\" />\n  <gpio id=\"gpio6\" pin=\"6\" direction=\"output\" />\n# components",
        )
        .replace(
            "# components\n",
            "# components\n  <led id=\"l\" gpio=\"$gpio6\" />\n",
        );
        let err = validate(&src, &AnswerArgs::default())
            .ok()
            .and_then(|c| buildable(&c).err())
            .expect("led on tx is a wiring bug");
        assert!(err.contains("both use GPIO6"), "{err}");
    }

    #[test]
    fn a_uart_can_select_its_peripheral_explicitly_and_frees_the_id() {
        // `uart="UART1"` names the physical peripheral; the id is then just a
        // reference name.
        let src = WITH_UART
            .replace(
                "<uart id=\"uart1\" tx=\"6\" rx=\"5\" />",
                "<uart id=\"gps\" uart=\"UART1\" tx=\"6\" rx=\"5\" />",
            )
            .replace("uart=\"$uart1\"", "uart=\"$gps\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let spec = &hardware(&c).unwrap().uarts[0];
        assert_eq!(spec.peripheral, "UART1");

        // Lowercase is accepted too (the id spelling no longer matters).
        let src = WITH_UART.replace("uart=\"$uart1\"", "uart=\"$gps\"")
            .replace(
                "<uart id=\"uart1\" tx=\"6\" rx=\"5\" />",
                "<uart id=\"gps\" uart=\"uart1\" tx=\"6\" rx=\"5\" />",
            );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert_eq!(hardware(&c).unwrap().uarts[0].peripheral, "UART1");

        // A nonsensical peripheral name is refused.
        let bad = WITH_UART.replace("tx=\"6\" rx=\"5\"", "uart=\"UART9\" tx=\"6\" rx=\"5\"");
        let c = validate(&bad, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("must be UART0, UART1, UART2 or UART3"), "{err}");

        // Without the attribute the old rule still applies: the id must be
        // `uart0`..`uart3`.
        let src = WITH_UART
            .replace("<uart id=\"uart1\"", "<uart id=\"gps\"")
            .replace("uart=\"$uart1\"", "uart=\"$gps\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("the id must be uart0, uart1, uart2 or uart3"), "{err}");
    }

    #[test]
    fn two_uarts_on_one_peripheral_are_refused() {
        // Both via explicit `uart=` attributes...
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"other\" uart=\"UART1\" tx=\"7\" rx=\"8\" />\n# components",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(
            err.contains("<uart id=\"uart1\">") && err.contains("both use UART1"),
            "{err}"
        );

        // ...and when one derives it from its id.
        let src = WITH_UART.replace(
            "rx=\"5\" />\n# components",
            "rx=\"5\" />\n  <uart id=\"gps\" uart=\"uart1\" tx=\"7\" rx=\"8\" />\n# components",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("both use UART1"), "{err}");
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
        let c = validate(I2C_400K, &AnswerArgs::default())
            .ok()
            .expect("valid");
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
    fn a_spi_component_with_a_bus_becomes_hardware() {
        let c = validate(WITH_SPI, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        assert!(buildable(&c).is_ok());
        let hw = hardware(&c).unwrap();
        assert_eq!(
            hw.spi_buses,
            [SpiBusSpec {
                id: "spi2".into(),
                peripheral: "SPI2".into(),
                sclk: 8,
                mosi: 7,
                miso: 6,
                khz: 100,
                mode: 0,
            }]
        );
        assert_eq!(
            hw.spis,
            [SpiSpec {
                name: "tester".into(),
                bus: "spi2".into(),
                cs: 9,
            }]
        );
    }

    #[test]
    fn spi_defaults_are_1_mhz_mode_0_and_explicit_attrs_are_parsed() {
        let src = WITH_SPI.replace(" frequency=\"100kHz\"", " mode=\"3\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let bus = &hardware(&c).unwrap().spi_buses[0];
        assert_eq!(bus.khz, 1000);
        assert_eq!(bus.mode, 3);

        let bad = WITH_SPI.replace("frequency=\"100kHz\"", "mode=\"4\"");
        let c = validate(&bad, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("must be 0, 1, 2 or 3"));

        let mhz = WITH_SPI.replace("100kHz", "105MHZ");
        let c = validate(&mhz, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("not supported yet"));
    }

    #[test]
    fn a_spi_component_needs_a_declared_spi() {
        // The ref passes graph resolution (`spi2` becomes a gpio peripheral)
        // but the component then points at a gpio, not an `<spi>`.
        let src = WITH_SPI.replace(
            "<spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" frequency=\"100kHz\" />",
            "<gpio id=\"spi2\" pin=\"8\" direction=\"output\" />",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("not an <spi>"));
    }

    #[test]
    fn spi_ids_must_be_spi2() {
        let src = WITH_SPI
            .replace("<spi id=\"spi2\"", "<spi id=\"bus0\"")
            .replace("spi=\"$spi2\"", "spi=\"$bus0\"");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("the id must be spi2"));
    }

    #[test]
    fn a_bare_spi_without_a_component_generates_nothing() {
        let src = WITH_SPI.replace(
            "# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />\n",
            "# components\n",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap().spis.is_empty());
        assert!(hardware(&c).unwrap().spi_buses.is_empty());
    }

    #[test]
    fn several_spi_devices_may_share_one_bus_but_not_one_cs() {
        let src = WITH_SPI.replace(
            "# devices",
            "  <spi_component id=\"second\" spi=\"$spi2\" cs=\"10\" />\n# devices",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let hw = hardware(&c).unwrap();
        assert_eq!(hw.spis.len(), 2);
        assert_eq!(hw.spi_buses.len(), 1, "bus is emitted once");

        let same_cs = WITH_SPI.replace(
            "# devices",
            "  <spi_component id=\"second\" spi=\"$spi2\" cs=\"9\" />\n# devices",
        );
        let c = validate(&same_cs, &AnswerArgs::default())
            .ok()
            .expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(
            err.contains("tester") && err.contains("second") && err.contains("both use GPIO9"),
            "{err}"
        );
    }

    #[test]
    fn a_spi_component_needs_a_cs_pin() {
        let src = WITH_SPI.replace(" cs=\"9\"", "");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("cs=\"...\""));
    }

    #[test]
    fn spi_pins_and_cs_must_not_clash_with_anything_else() {
        let with_led = WITH_SPI.replace(
            "# peripherals",
            "# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />",
        );
        let with_led = with_led.replace(
            "# components",
            "# components\n  <led id=\"red\" gpio=\"$gpio9\" />",
        );
        let c = validate(&with_led, &AnswerArgs::default())
            .ok()
            .expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(
            err.contains("gpio9") && err.contains("tester") && err.contains("GPIO9"),
            "{err}"
        );

        // a bus pin colliding with an I2C pin
        let both = WITH_BUS
            .replace("sda=\"4\" scl=\"5\"", "sda=\"8\" scl=\"5\"")
            .replace(
                "# peripherals",
                "# peripherals\n  <spi id=\"spi2\" sclk=\"8\" mosi=\"7\" miso=\"6\" />",
            )
            .replace(
                "# components",
                "# components\n  <spi_component id=\"tester\" spi=\"$spi2\" cs=\"9\" />",
            );
        let c = validate(&both, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(
            err.contains("<i2c id=\"i2c0\">") && err.contains("<spi id=\"spi2\">"),
            "{err}"
        );

        // a device's cs on its own bus pin would hold CS low permanently
        let cs_on_bus = WITH_SPI
            .replace(" miso=\"6\"", " miso=\"9\"")
            .replace("cs=\"9\"", "cs=\"9\"");
        let c = validate(&cs_on_bus, &AnswerArgs::default())
            .ok()
            .expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("both use GPIO9"), "{err}");
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

    const WITH_BUTTON: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"input\" />\n# components\n  <button id=\"button\" gpio=\"$gpio9\" pull_up=\"true\" />\n# devices\n# services\n";

    #[test]
    fn a_button_is_buildable_and_becomes_hardware() {
        let c = validate(WITH_BUTTON, &AnswerArgs::default())
            .ok()
            .expect("graph is fine");
        let hw = hardware(&c).unwrap();
        assert_eq!(
            hw.buttons,
            [ButtonSpec {
                name: "button".into(),
                pin: 9,
                pull_up: true
            }]
        );
    }

    #[test]
    fn pull_up_defaults_to_false_and_bad_values_are_refused() {
        let src = WITH_BUTTON.replace(" pull_up=\"true\"", "");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(!hardware(&c).unwrap().buttons[0].pull_up);

        let bad = WITH_BUTTON.replace("pull_up=\"true\"", "pull_up=\"up\"");
        let c = validate(&bad, &AnswerArgs::default()).ok().expect("valid");
        assert!(
            hardware(&c)
                .unwrap_err()
                .contains("must be \"true\" or \"false\"")
        );
    }

    #[test]
    fn a_button_refers_to_a_gpio_peripheral() {
        // Without the gpio ref attribute the graph itself refuses the
        // component (the same controller-link check every component gets).
        let src = WITH_BUTTON.replace(" gpio=\"$gpio9\"", "");
        assert!(
            validate(&src, &AnswerArgs::default()).is_err(),
            "a button without gpio= must fail validation"
        );

        // Referring at something that exists but is not a <gpio>: the ref
        // passes graph resolution (like `spi_component` pointing at a gpio)
        // but the component then rejects it.
        let src = WITH_BUTTON
            .replace(
                "# peripherals",
                "# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />",
            )
            .replace("$gpio9", "$i2c0");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        let err = hardware(&c).unwrap_err();
        assert!(err.contains("which is not a <gpio>"), "{err}");
    }

    #[test]
    fn a_button_on_an_output_pin_is_refused() {
        let src = WITH_BUTTON.replace("input", "output");
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("needs an input pin"));
    }

    #[test]
    fn two_buttons_on_one_pin_are_refused() {
        let src = WITH_BUTTON.replace(
            "# devices",
            "  <button id=\"other\" gpio=\"$gpio9\" />\n# devices",
        );
        let c = validate(&src, &AnswerArgs::default()).ok().expect("valid");
        assert!(hardware(&c).unwrap_err().contains("both use GPIO9"));
    }

    #[test]
    fn a_led_and_a_button_cannot_share_one_pin() {
        // A led needs an output gpio and a button an input one, so sharing a
        // physical pin uses two <gpio> entries with the same pin number.
        for (first, second) in [
            (
                "  <led id=\"red_led\" gpio=\"$gpio9\" />\n  <button id=\"button\" gpio=\"$gpio9b\" />",
                "led before button",
            ),
            (
                "  <button id=\"button\" gpio=\"$gpio9b\" />\n  <led id=\"red_led\" gpio=\"$gpio9\" />",
                "button before led",
            ),
        ] {
            let src = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"gpio9\" pin=\"9\" direction=\"output\" />\n  <gpio id=\"gpio9b\" pin=\"9\" direction=\"input\" />\n# components\n".to_string()
                + first
                + "\n# devices\n# services\n";
            let c = validate(&src, &AnswerArgs::default())
                .ok()
                .unwrap_or_else(|| panic!("{} must validate", second));
            let err = hardware(&c).unwrap_err();
            assert!(err.contains("input and output"), "{second}: {err}");
        }
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
