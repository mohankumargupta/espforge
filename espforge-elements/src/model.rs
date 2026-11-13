//! The board IR: what the `# espforge` section says, and what lowering built.
//!
//! A [`Board`] carries its [`Meta`], the [`ResourceGraph`] of pins it owns and
//! the devices on them. A [`ClaimedPin`] can only be made by
//! [`ResourceGraph::claim`], so a board that holds one cannot have a pin
//! conflict: validation and construction are the same act.
//!
//! That act is **transactional**: a claim is staged, not immediately permanent.
//! [`ResourceGraph`] keeps a journal of inverses, so the lowering dispatch can
//! [`ResourceGraph::checkpoint`] before one element and [`ResourceGraph::rollback`]
//! when that element fails. Without it, the first pin of a two-pin bus that
//! conflicts with its second stayed claimed after the error, and every later
//! element reported a phantom conflict on a pin nobody actually uses. Rolling
//! back keeps "one error per mistake" true: the claims of a failed element are
//! never visible to the next one.
//!
//! The graph is cut to what the elements need. Buses (`add_bus`, `bus`,
//! `bus_by_name`) return in phase 3, shared peripherals (`claim_peripheral`) with
//! the elements that share them, and the bus kinds join [`OwnerKind`] with
//! their buses. Pin devices share the one [`OwnerKind::Pin`] variant.

use espforge_diagnostics::Diagnostic;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::ops::Range;

pub use espforge_schema::{RustExpr, RustPath, RustType};

/// Which async model the generated project uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Blocking,
    Embassy,
}

/// How one bus is driven: esp-hal `Blocking` or `Async`.
///
/// The scheduler and the bus mode are different decisions. A project may be
/// `runtime="embassy"` (Wi-Fi needs it) while a display on an I2C bus talks
/// over a blocking bus, before the executor starts. Phase 4 resolves the mode
/// per bus from the drivers bound to it instead of from the crate feature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BusMode {
    /// esp-hal `Blocking`, shared through a `RefCell`.
    #[default]
    Blocking,
    /// esp-hal `Async`, shared through an `embassy_sync::Mutex`.
    Async,
}

/// The bus modes a driver declares it can be used with (its manifest's
/// `runtime` lines).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSet {
    pub blocking: bool,
    pub asynchronous: bool,
}

impl RuntimeSet {
    /// A blocking-only driver: every driver in this phase.
    pub const BLOCKING: Self = Self {
        blocking: true,
        asynchronous: false,
    };

    pub fn supports(self, mode: BusMode) -> bool {
        match mode {
            BusMode::Blocking => self.blocking,
            BusMode::Async => self.asynchronous,
        }
    }

    /// Declares `async` and not `blocking`.
    pub fn async_only(self) -> bool {
        self.asynchronous && !self.blocking
    }

    /// Declares `blocking` and not `async`.
    pub fn blocking_only(self) -> bool {
        self.blocking && !self.asynchronous
    }
}

impl Default for RuntimeSet {
    fn default() -> Self {
        Self::BLOCKING
    }
}

/// Where `print!`/log output goes (esp-println's backend).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Console {
    /// Plain UART. Works on every chip.
    #[default]
    Uart,
    /// Built-in USB-Serial-JTAG peripheral (not on the original ESP32).
    JtagSerial,
}

/// The flash chip size a `<ota>` element lays its partition table out for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlashSize {
    /// 2 MiB of flash: two 960 KiB app slots.
    Mb2,
    /// 4 MiB of flash: two ~1.94 MiB app slots.
    #[default]
    Mb4,
    /// 8 MiB of flash: two ~3.94 MiB app slots.
    Mb8,
    /// 16 MiB of flash: two ~7.94 MiB app slots.
    Mb16,
}

impl FlashSize {
    /// The size in bytes.
    pub const fn bytes(self) -> u32 {
        match self {
            FlashSize::Mb2 => 2 * 1024 * 1024,
            FlashSize::Mb4 => 4 * 1024 * 1024,
            FlashSize::Mb8 => 8 * 1024 * 1024,
            FlashSize::Mb16 => 16 * 1024 * 1024,
        }
    }

    /// The size an `<ota flash_size>` names (`"2MB"`, `"4MB"`, `"8MB"`, `"16MB"`).
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "2MB" => Some(FlashSize::Mb2),
            "4MB" => Some(FlashSize::Mb4),
            "8MB" => Some(FlashSize::Mb8),
            "16MB" => Some(FlashSize::Mb16),
            _ => None,
        }
    }
}

/// What `<ota />` asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OtaMeta {
    /// The physical flash size the default partition table is laid out for.
    pub flash_size: FlashSize,
    /// Whether a freshly booted image must call `mark_app_valid()`.
    pub rollback: bool,
}

/// A log level, in the `<logger>` element's vocabulary.
///
/// The order is verbosity, least first: `Off` disables logging and `Trace` is
/// the most detailed. The emitter prints a level as
/// `log::LevelFilter::<Variant>`, and `ESP_LOG` spells `Off` as `off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Every spelling `<logger level>` accepts, in schema order.
    pub const CHOICES: [&'static str; 6] = ["none", "error", "warn", "info", "debug", "trace"];

    /// The level an attribute value names (`"none"` is [`LogLevel::Off`]).
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "none" => LogLevel::Off,
            "error" => LogLevel::Error,
            "warn" => LogLevel::Warn,
            "info" => LogLevel::Info,
            "debug" => LogLevel::Debug,
            "trace" => LogLevel::Trace,
            _ => return None,
        })
    }

    /// The spelling `ESP_LOG` uses (`off`, not `none`).
    pub fn esp_log(self) -> &'static str {
        match self {
            LogLevel::Off => "off",
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }

    /// The `log::LevelFilter` variant the generated code names.
    pub fn variant(self) -> &'static str {
        match self {
            LogLevel::Off => "Off",
            LogLevel::Error => "Error",
            LogLevel::Warn => "Warn",
            LogLevel::Info => "Info",
            LogLevel::Debug => "Debug",
            LogLevel::Trace => "Trace",
        }
    }
}

/// The target prefixes the `drivers` category owns.
///
/// A prefix matches a `log` target at a module boundary (`esp_hal` matches
/// `esp_hal::gpio` but not `esp_halo`), so a rule names a crate once and covers
/// every module in it.
const DRIVER_TARGETS: &[&str] = &["espforge_drivers", "ssd1306", "sky_ili9341"];

/// The prefixes `network` owns: esp-radio's baseband, the two protocol stacks
/// and the runtime's Wi-Fi station.
const NETWORK_TARGETS: &[&str] = &[
    "esp_radio",
    "smoltcp",
    "embassy_net",
    "espforge_runtime::hw::wifi",
];

/// The prefixes `hal` owns: the silicon layer the generated `main()` sits on.
const HAL_TARGETS: &[&str] = &[
    "esp_hal",
    "esp_rtos",
    "esp_bootloader_esp_idf",
    "esp_println",
    "esp_alloc",
    "esp_backtrace",
    "esp_hal_ota",
    "esp_storage",
];

/// What `<logger>` asked for.
///
/// `None` on [`Meta`] means the project did not declare the element: the
/// generated `main()` keeps esp-println's `ESP_LOG` logger, which is what
/// every project had before `<logger>` existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoggerConfig {
    /// The level for the app and anything no category rule overrides.
    pub level: LogLevel,
    /// `espforge-drivers` and the vendor crates they wrap; `None` inherits
    /// [`Self::level`].
    pub drivers: Option<LogLevel>,
    /// The network stack (`esp-radio`, `smoltcp`, `embassy-net`, the Wi-Fi
    /// station); `None` inherits.
    pub network: Option<LogLevel>,
    /// The silicon layer (`esp-hal`, `esp-rtos`, the bootloader, esp-println);
    /// `None` inherits.
    pub hal: Option<LogLevel>,
}

impl Default for LoggerConfig {
    /// The schema's defaults: `level="info"`, `drivers`/`network="inherit"`,
    /// `hal="warn"`.
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            drivers: None,
            network: None,
            hal: Some(LogLevel::Warn),
        }
    }
}

impl LoggerConfig {
    /// Whether `<logger level="none" />` asked for silence: the emitter strips
    /// `log` at compile time and installs no logger at all.
    pub fn silenced(self) -> bool {
        self.level == LogLevel::Off
    }

    /// `(target prefix, level)` pairs for the runtime filter, most specific
    /// first.
    ///
    /// A category that inherits the default needs no rule: the default already
    /// covers it, and an inherited level would only repeat it.
    pub fn filters(self) -> Vec<(&'static str, LogLevel)> {
        if self.silenced() {
            return Vec::new();
        }
        let mut filters = Vec::new();
        for (level, targets) in [
            (self.drivers, DRIVER_TARGETS),
            (self.network, NETWORK_TARGETS),
            (self.hal, HAL_TARGETS),
        ] {
            let Some(level) = level else { continue };
            if level == self.level {
                continue;
            }
            filters.extend(targets.iter().map(|target| (*target, level)));
        }
        filters
    }

    /// The `ESP_LOG` string esp-println's parser reads.
    ///
    /// Kept in step with the generated logger so a project that goes back to
    /// `init_logger_from_env()` (or reads the value through `esp-config`)
    /// filters the same way.
    pub fn esp_log(self) -> String {
        let mut text = self.level.esp_log().to_string();
        for (target, level) in self.filters() {
            text.push_str(&format!(",{target}={}", level.esp_log()));
        }
        text
    }
}

/// Contents of the `# espforge` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: String,
    pub description: Option<String>,
    pub chip: String,
    pub runtime: Runtime,
    pub console: Console,
    /// `Some` when the project asked for `<ota />`.
    pub ota: Option<OtaMeta>,
    /// `Some` when the project declared `<logger>`.
    pub logger: Option<LoggerConfig>,
}

impl Meta {
    /// The project name as a valid crate/package identifier
    /// (`"example project"` -> `example_project`).
    pub fn crate_name(&self) -> String {
        let slug: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let slug = slug.trim_matches('_').to_string();
        if slug.is_empty() {
            "espforge_project".to_string()
        } else if slug.starts_with(|c: char| c.is_ascii_digit()) {
            format!("p_{slug}")
        } else {
            slug
        }
    }
}

/// An element id: the text as written, plus where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    /// The id exactly as written (`red-led`).
    pub raw: String,
    /// Span of the id attribute's value.
    pub span: Range<usize>,
}

impl Ident {
    pub fn new(raw: impl Into<String>, span: Range<usize>) -> Self {
        Self {
            raw: raw.into(),
            span,
        }
    }

    /// The id as written.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The id as a Rust identifier (`red-led` -> `red_led`).
    pub fn rust_name(&self) -> String {
        self.raw.replace('-', "_")
    }
}

/// A GPIO number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pin(pub u8);

/// What a pin is asked to do. Two claims that demand opposite directions are a
/// wiring error; anything else on an occupied pin is a plain conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinUse {
    Input,
    Output,
    /// A bus pin that both sides drive (I2C SDA/SCL), a pin the peripheral
    /// samples at its own rate (SPI MISO), or a driver line that switches
    /// between driving and reading.
    Bidirectional,
    /// No declared direction (`<gpio>` without `direction`).
    Unspecified,
}

impl PinUse {
    /// Whether a claim for `self` and one for `other` cannot share a pin
    /// because one needs an input and the other an output.
    pub fn opposes(self, other: Self) -> bool {
        matches!(
            (self, other),
            (PinUse::Input, PinUse::Output) | (PinUse::Output, PinUse::Input)
        )
    }
}

/// The element kinds that can own a resource. The set is closed on purpose:
/// adding `<pwm>` adds one variant, not a string tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    Gpio,
    /// A pin device built from a recipe; carries its schema tag.
    ///
    /// Owned, not `&'static`: a project manifest's tag belongs to the session
    /// that composed it.
    Pin(String),
    I2c,
    Spi,
    Uart,
    /// A device driving its auxiliary pins (`<ili9341>`); carries its schema
    /// tag.
    Device(String),
    /// The on-chip Wi-Fi radio (`<wifi>`), which claims the `WIFI` block.
    Wifi,
}

impl OwnerKind {
    /// The schema tag spelling.
    pub fn tag(&self) -> &str {
        match self {
            OwnerKind::Gpio => "gpio",
            OwnerKind::Pin(tag) => tag,
            OwnerKind::I2c => "i2c",
            OwnerKind::Spi => "spi",
            OwnerKind::Uart => "uart",
            OwnerKind::Device(tag) => tag,
            OwnerKind::Wifi => "wifi",
        }
    }
}

/// Who claims a resource: its kind and its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub kind: OwnerKind,
    pub id: Ident,
}

impl Owner {
    pub fn new(kind: OwnerKind, id: Ident) -> Self {
        Self { kind, id }
    }
}

/// One recorded claim: the owner, the node span a diagnostic points at, and
/// what the pin is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub owner: Owner,
    /// The owning element's node span (`<led ...>`), not the id value.
    pub span: Range<usize>,
    pub need: PinUse,
}

/// A pin that is owned by exactly one claim.
///
/// Fields are private and there is no public constructor: the only way to get
/// one is [`ResourceGraph::claim`], which is what makes an invalid `Board`
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedPin {
    pin: Pin,
    claim: Claim,
}

impl ClaimedPin {
    pub fn pin(&self) -> Pin {
        self.pin
    }

    pub fn owner(&self) -> &Owner {
        &self.claim.owner
    }

    pub fn claim(&self) -> &Claim {
        &self.claim
    }
}

/// Why a second claim on one pin was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Two elements want the same pin.
    Pin,
    /// One needs an input and the other an output.
    Direction,
    /// One element claims the same pin twice (a UART whose tx and rx match).
    SelfClash,
}

/// A refused pin claim, carrying both sides so the lowering code can keep the
/// exact wording users see today (which side is underlined, which is named
/// first) without re-checking ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub pin: Pin,
    /// The claim that already owns the pin.
    pub existing: Claim,
    /// The claim that was refused.
    pub incoming: Claim,
}

/// Where a bus lives in [`ResourceGraph::buses`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BusId(pub usize);

/// A bus and the pins it owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bus {
    I2c {
        id: Ident,
        /// Upper-cased esp-hal peripheral name (`I2C0`, `I2C1`).
        peripheral: String,
        sda: ClaimedPin,
        scl: ClaimedPin,
        /// Bus frequency in kHz.
        khz: u32,
    },
    Spi {
        id: Ident,
        /// Upper-cased esp-hal peripheral name (`SPI2`).
        peripheral: String,
        sclk: ClaimedPin,
        mosi: ClaimedPin,
        /// `None` is a write-only bus (no MISO pin).
        miso: Option<ClaimedPin>,
        /// Bus frequency in kHz.
        khz: u32,
        mode: u8,
        /// Optional chip select for direct raw-bus use. A device declares its
        /// own `cs` instead, so this is only present on a bus with no driver.
        cs: Option<ClaimedPin>,
    },
    Uart {
        id: Ident,
        /// Upper-cased esp-hal peripheral name (`UART0`, `UART1`), which may
        /// differ from the id when `uart="UART1"` is written explicitly.
        peripheral: String,
        tx: ClaimedPin,
        rx: ClaimedPin,
        /// Line speed for direct raw-bus use.
        baud: u32,
        /// Framing for direct raw-bus use.
        framing: Framing,
    },
}

impl Bus {
    /// The bus id as the user wrote it.
    pub fn id(&self) -> &Ident {
        match self {
            Bus::I2c { id, .. } | Bus::Spi { id, .. } | Bus::Uart { id, .. } => id,
        }
    }
}

/// One claim on a named hardware peripheral (`UART1`, `I2C0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralClaim {
    pub owner: Owner,
    /// The claiming element's node span.
    pub span: Range<usize>,
}

/// Two elements claiming the same hardware peripheral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralClash {
    pub peripheral: String,
    pub existing: PeripheralClaim,
    pub incoming: PeripheralClaim,
}

/// A resource the generated runtime takes before any element is lowered.
///
/// The runtime's own `main()` binds things no element can see: the console's
/// UART and, under embassy, the timer group and interrupt line the scheduler
/// runs on. Recording them here turns a spec that asks for the same silicon
/// into a located error instead of two users of one peripheral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldResource {
    /// Who holds it, in a diagnostic's words (`the log console`).
    pub holder: String,
    /// What the user can do instead, as the second half of the label.
    pub remedy: String,
}

/// One element's claim on a peripheral the generated runtime already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldClash {
    pub peripheral: String,
    pub held: HeldResource,
    /// The claim that was refused.
    pub owner: Owner,
}

/// Why a peripheral claim was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeripheralRefusal {
    /// Another element named the same hardware peripheral.
    Clash(PeripheralClash),
    /// The generated runtime already holds it.
    Held(HeldClash),
}

/// The inverse of one recorded claim, pushed when the claim succeeds.
///
/// Kept private to this module: the journal is an implementation detail of
/// [`ResourceGraph::rollback`], not part of the board IR.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Undo {
    Pin(Pin),
    Peripheral(String),
    /// Buses are appended, so their ids are positional and the inverse of an
    /// append is a pop.
    Bus,
}

/// Pins, peripherals and buses: the resources a board owns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceGraph {
    pins: BTreeMap<Pin, Claim>,
    peripherals: BTreeMap<String, PeripheralClaim>,
    /// Names the generated runtime holds (`UART0`, `TIMG0.timer0`), which no
    /// element may claim. Not journaled: they are recorded before any element.
    held: BTreeMap<String, HeldResource>,
    buses: Vec<Bus>,
    /// Inverses of the claims made so far, oldest first.
    undo: Vec<Undo>,
}

impl ResourceGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Index into the undo journal, for a later [`rollback`](Self::rollback).
    ///
    /// Take one immediately before lowering a single element, and roll back
    /// only on that element's error path. Any [`ClaimedPin`] produced after a
    /// checkpoint belongs to that element and must be dropped with it.
    pub fn checkpoint(&self) -> usize {
        self.undo.len()
    }

    /// Undo every claim recorded since `checkpoint`, newest first.
    ///
    /// A checkpoint past the end is a no-op, so rolling back twice is safe.
    pub fn rollback(&mut self, checkpoint: usize) {
        while self.undo.len() > checkpoint {
            match self.undo.pop() {
                Some(Undo::Pin(pin)) => {
                    self.pins.remove(&pin);
                }
                Some(Undo::Peripheral(peripheral)) => {
                    self.peripherals.remove(&peripheral);
                }
                Some(Undo::Bus) => {
                    self.buses.pop();
                }
                None => break,
            }
        }
    }

    /// Drop the journal: a finished board has no elements left to undo.
    ///
    /// Keeps `PartialEq` about the resources a board owns rather than the
    /// history of how they were claimed.
    pub fn seal(mut self) -> Self {
        self.undo.clear();
        self
    }

    /// Claim `pin` for `owner`.
    ///
    /// A free pin is recorded and returned. An occupied one is refused, and
    /// the graph is unchanged: a failed claim never steals the pin from the
    /// first owner.
    pub fn claim(
        &mut self,
        pin: Pin,
        owner: Owner,
        span: Range<usize>,
        need: PinUse,
    ) -> Result<ClaimedPin, Conflict> {
        let incoming = Claim { owner, span, need };

        if let Some(existing) = self.pins.get(&pin) {
            let kind = if existing.owner.kind == incoming.owner.kind
                && existing.owner.id == incoming.owner.id
            {
                ConflictKind::SelfClash
            } else if existing.need.opposes(incoming.need) {
                ConflictKind::Direction
            } else {
                ConflictKind::Pin
            };
            return Err(Conflict {
                kind,
                pin,
                existing: existing.clone(),
                incoming,
            });
        }

        self.pins.insert(pin, incoming.clone());
        self.undo.push(Undo::Pin(pin));
        Ok(ClaimedPin {
            pin,
            claim: incoming,
        })
    }

    /// Claim a named hardware peripheral (`UART1`, `I2C0`) for `owner`.
    ///
    /// A second claim is the "peripheral clash" case: the board drives at
    /// most one device per physical peripheral. A name the generated runtime
    /// holds ([`Self::hold`]) is refused first, so the runtime's claim cannot
    /// be overwritten by an element.
    pub fn claim_peripheral(
        &mut self,
        peripheral: impl Into<String>,
        owner: Owner,
        span: Range<usize>,
    ) -> Result<(), PeripheralRefusal> {
        let peripheral = peripheral.into();
        if let Some(held) = self.held.get(&peripheral) {
            return Err(PeripheralRefusal::Held(HeldClash {
                peripheral,
                held: held.clone(),
                owner,
            }));
        }
        let incoming = PeripheralClaim { owner, span };
        if let Some(existing) = self.peripherals.get(&peripheral) {
            return Err(PeripheralRefusal::Clash(PeripheralClash {
                peripheral,
                existing: existing.clone(),
                incoming,
            }));
        }
        self.undo.push(Undo::Peripheral(peripheral.clone()));
        self.peripherals.insert(peripheral, incoming);
        Ok(())
    }

    /// Record a name the generated runtime holds, before any element lowers.
    ///
    /// Not journaled: what the runtime takes follows from the project meta, and
    /// no element would undo it. A later claim on the name is
    /// [`PeripheralRefusal::Held`].
    pub fn hold(
        &mut self,
        resource: impl Into<String>,
        holder: impl Into<String>,
        remedy: impl Into<String>,
    ) {
        self.held.insert(
            resource.into(),
            HeldResource {
                holder: holder.into(),
                remedy: remedy.into(),
            },
        );
    }

    /// What holds `resource`, if the generated runtime does.
    pub fn held(&self, resource: &str) -> Option<&HeldResource> {
        self.held.get(resource)
    }

    /// Every runtime-held resource, in name order.
    pub fn held_resources(&self) -> impl Iterator<Item = (&str, &HeldResource)> {
        self.held.iter().map(|(name, held)| (name.as_str(), held))
    }

    /// Append a bus and return its id. Bus ids are positional and assigned in
    /// construction order (source order).
    pub fn add_bus(&mut self, bus: Bus) -> BusId {
        let id = BusId(self.buses.len());
        self.buses.push(bus);
        self.undo.push(Undo::Bus);
        id
    }

    pub fn bus(&self, id: BusId) -> Option<&Bus> {
        self.buses.get(id.0)
    }

    pub fn bus_mut(&mut self, id: BusId) -> Option<&mut Bus> {
        self.buses.get_mut(id.0)
    }

    pub fn buses(&self) -> &[Bus] {
        &self.buses
    }

    /// The bus whose id is `name`, in declaration order.
    pub fn bus_by_name(&self, name: &str) -> Option<BusId> {
        self.buses
            .iter()
            .position(|bus| bus.id().as_str() == name)
            .map(BusId)
    }

    /// The claim on `pin`, if any.
    pub fn claim_of(&self, pin: Pin) -> Option<&Claim> {
        self.pins.get(&pin)
    }

    /// Every pin claim, in pin order (the map is a `BTreeMap`).
    pub fn pin_claims(&self) -> impl Iterator<Item = (Pin, &Claim)> {
        self.pins.iter().map(|(&pin, claim)| (pin, claim))
    }

    /// Every peripheral claim, in name order.
    pub fn peripheral_claims(&self) -> impl Iterator<Item = (&str, &PeripheralClaim)> {
        self.peripherals
            .iter()
            .map(|(name, claim)| (name.as_str(), claim))
    }
}

/// The `direction` a `<gpio>` declared, or a device's pin needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Output,
    Input,
    /// A line the driver switches between driving and reading.
    Bidirectional,
}

/// A lowered `<gpio>`: an id, a pin, and the declared direction, if any.
///
/// It claims nothing; the first device that uses the pin does, as in the
/// legacy lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpioDecl {
    pub id: Ident,
    pub pin: u8,
    pub direction: Option<Direction>,
    /// The `<gpio ...>` node's span, for pin-conflict diagnostics.
    pub span: Range<usize>,
}

/// UART parity, in the IR's own vocabulary. The lowering maps the generated
/// schema choice onto this; the emitter maps it onto `esp_hal`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Parity {
    #[default]
    None,
    Even,
    Odd,
}

/// UART stop bits, in the IR's own vocabulary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StopBits {
    #[default]
    One,
    Two,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Framing {
    pub parity: Parity,
    pub stop_bits: StopBits,
}

/// A GPIO-backed device lowered from `# devices` (`<led>`, `<button>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinDevice {
    pub id: Ident,
    /// The schema tag, which names the recipe that built this.
    pub tag: String,
    pub pin: ClaimedPin,
    /// The Rust type, resolved from the schema.
    pub runtime: RustPath,
    /// The direction-specific configuration the recipe read.
    pub kind: PinDeviceKind,
}

/// The configuration a pin recipe reads from its boolean attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinDeviceKind {
    Output { active_low: bool },
    Input { pull_up: bool },
}

/// A device lowered from `# devices`.
///
/// The transport split and the typed IR made the bus variant smaller but did
/// not lift the `large_enum_variant` note: `BusDevice` is still 592 bytes
/// against 168 for `PinDevice`, because the driver record itself is large.
/// That remaining size is recorded here rather than silenced with an `allow`;
/// boxing the record is a separate change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Device {
    /// A GPIO-backed device such as an LED or button.
    Pin(PinDevice),
    /// A driver bound to an I2C or SPI bus.
    Bus(BusDevice),
}

impl Device {
    /// The device's id.
    pub fn id(&self) -> &Ident {
        match self {
            Device::Pin(device) => &device.id,
            Device::Bus(device) => &device.id,
        }
    }

    /// The schema tag of the element this device came from.
    pub fn tag(&self) -> &str {
        match self {
            Device::Pin(device) => &device.tag,
            Device::Bus(device) => &device.tag,
        }
    }

    pub fn as_pin(&self) -> Option<&PinDevice> {
        match self {
            Device::Pin(device) => Some(device),
            Device::Bus(_) => None,
        }
    }

    pub fn as_bus(&self) -> Option<&BusDevice> {
        match self {
            Device::Bus(device) => Some(device),
            Device::Pin(_) => None,
        }
    }
}

impl crate::element::Lowered for Device {
    fn id(&self) -> &Ident {
        self.id()
    }

    fn emit(&self, board: &Board, cx: &mut crate::element::EmitCx) {
        match self {
            Device::Pin(device) => device.emit(board, cx),
            Device::Bus(device) => device.emit(board, cx),
        }
    }

    fn overlay_attr(&self, attr: &str) -> Option<String> {
        match self {
            Device::Pin(device) => device.overlay_attr(attr),
            Device::Bus(_) => None,
        }
    }
}

/// A service in the board: software that rides on a network provider, not a
/// pin. Lowered after the devices, so its `network=` can refer to one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Service {
    /// The `<wifi_station>` provider.
    WifiStation(crate::services::wifi_station::WifiStationService),
    /// A service built from a `service_recipe`.
    Protocol(ProtocolService),
}

/// A service built from a `service_recipe`.
///
/// Every `service_recipe` lowers to this shape. The type, the constructor, the
/// config fields and the feature are strings the schema itself supplied, so
/// adding a service edits the runtime and the schema, never this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolService {
    pub id: Ident,
    /// The schema tag, which names the recipe that built this.
    pub tag: String,
    /// The service provider whose network stack the service rides on.
    pub network: Ident,
    /// The `espforge-runtime` feature the generated project names.
    pub feature: String,
    /// The `Services` field type.
    pub ty: RustType,
    /// The Rust path of the constructor.
    pub new: RustPath,
    /// The `static` storage type the constructor takes first, if the service
    /// owns mutable state (`espforge_runtime::HttpClientStorage`).
    pub storage: Option<RustType>,
    /// The config struct passed to `new`, if the recipe declares one.
    pub config: Option<RustType>,
    /// The heap the service needs in `main.rs`, in bytes.
    pub heap_bytes: Option<u32>,
    /// `(field, expression)` pairs, in config-struct declaration order.
    pub config_fields: Vec<(String, RustExpr)>,
    /// The Rust expressions passed to `new` after the config.
    pub extra_args: Vec<RustExpr>,
}

impl ProtocolService {
    /// The service's id.
    pub fn id(&self) -> &Ident {
        &self.id
    }
}

impl crate::element::Lowered for ProtocolService {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, _board: &Board, cx: &mut crate::element::EmitCx) {
        let name = format_ident!("{}", self.id.rust_name());
        let ty = &self.ty;
        let new = &self.new;
        // A service with mutable state gets its own generated `static`, passed
        // as the constructor's first argument (and a comma, only when present).
        let storage = self.storage.as_ref().map(|ty| {
            let name = format_ident!("__{}_STORAGE", self.id.rust_name().to_uppercase());
            cx.module_item(quote! {
                static #name: #ty = #ty::new();
            });
            quote! { &#name, }
        });
        let stack = crate::services::network_stack(&self.network);
        let extra: Vec<&RustExpr> = self.extra_args.iter().collect();

        let init = match &self.config {
            Some(config) => {
                let fields: Vec<TokenStream> = self
                    .config_fields
                    .iter()
                    .map(|(field, expr)| {
                        let field = format_ident!("{field}");
                        quote! { #field: #expr, }
                    })
                    .collect();
                quote! {
                    #new(
                        #storage
                        #stack,
                        #config {
                            #(#fields)*
                        },
                        #(#extra),*
                    )
                }
            }
            None => quote! {
                #new(#storage #stack, #(#extra),*)
            },
        };

        cx.service_field(quote! {
            pub #name: #ty,
        });
        // Build into a local first: a client borrows its provider
        // (`home.stack()`) and a struct literal evaluates its fields in source
        // order, so moving the provider before the client is constructed
        // would borrow a moved value.
        cx.service_prelude(quote! {
            let #name = #init;
        });
        cx.service_init(quote! {
            #name: #name,
        });
    }
}

impl Service {
    pub fn id(&self) -> &Ident {
        match self {
            Service::WifiStation(service) => &service.id,
            Service::Protocol(service) => &service.id,
        }
    }

    /// The `espforge-runtime` feature this service needs, if any.
    pub fn feature(&self) -> Option<&str> {
        match self {
            Service::WifiStation(_) => None,
            Service::Protocol(service) => Some(service.feature.as_str()),
        }
    }

    /// The heap this service needs, if any.
    pub fn heap_bytes(&self) -> Option<u32> {
        match self {
            Service::WifiStation(_) => None,
            Service::Protocol(service) => service.heap_bytes,
        }
    }
}

impl crate::element::Lowered for Service {
    fn id(&self) -> &Ident {
        self.id()
    }

    fn emit(&self, board: &Board, cx: &mut crate::element::EmitCx) {
        match self {
            Service::WifiStation(service) => service.emit(board, cx),
            Service::Protocol(service) => service.emit(board, cx),
        }
    }
}

/// When a device is brought up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Init {
    /// Built and initialised in `Devices::new`, before `app::setup`.
    #[default]
    Boot,
    /// Built after boot, when the app calls `Deferred::init`.
    Deferred,
}

/// A device's own SPI transfer configuration: the mode and clock it needs,
/// applied by its endpoint before every transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpiConfig {
    /// Clock in kHz.
    pub khz: u32,
    /// SPI mode, `0..=3`.
    pub mode: u8,
}

/// Which level asserts an SPI device's chip select.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CsPolarity {
    #[default]
    ActiveLow,
    ActiveHigh,
}

/// One constructor argument a deferred device stores in its factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryArg {
    /// A value recomputed after boot: a mapped spelling (`Oled128x64`) or the
    /// address the lowerer consumed.
    Constant(RustExpr),
    /// A control line built during `Devices::new` and moved into the factory.
    Pin {
        name: String,
        optional: bool,
        /// The wrapper the factory's field stores.
        direction: Direction,
    },
}

/// A bus-backed device: a driver for a specific part, plus one concrete bus
/// transport.
///
/// Every `device_recipe` lowers to this shape, and a hand-written `Element`
/// can build one too, so a driver that needs its own Rust file still emits its
/// `Devices` field and initialiser the same way. The recipe is data: the type,
/// the constructor and the arguments are strings the schema itself supplied.
///
/// The transport is a sum, not a set of independent optionals: an I2C device
/// has an address and the fields emission reads are the ones its transport
/// says are meaningful.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusDevice {
    pub id: Ident,
    /// The schema tag, which names the recipe that built this.
    pub tag: String,
    /// Everything that is independent of the bus kind.
    pub driver: Driver,
    /// The bus and the part's connection to it.
    pub transport: BusTransport,
}

/// The codegen and lifecycle data every bus device has.
///
/// Every Rust fragment the recipe supplied is a parsed node, so emission
/// quotes it and never reparses schema text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Driver {
    /// The `espforge-drivers` feature the generated project names.
    pub feature: String,
    /// Crates the generated project must name because of this driver.
    pub deps: Vec<String>,
    /// The `Devices` field type, its value slots already filled.
    pub ty: RustType,
    /// The Rust path of the constructor.
    pub new: RustPath,
    /// The auxiliary pins it claimed, with the argument each one fills.
    pub pins: Vec<(String, ClaimedPin)>,
    /// The Rust expressions passed to the constructor after the bus.
    pub args: Vec<RustExpr>,
    /// The same arguments as [`Self::args`], split into constants a deferred
    /// factory can recompute and pins it must store.
    pub factory_args: Vec<FactoryArg>,
    /// The bus modes the driver's manifest declares it supports.
    pub runtimes: RuntimeSet,
    /// When the device is brought up.
    pub init: Init,
    /// The async half of the recipe, when the driver declares one.
    pub async_ty: Option<RustType>,
    /// The async constructor.
    pub async_new: Option<RustPath>,
    /// The `espforge-drivers` feature the async type needs.
    pub async_feature: Option<String>,
    /// The external crate implementing this driver, when its manifest declares
    /// a `crate` block. `None` means the bundled `espforge-drivers` feature.
    pub package: Option<espforge_schema::PackageRef>,
    /// Crates the driver's public types come from (`embedded-graphics`).
    pub expose: Vec<espforge_schema::Expose>,
    /// Every crate the manifest names, including the implementation package.
    /// The emitter writes one deduplicated line per package.
    pub dependencies: Vec<espforge_schema::Dependency>,
    /// The manifest file this driver's element was read from, when it is not a
    /// bundled one. A package's relative `path` resolves against it.
    pub manifest: Option<String>,
}

/// The transports a bus device can actually emit.
///
/// Each variant is one concrete connection: an I2C part has an address, an SPI
/// part owns a chip select, and a UART part owns the whole port. A UART port
/// has exactly one peer, so lowering refuses a second device on it rather than
/// emitting two builders for one peripheral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusTransport {
    I2c(I2cTransport),
    Spi(SpiTransport),
    Uart(UartTransport),
}

impl BusTransport {
    /// The bus this device binds to.
    pub fn bus(&self) -> BusId {
        match self {
            BusTransport::I2c(transport) => transport.bus,
            BusTransport::Spi(transport) => transport.bus,
            BusTransport::Uart(transport) => transport.bus,
        }
    }

    /// The endpoint type for the resolved bus mode. An async device falls back
    /// to its blocking endpoint only when the driver declared no async one.
    pub fn endpoint(&self, asynchronous: bool) -> Option<&RustType> {
        let (blocking, async_endpoint) = match self {
            BusTransport::I2c(transport) => (
                transport.endpoint.as_ref(),
                transport.async_endpoint.as_ref(),
            ),
            BusTransport::Spi(transport) => (
                transport.endpoint.as_ref(),
                transport.async_endpoint.as_ref(),
            ),
            BusTransport::Uart(transport) => (
                transport.endpoint.as_ref(),
                transport.async_endpoint.as_ref(),
            ),
        };
        if asynchronous {
            async_endpoint.or(blocking)
        } else {
            blocking
        }
    }
}

/// A device on an I2C bus: the address is part of the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cTransport {
    pub bus: BusId,
    /// The I2C address the device declares (`address="0x3C"`).
    pub address: u8,
    /// The endpoint type a deferred factory stores (blocking half).
    pub endpoint: Option<RustType>,
    /// The endpoint type a deferred async factory stores.
    pub async_endpoint: Option<RustType>,
}

/// A device on a UART port.
///
/// A UART is point to point, so there is no address and no chip select: the
/// device owns the port the `<uart>` declared. The endpoint types are the ones
/// the recipe names, so a deferred factory can store the port it was built
/// with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UartTransport {
    pub bus: BusId,
    /// The endpoint type a deferred factory stores (blocking half).
    pub endpoint: Option<RustType>,
    /// The endpoint type a deferred async factory stores.
    pub async_endpoint: Option<RustType>,
}

/// A device on an SPI bus: it owns its chip select and its own transfer mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiTransport {
    pub bus: BusId,
    /// The chip select the device owns (`cs="9"`).
    pub cs: ClaimedPin,
    /// The device's own SPI mode and clock, when its recipe declares
    /// `spi_config`.
    pub spi_config: Option<SpiConfig>,
    /// The level that asserts this device's chip select.
    pub cs_polarity: CsPolarity,
    /// The endpoint type a deferred factory stores (blocking half).
    pub endpoint: Option<RustType>,
    /// The endpoint type a deferred async factory stores.
    pub async_endpoint: Option<RustType>,
}

impl BusDevice {
    /// The device's id.
    pub fn id(&self) -> &Ident {
        &self.id
    }

    /// The bus this device binds to.
    pub fn bus(&self) -> BusId {
        self.transport.bus()
    }
}

impl crate::element::Lowered for BusDevice {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut crate::element::EmitCx) {
        let BusDevice {
            id,
            tag,
            driver,
            transport,
        } = self;
        let name = format_ident!("{}", id.rust_name());
        let name_text = id.rust_name();
        // A dual-mode driver on an embassy project resolves its bus async; the
        // device then takes the async type and is always deferred.
        let async_bus = board.bus_mode(transport.bus()) == BusMode::Async;
        let ty = if async_bus {
            driver.async_ty.as_ref().unwrap_or(&driver.ty)
        } else {
            &driver.ty
        };
        let new = if async_bus {
            driver.async_new.as_ref().unwrap_or(&driver.new)
        } else {
            &driver.new
        };
        let deferred = async_bus || driver.init == Init::Deferred;
        let mut args: Vec<TokenStream> = driver.args.iter().map(|arg| quote! { #arg }).collect();

        // Build the bus and the device's own handle inside `Devices::new`, so
        // the bus exists exactly once and nothing is exposed through a
        // separate field the app could move out from under the device.
        //
        // The panel address is a constructor argument, not something the
        // endpoint rewrites: the driver's own `I2c` calls reach the address it
        // asks for, so a second part on the same bus is addressable too.
        let handle = match transport {
            BusTransport::I2c(i2c) => {
                let address = Literal::u8_unsuffixed(i2c.address);
                args.insert(0, quote! { #address });
                crate::bus_emit::i2c_device(cx, board, i2c.bus)
            }
            BusTransport::Spi(spi) => {
                // A config-declaring recipe applies its own mode and clock;
                // every other part inherits the bus boot values.
                let (config_khz, config_mode) = spi
                    .spi_config
                    .map(|config| (config.khz, config.mode))
                    .unwrap_or_else(|| match board.resources.bus(spi.bus) {
                        Some(Bus::Spi { khz, mode, .. }) => (*khz, *mode),
                        _ => unreachable!("an SPI transport names an SPI bus"),
                    });
                crate::bus_emit::spi_device(
                    cx,
                    board,
                    spi.bus,
                    spi.cs.pin().0,
                    config_khz,
                    config_mode,
                    spi.cs_polarity,
                )
            }
            // A UART port has one peer, so the device takes the whole port and
            // emission builds it once: lowering already refused a second bind.
            BusTransport::Uart(uart) => crate::bus_emit::uart_device(cx, board, uart.bus),
        };

        // A device constructor is fallible, and the error carries the element
        // id so a boot log names the part that did not answer.
        let device = Literal::string(id.raw.as_str());
        let fallible = quote! {
            .map_err(|kind| espforge_runtime::DeviceError { device: #device, kind })?
        };

        if deferred {
            // A deferred device stores a factory. The endpoint and its control
            // lines are built now; the driver constructor and its I/O run when
            // the app calls `init()`.
            let factory = format_ident!("{}", factory_name(&name_text));
            let endpoint_ty = transport.endpoint(async_bus).unwrap_or_else(|| {
                panic!(
                    "deferred device `<{}>` has no `endpoint` in its device_recipe",
                    tag
                )
            });

            let mut fields = vec![quote! { endpoint: #endpoint_ty }];
            let mut pin_inits = Vec::new();
            let mut call_args = vec![quote! { self.endpoint }];
            for (arg, expression) in driver.factory_args.iter().zip(args.iter()) {
                match arg {
                    FactoryArg::Pin {
                        name,
                        optional,
                        direction,
                    } => {
                        let field = format_ident!("pin_{}", name);
                        let field_ty = match direction {
                            Direction::Output => quote! { esp_hal::gpio::Output<'static> },
                            Direction::Input => quote! { esp_hal::gpio::Input<'static> },
                            Direction::Bidirectional => {
                                quote! { espforge_runtime::Bidirectional }
                            }
                        };
                        let field_ty = if *optional {
                            quote! { Option<#field_ty> }
                        } else {
                            field_ty
                        };
                        fields.push(quote! { #field: #field_ty });
                        pin_inits.push(quote! { #field: #expression });
                        call_args.push(quote! { self.#field });
                    }
                    FactoryArg::Constant(_) => call_args.push(expression.clone()),
                }
            }

            let factory_impl = if async_bus {
                quote! {
                    impl espforge_runtime::AsyncDeferredInit for #factory {
                        type Device = #ty;

                        fn init(
                            self,
                        ) -> impl core::future::Future<
                            Output = Result<Self::Device, espforge_runtime::DeviceError>,
                        > {
                            async move {
                                #new(#(#call_args),*)
                                    .await
                                    .map_err(|kind| espforge_runtime::DeviceError { device: #device, kind })
                            }
                        }
                    }
                }
            } else {
                quote! {
                    impl espforge_runtime::DeferredInit for #factory {
                        type Device = #ty;

                        fn init(self) -> Result<Self::Device, espforge_runtime::DeviceError> {
                            #new(#(#call_args),*)
                                .map_err(|kind| espforge_runtime::DeviceError { device: #device, kind })
                        }
                    }
                }
            };

            cx.module_item(quote! {
                /// Built during `Devices::new`, initialised after boot.
                pub struct #factory {
                    #(#fields,)*
                }

                #factory_impl
            });

            let factory_expr = quote! {
                #factory {
                    endpoint: #handle,
                    #(#pin_inits,)*
                }
            };

            // Every device on an embassy board shares one access shape, so
            // `device!` can always await a lock: an async bus (always
            // deferred) and a synchronous part on the same board differ only
            // in the deferred flavour stored inside the mutex.
            if async_bus {
                cx.device_field(quote! {
                    pub #name: espforge_runtime::AsyncMutex<
                        espforge_runtime::AsyncDeferred<#factory>,
                    >,
                });
                cx.device_init(quote! {
                    #name: espforge_runtime::AsyncMutex::new(
                        espforge_runtime::AsyncDeferred::new(#factory_expr),
                    ),
                });
            } else if board.meta.runtime == Runtime::Embassy {
                cx.device_field(quote! {
                    pub #name: espforge_runtime::AsyncMutex<
                        espforge_runtime::Deferred<#factory>,
                    >,
                });
                cx.device_init(quote! {
                    #name: espforge_runtime::AsyncMutex::new(
                        espforge_runtime::Deferred::new(#factory_expr),
                    ),
                });
            } else {
                cx.device_field(quote! {
                    pub #name: espforge_runtime::Deferred<#factory>,
                });
                cx.device_init(quote! {
                    #name: espforge_runtime::Deferred::new(#factory_expr),
                });
            }
            return;
        }

        if board.meta.runtime == Runtime::Embassy {
            cx.device_field(quote! {
                pub #name: espforge_runtime::AsyncMutex<#ty>,
            });
            cx.device_init(quote! {
                #name: espforge_runtime::AsyncMutex::new(
                    #new(#handle, #(#args),*) #fallible
                ),
            });
        } else {
            cx.device_field(quote! {
                pub #name: #ty,
            });
            cx.device_init(quote! {
                #name: #new(#handle, #(#args),*) #fallible,
            });
        }
    }
}

/// Everything the emitters need: the project settings, the resources it owns,
/// the pin devices and bus devices on them, and the services.
#[derive(Debug, Clone)]
pub struct Board {
    pub meta: Meta,
    pub resources: ResourceGraph,
    /// Pin devices (`<led>`, `<button>`) and bus devices.
    pub devices: Vec<Device>,
    pub services: Vec<Service>,
    /// Warnings collected while lowering. Warnings never stop the build, so
    /// they ride on the board rather than in the error list.
    pub warnings: Vec<Diagnostic>,
}

impl Board {
    /// Distinct service features requested by the board, in declaration order
    /// and without duplicates: what the generated `Cargo.toml` asks
    /// `espforge-runtime` for.
    ///
    /// Source order, not sorted: the emitted feature array keeps the order the
    /// services were declared in, which is what makes a golden diff readable.
    /// The feature is the recipe's own, so adding a service never edits the
    /// manifest backend.
    pub fn service_features(&self) -> Vec<&str> {
        self.services
            .iter()
            .filter_map(Service::feature)
            .fold(Vec::new(), |mut out, feature| {
                if !out.contains(&feature) {
                    out.push(feature);
                }
                out
            })
    }

    /// The largest heap any service needs (`embedded-tls` scratch), in bytes.
    pub fn max_service_heap(&self) -> Option<u32> {
        self.services.iter().filter_map(Service::heap_bytes).max()
    }

    /// The driver features the project needs, in declaration order and without
    /// duplicates: what the generated `Cargo.toml` asks `espforge-drivers` for.
    ///
    /// Source order, not sorted: the emitted feature array keeps the order the
    /// devices were declared in, which is what makes a golden diff readable.
    pub fn driver_features(&self) -> Vec<&str> {
        self.devices
            .iter()
            .filter_map(Device::as_bus)
            // An external package names its own crate in `Cargo.toml`; its
            // feature belongs to that crate, not to `espforge-drivers`.
            .filter(|device| device.driver.package.is_none())
            .map(|device| {
                let asynchronous = self.bus_mode(device.bus()) == BusMode::Async;
                if asynchronous {
                    device
                        .driver
                        .async_feature
                        .as_deref()
                        .unwrap_or(&device.driver.feature)
                } else {
                    device.driver.feature.as_str()
                }
            })
            .fold(Vec::new(), |mut out, feature| {
                if !out.contains(&feature) {
                    out.push(feature);
                }
                out
            })
    }

    /// The mode one bus is driven in, resolved from the drivers bound to it.
    ///
    /// A blocking-only driver forces a blocking bus; an async-only one forces
    /// an async bus (a mix is refused by lowering as `E0909`). A bus no device
    /// binds to follows the scheduler, because it is exposed as a direct raw
    /// handle: an embassy project's `<i2c>` is async, a blocking project's is
    /// blocking.
    pub fn bus_mode(&self, bus: BusId) -> BusMode {
        let mut bound = false;
        let mut every_supports_async = true;
        for device in self
            .devices
            .iter()
            .filter_map(Device::as_bus)
            .filter(|device| device.bus() == bus)
        {
            bound = true;
            if device.driver.runtimes.async_only() {
                return BusMode::Async;
            }
            if !device.driver.runtimes.asynchronous {
                every_supports_async = false;
            }
        }
        if bound {
            // A dual-mode driver follows the project's scheduler: an embassy
            // project can await its transfers, a blocking one keeps the bus
            // blocking. A blocking-only device on the bus keeps it blocking.
            if every_supports_async && self.meta.runtime == Runtime::Embassy {
                BusMode::Async
            } else {
                BusMode::Blocking
            }
        } else if self.meta.runtime == Runtime::Embassy {
            BusMode::Async
        } else {
            BusMode::Blocking
        }
    }

    /// Crates the declared drivers need the project to name, in declaration
    /// order and without duplicates (`embedded-graphics` for a display). A
    /// device whose recipe names no dependency adds none.
    pub fn driver_deps(&self) -> Vec<&str> {
        self.devices
            .iter()
            .filter_map(Device::as_bus)
            .flat_map(|device| device.driver.deps.iter().map(String::as_str))
            .fold(Vec::new(), |mut out, dep| {
                if !out.contains(&dep) {
                    out.push(dep);
                }
                out
            })
    }

    /// The external driver packages this board uses, one per package name.
    ///
    /// Two devices from one crate share one dependency line, so
    /// `Cargo.toml` names each package once.
    pub fn driver_packages(&self) -> Vec<&espforge_schema::PackageRef> {
        self.devices
            .iter()
            .filter_map(Device::as_bus)
            .filter_map(|device| device.driver.package.as_ref())
            .fold(Vec::new(), |mut out, package| {
                if !out
                    .iter()
                    .any(|known: &&espforge_schema::PackageRef| known.package == package.package)
                {
                    out.push(package);
                }
                out
            })
    }

    /// Every dependency the board's drivers name, deduplicated by package.
    ///
    /// A v2 manifest names its full dependency set; a v1 element is converted
    /// from `crate`, `expose` and the bundled `deps` list. Two drivers naming
    /// one package with different sources are refused here, with both manifest
    /// paths, rather than letting Cargo silently pick one.
    pub fn driver_dependencies(&self) -> Result<Vec<espforge_schema::Dependency>, String> {
        let mut out: Vec<espforge_schema::Dependency> = Vec::new();
        let mut from: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        for device in self.devices.iter().filter_map(Device::as_bus) {
            let mut dependencies = device.driver.dependencies.clone();
            if dependencies.is_empty() {
                if let Some(package) = &device.driver.package {
                    dependencies.push(espforge_schema::Dependency::from_package(package));
                }
                dependencies.extend(
                    device
                        .driver
                        .expose
                        .iter()
                        .map(espforge_schema::Dependency::from_expose),
                );
                for dep in &device.driver.deps {
                    if !dependencies.iter().any(|known| &known.package == dep) {
                        dependencies.push(espforge_schema::Dependency {
                            package: dep.clone(),
                            version: None,
                            path: None,
                            git: None,
                            rev: None,
                            features: Vec::new(),
                            default_features: true,
                        });
                    }
                }
            }
            let manifest = device
                .driver
                .manifest
                .clone()
                .unwrap_or_else(|| format!("bundled <{}>", device.tag));
            for dependency in dependencies {
                match out
                    .iter_mut()
                    .find(|known| known.package == dependency.package)
                {
                    None => {
                        from.insert(dependency.package.clone(), manifest.clone());
                        out.push(dependency);
                    }
                    Some(existing) => {
                        let first = from
                            .get(&dependency.package)
                            .map(String::as_str)
                            .unwrap_or("?");
                        if !existing.same_source(&dependency) {
                            let mut detail = String::new();
                            if let (Some(kept), Some(wanted)) =
                                (&existing.version, &dependency.version)
                            {
                                detail = format!(
                                    "\n   '{first}' requires `{kept}`, '{manifest}' requires \
                                     `{wanted}`"
                                );
                            }
                            return Err(format!(
                                "DRIVER_DEPENDENCY_CONFLICT: `{}` is named by '{first}' and \
                                 '{manifest}' with different sources{detail}\n   fix: use one \
                                 source, narrow a requirement, or rename one crate",
                                dependency.package
                            ));
                        }
                        // A compatible second sighting only widens the line:
                        // the stricter requirement, every feature, and default
                        // features while both want them.
                        existing.absorb(&dependency);
                    }
                }
            }
        }
        Ok(out)
    }

    /// The crates the board's drivers expose to app code, one per name.
    pub fn expose_deps(&self) -> Vec<&espforge_schema::Expose> {
        self.devices
            .iter()
            .filter_map(Device::as_bus)
            .flat_map(|device| device.driver.expose.iter())
            .fold(Vec::new(), |mut out, expose| {
                if !out
                    .iter()
                    .any(|known: &&espforge_schema::Expose| known.package == expose.package)
                {
                    out.push(expose);
                }
                out
            })
    }
}

/// The generated factory type for one deferred device: `oled` -> `OledFactory`.
fn factory_name(id: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in id.chars() {
        if ch == '_' {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out.push_str("Factory");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: &str, at: usize) -> Ident {
        Ident::new(raw, at..at + raw.len())
    }

    fn owner(kind: OwnerKind, raw: &str, at: usize) -> Owner {
        Owner::new(kind, id(raw, at))
    }

    fn meta(name: &str) -> Meta {
        Meta {
            name: name.into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::default(),
            ota: None,
            logger: None,
        }
    }

    fn dependency(package: &str, version: Option<&str>) -> espforge_schema::Dependency {
        espforge_schema::Dependency {
            package: package.into(),
            version: version.map(str::to_string),
            path: None,
            git: None,
            rev: None,
            features: Vec::new(),
            default_features: true,
        }
    }

    #[test]
    fn near_versions_of_one_crate_are_one_source() {
        assert!(
            dependency("embedded-graphics", Some("0.8.1"))
                .same_source(&dependency("embedded-graphics", Some("0.8.2")))
        );
        assert!(
            !dependency("embedded-graphics", Some("0.8.1"))
                .same_source(&dependency("embedded-graphics", Some("0.9.0")))
        );
    }

    #[test]
    fn a_missing_version_is_compatible_with_any_version() {
        assert!(
            dependency("embedded-graphics", None)
                .same_source(&dependency("embedded-graphics", Some("0.8.1")))
        );
    }

    #[test]
    fn a_different_revision_is_a_different_source() {
        let mut left = dependency("esp-hal", Some("1.2.2"));
        let mut right = dependency("esp-hal", Some("1.2.2"));
        left.git = Some("https://example.invalid/hal".into());
        left.rev = Some("aaa".into());
        right.git = left.git.clone();
        right.rev = Some("bbb".into());
        assert!(!left.same_source(&right));
    }

    #[test]
    fn absorbing_keeps_the_stricter_requirement_and_both_feature_sets() {
        let mut left = dependency("embedded-graphics", Some("0.8.1"));
        left.features = vec!["font".into()];
        let mut right = dependency("embedded-graphics", Some("0.8.2"));
        right.features = vec!["font".into(), "fixed".into()];
        right.default_features = false;

        left.absorb(&right);

        assert_eq!(left.version.as_deref(), Some("0.8.2"));
        assert_eq!(left.features, ["font", "fixed"]);
        assert!(!left.default_features);
    }

    /// `<led ...>` at `at` as the diagnostics corpus would see it.
    fn led_claim(
        g: &mut ResourceGraph,
        pin: u8,
        name: &str,
        at: usize,
    ) -> Result<ClaimedPin, Conflict> {
        g.claim(
            Pin(pin),
            owner(OwnerKind::Pin("led".into()), name, at + 1),
            at..at + 4 + name.len(),
            PinUse::Output,
        )
    }

    #[test]
    fn crate_name_is_slugged() {
        assert_eq!(meta("example project").crate_name(), "example_project");
        assert_eq!(meta("Hello-World").crate_name(), "hello_world");
        assert_eq!(meta("3d").crate_name(), "p_3d");
        assert_eq!(meta("!!!").crate_name(), "espforge_project");
    }

    #[test]
    fn console_defaults_to_uart() {
        assert_eq!(Console::default(), Console::Uart);
    }

    #[test]
    fn ident_rust_name_replaces_dashes() {
        assert_eq!(Ident::new("red-led", 0..0).rust_name(), "red_led");
        assert_eq!(Ident::new("status", 0..0).as_str(), "status");
    }

    #[test]
    fn claiming_a_free_pin_records_its_owner_and_span() {
        let mut g = ResourceGraph::new();
        let p = led_claim(&mut g, 18, "red-led", 40).unwrap();

        assert_eq!(p.pin(), Pin(18));
        assert_eq!(p.owner().kind, OwnerKind::Pin("led".into()));
        assert_eq!(p.owner().id.raw, "red-led");
        assert_eq!(p.owner().id.span, 41..48);
        assert_eq!(p.claim().span, 40..51);
        assert_eq!(p.claim().need, PinUse::Output);
        assert_eq!(g.claim_of(Pin(18)).unwrap().owner.id.raw, "red-led");
    }

    #[test]
    fn a_second_claim_names_both_owners_and_keeps_the_first() {
        let mut g = ResourceGraph::new();
        led_claim(&mut g, 18, "red_led", 10).unwrap();
        let conflict = led_claim(&mut g, 18, "other", 50).unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::Pin);
        assert_eq!(conflict.pin, Pin(18));
        assert_eq!(conflict.existing.owner.id.raw, "red_led");
        assert_eq!(conflict.incoming.owner.id.raw, "other");
        assert_eq!(conflict.existing.span, 10..21);
        assert_eq!(conflict.incoming.span, 50..59);
        // The failed claim did not steal the pin.
        assert_eq!(g.claim_of(Pin(18)).unwrap().owner.id.raw, "red_led");
    }

    #[test]
    fn a_runtime_held_peripheral_cannot_be_claimed() {
        let mut g = ResourceGraph::new();
        g.hold("UART0", "the log console", "use another UART");
        let refusal = g
            .claim_peripheral("UART0", owner(OwnerKind::Uart, "gps", 10), 10..30)
            .expect_err("the console has it");
        let PeripheralRefusal::Held(clash) = refusal else {
            panic!("a held resource is not an element clash");
        };
        assert_eq!(clash.peripheral, "UART0");
        assert_eq!(clash.held.holder, "the log console");
        assert_eq!(g.peripheral_claims().count(), 0, "nothing is recorded");
    }

    #[test]
    fn held_resources_are_listed_by_name() {
        let mut g = ResourceGraph::new();
        g.hold("TIMG0.timer0", "the embassy tick", "use another timer");
        g.hold("UART0", "the log console", "use another UART");
        assert_eq!(
            g.held_resources().map(|(name, _)| name).collect::<Vec<_>>(),
            ["TIMG0.timer0", "UART0"]
        );
        assert_eq!(g.held("UART0").unwrap().holder, "the log console");
        assert!(g.held("UART1").is_none());
    }

    #[test]
    fn opposite_directions_are_a_direction_clash() {
        let mut g = ResourceGraph::new();
        led_claim(&mut g, 18, "red_led", 10).unwrap();
        let conflict = g
            .claim(
                Pin(18),
                owner(OwnerKind::Gpio, "input_gpio", 40),
                40..47,
                PinUse::Input,
            )
            .unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::Direction);
        assert_eq!(conflict.existing.owner.kind, OwnerKind::Pin("led".into()));
        assert_eq!(conflict.incoming.owner.kind, OwnerKind::Gpio);
    }

    #[test]
    fn one_element_claiming_the_same_pin_twice_is_a_self_clash() {
        let mut g = ResourceGraph::new();
        let owner = owner(OwnerKind::Gpio, "gpio5", 10);
        g.claim(Pin(5), owner.clone(), 10..35, PinUse::Unspecified)
            .unwrap();
        let conflict = g
            .claim(Pin(5), owner, 10..35, PinUse::Unspecified)
            .unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::SelfClash);
        assert_eq!(conflict.existing.owner.id.raw, "gpio5");
        assert_eq!(conflict.incoming.owner.id.raw, "gpio5");
    }

    /// The two ownership cases `blink` can reach, expressed as claims. The
    /// graph detects them and keeps both sides; the element's wording stays
    /// with the lowering that knows the element.
    #[test]
    fn led_conflicts_are_expressed_as_claims() {
        let mut g = ResourceGraph::new();
        led_claim(&mut g, 18, "red_led", 10).unwrap();
        let conflict = led_claim(&mut g, 18, "other", 50).unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::Pin);
        assert_eq!(
            (
                conflict.existing.owner.kind,
                conflict.existing.owner.id.raw.as_str()
            ),
            (OwnerKind::Pin("led".into()), "red_led")
        );
        assert_eq!(
            (
                conflict.incoming.owner.kind,
                conflict.incoming.owner.id.raw.as_str()
            ),
            (OwnerKind::Pin("led".into()), "other")
        );
    }

    #[test]
    fn pin_claims_are_listed_in_pin_order() {
        let mut g = ResourceGraph::new();
        led_claim(&mut g, 18, "red_led", 10).unwrap();
        led_claim(&mut g, 4, "blue_led", 40).unwrap();

        let pins: Vec<u8> = g.pin_claims().map(|(pin, _)| pin.0).collect();
        assert_eq!(pins, [4, 18]);
    }

    #[test]
    fn a_board_holds_its_resources() {
        let mut resources = ResourceGraph::new();
        led_claim(&mut resources, 18, "red-led", 10).unwrap();
        let board = Board {
            meta: meta("blink"),
            resources,
            devices: Vec::new(),
            services: Vec::new(),
            warnings: Vec::new(),
        };

        assert!(board.devices.is_empty());
        assert_eq!(board.resources.claim_of(Pin(18)).unwrap().span, 10..21);
    }
}
