//! The schema: one loader over the core file and its fragments.
//!
//! `espf.schema.kdl` is the single source of truth for what an ESPF file may
//! contain (sections, elements, attributes, examples) and for the build
//! options it can turn on. The `# services` elements live beside it in
//! `services.schema.kdl`, and the `# devices` elements live beside their
//! drivers in `espforge-drivers/drivers.schema.kdl`; [`load_all`] composes the
//! fragments into one schema. This crate owns the only code in the workspace
//! that walks KDL; everything else reads the typed model it produces.
//!
//! ```kdl
//! section "peripherals" {
//!     summary "The chip's hardware blocks you use"
//!     description "Each one gets an id devices refer to with $id."
//! }
//! element "i2c" section="peripherals" {
//!     summary "I2C bus controller"
//!     attr "sda" kind="pin" required=#true
//!     example "minimal" #"<i2c id="i2c0" sda="6" scl="7" />"#
//!     see_also "devices/ssd1306"
//! }
//! options {
//!     option "alloc" default=#true {
//!         help "esp-alloc comes with no stability guarantees at this time."
//!     }
//! }
//! ```
//!
//! [`load`] reports every problem with the byte span of the offending node or
//! argument; [`schema`] is the embedded copy, parsed once.

use espforge_diagnostics::{DiagCode, codes};
use kdl::{KdlDocument, KdlEntry, KdlNode};
use std::ops::Range;
use std::sync::LazyLock;

mod driver;
mod requirement;
pub mod syntax;

pub use syntax::{MappedValue, RustExpr, RustPath, RustType, ServiceExprTemplate, TypeTemplate};

/// The core schema text, embedded at compile time.
pub const SCHEMA_KDL: &str = include_str!("espf.schema.kdl");

/// The `# services` elements, embedded at compile time.
///
/// A service element carries a `service_recipe` rather than a Rust file, so its
/// schema is the whole of its specification; keeping it in its own file keeps
/// the core vocabulary readable. Parse it with [`SCHEMA_KDL`], which declares
/// the `services` section it names.
pub const SERVICES_SCHEMA_KDL: &str = include_str!("services.schema.kdl");

/// The one schema instance, parsed on first use.
///
/// A schema that fails to load is a bug in this crate, not in a user's file:
/// the workspace's own tests parse it before it can ever be shipped.
pub fn schema() -> &'static Schema {
    static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
        load_all(&[SCHEMA_KDL, SERVICES_SCHEMA_KDL]).unwrap_or_else(|errors| {
            let mut text = String::from("espf.schema.kdl does not load:");
            for error in errors {
                text.push_str(&format!("\n  {}: {}", error.span.start, error.message));
            }
            panic!("{text}")
        })
    });
    &SCHEMA
}

// ---------------------------------------------------------------- data model

/// Names no section or element may have.
pub const RESERVED_NAMES: &[&str] = &["help", "examples", "overview"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub sections: Vec<Section>,
    pub elements: Vec<Element>,
    pub options: Vec<OptionDef>,
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

impl Schema {
    /// An empty schema, for [`load_all`] to fill from several sources.
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
            elements: Vec::new(),
            options: Vec::new(),
        }
    }

    pub fn section_named(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    pub fn element(&self, tag: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.tag == tag)
    }

    /// Every element that lives in `section`, in schema order.
    pub fn elements_in<'a>(&'a self, section: &'a str) -> impl Iterator<Item = &'a Element> {
        self.elements.iter().filter(move |e| e.section == section)
    }

    pub fn get(&self, name: &str) -> Option<&OptionDef> {
        self.options.iter().find(|o| o.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// One line, shown in listings.
    pub summary: String,
    /// Paragraph shown above the section's elements.
    pub description: String,
    /// Span of the `section` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub tag: String,
    pub section: String,
    pub summary: String,
    pub description: Option<String>,
    pub attrs: Vec<AttrDef>,
    pub examples: Vec<Example>,
    /// Targets of `see_also`, as `section/tag`.
    pub see_also: Vec<String>,
    /// The interface this element provides (`provides "network"`), if any.
    ///
    /// A `ref` `to=` may name an interface instead of a tag; the reference is
    /// then resolved by this declaration rather than by the provider's tag.
    pub provides: Option<String>,
    /// The `recipe` child, when this element is a device built from a pin
    /// reference and booleans rather than its own Rust file.
    pub recipe: Option<Recipe>,
    /// The `device_recipe` child, when this element is a driver built from a
    /// bus, auxiliary pins and value slots rather than its own Rust
    /// file.
    pub device_recipe: Option<DeviceRecipe>,
    /// The `service_recipe` child, when this element is a service built from
    /// its network provider, config fields and arguments rather than its own
    /// Rust file.
    pub service_recipe: Option<ServiceRecipe>,
    /// The `crate` block, when this element is an external driver package
    /// rather than one the bundled `espforge-drivers` umbrella implements.
    pub package: Option<PackageRef>,
    /// Crates whose traits the driver's types expose to app code.
    pub expose: Vec<Expose>,
    /// Every crate the manifest names, in declaration order.
    ///
    /// A v2 `driver` fills this from its `dependency` blocks; a v1 `element`
    /// is converted from `crate`/`expose` on parse, so the emitter consumes
    /// one list regardless of the format the author wrote.
    pub dependencies: Vec<Dependency>,
    /// Which manifest format declared this element.
    pub format: Format,
    /// The schema source this element came from: the path of a project
    /// manifest, a `--driver-dir` file, or a bundled file's label.
    pub source: Option<String>,
    /// Span of the `element` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

/// The manifest format that declared an element.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    /// `element "<tag>" section="devices" { device_recipe { .. } }`.
    #[default]
    V1,
    /// `driver "<tag>" { .. }`.
    V2,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::V1 => "v1",
            Format::V2 => "v2",
        }
    }
}

/// One crate a driver manifest names, as the generated `Cargo.toml` emits it.
///
/// Exactly one of `version`, `path` or `git` names the source; `git` carries
/// the pinned `rev`. `default_features` is `false` only when the manifest says
/// `default_features #false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub package: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub git: Option<String>,
    pub rev: Option<String>,
    pub features: Vec<String>,
    pub default_features: bool,
}

impl Dependency {
    /// Which of `version`, `path` or `git` names the source.
    pub fn source_kind(&self) -> &'static str {
        if self.path.is_some() {
            "path"
        } else if self.git.is_some() {
            "git"
        } else {
            "version"
        }
    }

    /// The v1 `crate` block, converted to the one dependency shape.
    pub fn from_package(package: &PackageRef) -> Self {
        Self {
            package: package.package.clone(),
            version: package.version.clone(),
            path: package.path.clone(),
            git: package.git.clone(),
            rev: package.rev.clone(),
            features: package.features.clone(),
            default_features: true,
        }
    }

    /// One v1 `expose` block: a version-only crate.
    pub fn from_expose(expose: &Expose) -> Self {
        Self {
            package: expose.package.clone(),
            version: expose.version.clone(),
            path: None,
            git: None,
            rev: None,
            features: Vec::new(),
            default_features: true,
        }
    }

    /// Whether `other` names the same crate from the same source.
    ///
    /// `path`, `git` and `rev` must match exactly; a missing one is "same as
    /// the other", because a v1 bundled `deps` entry names no version while a
    /// v2 manifest may add one. Versions are Cargo requirements, so `0.8.1`
    /// and `0.8.2` are one caret window and describe one source.
    pub fn same_source(&self, other: &Self) -> bool {
        fn agree<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
            match (left, right) {
                (Some(left), Some(right)) => left == right,
                _ => true,
            }
        }
        agree(&self.path, &other.path)
            && agree(&self.git, &other.git)
            && agree(&self.rev, &other.rev)
            && match (&self.version, &other.version) {
                (Some(left), Some(right)) => requirement::overlap(left, right),
                _ => true,
            }
    }

    /// Fold a compatible record into this one: the stricter requirement, every
    /// feature either side asks for, and default features only while both want
    /// them. Call it only after [`Dependency::same_source`] agrees.
    pub fn absorb(&mut self, other: &Self) {
        if let Some(version) = &other.version {
            self.version = Some(match &self.version {
                Some(existing) => requirement::stricter(existing, version).to_string(),
                None => version.clone(),
            });
        }
        for feature in &other.features {
            if !self.features.contains(feature) {
                self.features.push(feature.clone());
            }
        }
        self.default_features &= other.default_features;
    }
}

/// The `crate` block of an external driver element: which Rust crate
/// implements it, and where that crate comes from.
///
/// Exactly one of `version`, `path` or `git` (with `rev`) names the source, so
/// the generated `Cargo.toml` line is never ambiguous. A bundled element has no
/// `crate` block and keeps using the `espforge-drivers` feature instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRef {
    /// The crate name, as it appears in the generated `Cargo.toml`.
    pub package: String,
    /// A crates.io version.
    pub version: Option<String>,
    /// A path to a directory containing the crate, relative to the manifest.
    pub path: Option<String>,
    /// A git repository URL.
    pub git: Option<String>,
    /// The revision `git` is pinned to.
    pub rev: Option<String>,
    /// Features to switch on for the dependency.
    pub features: Vec<String>,
}

impl PackageRef {
    /// Which of `version`, `path` or `git` names the source.
    pub fn source(&self) -> &'static str {
        if self.path.is_some() {
            "path"
        } else if self.git.is_some() {
            "git"
        } else {
            "version"
        }
    }
}

/// One `expose` block: a crate the driver's public types come from, which the
/// generated project must name so app code can implement its traits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expose {
    pub package: String,
    pub version: Option<String>,
}

/// The `recipe` child of a `# devices` element: how to build it from an id,
/// one pin reference and booleans, with no Rust file of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// The element's `attr` of `kind="ref"` that names the referenced element.
    pub pin: String,
    /// The direction the referenced element must declare, and the claim's use.
    pub direction: Direction,
    /// The Rust type constructed as `new(pin, <bool attrs in schema order>)`.
    pub runtime: String,
    /// The `DiagCode` for "cannot be built".
    pub invalid: DiagCode,
    /// Span of the `recipe` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

/// The `device_recipe` child of a `# devices` element: how to build a driver
/// from one bus, its auxiliary pins and its value slots, with no
/// Rust file of its own.
///
/// The recipe carries data, not code. Each [`DeviceArg`] either maps an ESPF
/// spelling to the Rust expression it emits, or templates a claimed pin, so the
/// generic lowerer never parses a Rust expression to fill a placeholder.
/// The bus modes a `device_recipe` declares it supports.
///
/// One `runtime "blocking"` / `runtime "async"` line per mode. A recipe with
/// no line is blocking, which is every driver today; a driver that gains an
/// async body declares both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Runtimes {
    pub blocking: bool,
    pub asynchronous: bool,
}

impl Runtimes {
    pub const fn blocking() -> Self {
        Self {
            blocking: true,
            asynchronous: false,
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

impl Default for Runtimes {
    fn default() -> Self {
        Self::blocking()
    }
}

/// The `runtime "async"` block: the type, constructor and feature that replace
/// the blocking ones when the device's bus resolves async.
///
/// The type and constructor are a pair: half a block is a load error, and a
/// block is only legal beside `runtime "async"`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AsyncMode {
    /// The field's Rust type on an async bus.
    pub ty: Option<String>,
    /// The Rust path of the async constructor.
    pub new: Option<String>,
    /// The `espforge-drivers` feature the async type needs, when it differs
    /// from the recipe's own (`ssd1306-async`).
    pub feature: Option<String>,
    /// The endpoint type the async constructor takes, for a deferred factory.
    pub endpoint: Option<String>,
}

/// Which level asserts an SPI device's chip select. A recipe fact, like the
/// device's mode: no project attribute writes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CsPolarity {
    /// The common case.
    #[default]
    Low,
    /// A part whose select line is active-high.
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecipe {
    /// The bus kind the device binds to: `i2c`, `spi` or `uart`.
    ///
    /// The element carries a `bus="$<peripheral id>"` attribute; this word is
    /// the peripheral tag that reference must name, so a display on an SPI bus
    /// is refused at lowering instead of failing in `rustc`.
    pub bus: String,
    /// The `espforge-drivers` feature the generated project names.
    pub feature: String,
    /// The kind word the diagnostics use ("display").
    pub kind: String,
    /// The field's Rust type, with `{slot}` placeholders for `arg` values.
    pub ty: String,
    /// The Rust path of the constructor, called with the bus and every `arg`
    /// in declared order.
    pub new: String,
    /// Crates the generated project must name because of this driver
    /// (`embedded-graphics`).
    pub deps: Vec<String>,
    /// The value slots, in constructor argument order.
    pub args: Vec<DeviceArg>,
    /// The bus modes the driver supports (`runtime "blocking"`).
    pub runtimes: Runtimes,
    /// The async half of the recipe (`runtime "async" { .. }`).
    pub async_mode: AsyncMode,
    /// The bus endpoint type the blocking constructor takes. Needed only to
    /// name the field of a deferred device's factory.
    pub endpoint: Option<String>,
    /// Whether the device's `mode`/`frequency` attributes are its own transfer
    /// configuration, applied before each transaction.
    pub spi_config: bool,
    /// The chip-select polarity the part needs.
    pub cs_polarity: CsPolarity,
    /// What the device needs of the I2C address it declares itself.
    ///
    /// `None` for a device on a non-I2C bus; `Any` when the project's `.espf`
    /// chooses the address; `Fixed` when the part answers at one address only.
    pub address: AddressNeed,
    /// The address used when the spec omits `address=` (v2 `bus` default).
    pub address_default: Option<u8>,
    /// The inclusive range the address must fall in (v2 `bus` allowed).
    pub address_allowed: Option<(u8, u8)>,
    /// The companion drivers this one composes (`compose "<tag>" { .. }`).
    ///
    /// A composite is a driver plus its sub-chips: the parent's own device and
    /// one device per part, all on the parent's bus. Empty for every driver
    /// that is one part.
    pub parts: Vec<RecipePart>,
    /// Span of the `device_recipe` node in the schema text.
    pub span: Range<usize>,
}

/// One `compose` block of a [`DeviceRecipe`]: a companion driver on the
/// composite's bus, with its attributes filled from the composite's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipePart {
    /// The tag of the driver being composed.
    pub tag: String,
    /// The part's attributes, each filled from the composite or a literal.
    pub attrs: Vec<PartAttr>,
    /// Span of the `compose` node in the schema text.
    pub span: Range<usize>,
}

/// One `attr "<name>" from=".."` / `value=".."` line of a `compose` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartAttr {
    /// The part's attribute this fills (`cs`, `irq`, `address`).
    pub name: String,
    /// Where the value comes from.
    pub source: PartSource,
    /// Span of the `attr` node in the schema text.
    pub span: Range<usize>,
}

/// Where one composed attribute's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartSource {
    /// `from="touch_cs"`: one of the composite's own attributes.
    From(String),
    /// `value="0x48"`: a literal the spec does not write.
    Value(String),
}

/// One `arg` of a [`DeviceRecipe`]: a slot the constructor takes, either a
/// value mapped from the attribute's spelling or a claimed auxiliary pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceArg {
    /// The element attribute this slot reads.
    pub name: String,
    /// `Some` for a `map` slot: the ESPF spelling to the Rust expression it
    /// emits. Every value the attribute allows has an entry.
    pub map: Option<Vec<(String, String)>>,
    /// `Some` for a `pin` slot: the Rust expression with `{n}` where the pin
    /// number goes.
    pub pin: Option<String>,
    /// The direction the referenced `<gpio>` must declare (pin slots).
    pub direction: PinDirection,
    /// Span of the `arg` node in the schema text.
    pub span: Range<usize>,
}

/// The `service_recipe` child of a `# services` element: how to build a
/// service from one network provider, a config struct and positional
/// arguments, with no Rust file of its own.
///
/// A service is software over an interface, so it owns no pin. The recipe names
/// the attribute holding the network reference, the runtime feature to switch
/// on, the field type and its constructor, and one [`ServiceField`] or `arg`
/// per remaining attribute. Filling those slots is `espforge-elements`'s job,
/// and every rule that could make a slot unfillable is a load error here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRecipe {
    /// The element's `attr` of `kind="ref"` that names the network provider
    /// the service rides on (`network`).
    pub network: String,
    /// The `espforge-runtime` feature the generated project names.
    pub feature: String,
    /// The `Services` field's Rust type.
    pub ty: String,
    /// The Rust path of the constructor, called with the stack, the config and
    /// every `arg` in declared order.
    pub new: String,
    /// The `static` storage type the constructor takes first, if the service
    /// owns mutable state (`espforge_runtime::HttpClientStorage`). The generated
    /// project declares one per service, so two services never share one buffer.
    pub storage: Option<String>,
    /// The config struct passed to `new`, built from the recipe's `field`s.
    /// `None` passes the `arg`s directly.
    pub config: Option<String>,
    /// Heap the service needs in `main.rs`, in bytes (`embedded-tls` scratch,
    /// record buffers).
    pub heap_bytes: Option<u32>,
    /// Whether the service needs the embassy runtime (the default).
    pub requires_embassy: bool,
    /// The config fields, in the order the config struct declares them.
    pub fields: Vec<ServiceField>,
    /// The attribute names passed to `new` after the config, in order.
    pub args: Vec<String>,
    /// Span of the `service_recipe` node in the schema text.
    pub span: Range<usize>,
}

/// One `field` of a [`ServiceRecipe`]: an attribute that becomes a named member
/// of the service's config struct.
///
/// `expr` is the Rust expression the value is written as, with the attribute's
/// own name standing for its literal (`"timeout_ms as u64"`). The checks are
/// data too: `min` is the buffer floor a protocol cannot work below, `range` a
/// numeric window, and `non_empty` a value that cannot be blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceField {
    /// The element attribute this field reads.
    pub name: String,
    /// The Rust expression template, or `None` for the literal alone.
    pub expr: Option<String>,
    /// The smallest value the field accepts, in bytes.
    pub min: Option<u32>,
    /// The inclusive numeric window the field accepts.
    pub range: Option<(u32, u32)>,
    /// Whether an empty string is refused.
    pub non_empty: bool,
    /// `const=#true`: the value parameterizes the recipe's Rust types
    /// (`HttpClientStorage::<2>`) instead of filling a config member.
    pub is_const: bool,
    /// Span of the `field` node in the schema text.
    pub span: Range<usize>,
}

/// What a device needs of the I2C address of the component it binds.
///
/// The address itself always lives on the device: this says only
/// whether the device needs one, and whether the part fixes its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressNeed {
    /// The device does not need an address (an SPI device).
    None,
    /// Any usable address; the project's `.espf` decides (a display).
    Any,
    /// The part's own fixed address; a device declaring another is a
    /// wiring error.
    Fixed(u8),
}

/// The usable 7-bit I2C addresses: the specification reserves `0x00..=0x07`
/// (general call, CBUS) and `0x78..=0x7F` (10-bit prefixes, reserved).
pub const USABLE_I2C_ADDRESSES: std::ops::RangeInclusive<u8> = 0x08..=0x77;

/// Which way a recipe's pin is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

impl Direction {
    /// The ESPF spelling, as written in `direction=".."`.
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Input => "input",
            Direction::Output => "output",
        }
    }
}

/// Which way a v2 manifest's pin line is driven.
///
/// A [`Direction`] is what a pin *device* recipe declares: an LED is an output
/// and a button an input, and the runtime type owns no switching method. A bus
/// device's pin slot can also name a line the driver switches between the two,
/// so it needs one value more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinDirection {
    Input,
    Output,
    /// A line the driver drives and reads (`esp_hal::gpio::Flex`).
    Bidirectional,
}

impl PinDirection {
    /// The ESPF spelling, as written in `direction=".."`.
    pub fn as_str(self) -> &'static str {
        match self {
            PinDirection::Input => "input",
            PinDirection::Output => "output",
            PinDirection::Bidirectional => "bidirectional",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    /// Shown as the title of the example.
    pub title: String,
    /// One or more lines of valid ESPF (may include `# section` headers).
    pub fragment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrDef {
    /// Dotted names allowed: `network1.ssid`.
    pub name: String,
    pub kind: AttrKind,
    pub required: bool,
    pub default: Option<String>,
    /// Inclusive bounds for a `kind="u32"` attribute, from `range="0..=3"`.
    pub range: Option<(u32, u32)>,
    pub help: Option<String>,
    /// Span of the `attr` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttrKind {
    Ident,
    Str,
    Pin,
    Freq,
    Hex,
    U32,
    Bool,
    Choice(Vec<String>),
    /// A `$id` reference to elements of these tags.
    Ref {
        to: Vec<String>,
    },
    /// A GPIO number or a reference to a `<gpio>`.
    PinOrRef,
    /// Repeated with `+=` on every entry, the first one included.
    List(Box<AttrKind>),
}

impl AttrKind {
    /// The short form shown in listings (`pin`, `ref to i2c`, `none | even`).
    pub fn display(&self) -> String {
        match self {
            AttrKind::Ident => "ident".into(),
            AttrKind::Str => "string".into(),
            AttrKind::Pin => "pin".into(),
            AttrKind::Freq => "freq".into(),
            AttrKind::Hex => "hex".into(),
            AttrKind::U32 => "u32".into(),
            AttrKind::Bool => "bool".into(),
            AttrKind::Choice(c) => c.join(" | "),
            AttrKind::Ref { to } => format!("ref to {}", to.join(", ")),
            AttrKind::PinOrRef => "pin or ref".into(),
            AttrKind::List(inner) => format!("list of {}", inner.display()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptValue {
    Bool(bool),
    Str(String),
    List(Vec<String>),
}

impl OptValue {
    /// "Switched on": `true`, a non-empty string, or a non-empty list.
    pub fn is_on(&self) -> bool {
        match self {
            OptValue::Bool(b) => *b,
            OptValue::Str(s) => !s.is_empty(),
            OptValue::List(l) => !l.is_empty(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Bool,
    String,
    Choice(Vec<String>),
    List,
}

/// Which chips an option is available for. `None` = all chips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compat {
    pub chips: Option<Vec<String>>,
}

impl Compat {
    pub fn allows(&self, chip: &str) -> bool {
        self.chips
            .as_ref()
            .is_none_or(|c| c.iter().any(|x| x == chip))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requires {
    /// Other options that must be on when this one is.
    pub options: Vec<String>,
    /// External tools (e.g. `probe-rs`) needed; informational for `doctor`.
    pub tools: Vec<String>,
    pub nightly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionDef {
    pub name: String,
    pub display_name: Option<String>,
    pub help: Option<String>,
    pub kind: Kind,
    /// Value used when `defaults="true"` and nothing was given.
    pub default: Option<OptValue>,
    pub requires: Requires,
    pub compat: Compat,
    pub selection_group: Option<String>,
    /// Computed by the pipeline rather than chosen by the user (`embassy`).
    pub derived: bool,
    /// Span of the `option` node in `espf.schema.kdl`.
    pub span: Range<usize>,
}

/// One problem with the schema text, located in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// Short summary.
    pub message: String,
    /// Explanation shown at the span.
    pub label: String,
    /// Byte span of the offending node or argument.
    pub span: Range<usize>,
}

// -------------------------------------------------------------------- loader

pub(crate) fn node_range(n: &KdlNode) -> Range<usize> {
    let s = n.span();
    s.offset()..s.offset() + s.len()
}

pub(crate) fn entry_range(e: &KdlEntry) -> Range<usize> {
    let s = e.span();
    s.offset()..s.offset() + s.len()
}

pub(crate) fn err(
    span: Range<usize>,
    message: impl Into<String>,
    label: impl Into<String>,
) -> SchemaError {
    SchemaError {
        message: message.into(),
        label: label.into(),
        span,
    }
}

/// Positional (unnamed) string arguments of a node, with their spans.
pub(crate) fn positional_strings(
    node: &KdlNode,
    errors: &mut Vec<SchemaError>,
) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    for e in node.entries().iter().filter(|e| e.name().is_none()) {
        match e.value().as_string() {
            Some(s) => out.push((s.to_string(), entry_range(e))),
            None => errors.push(err(
                entry_range(e),
                format!("`{}` takes strings", node.name().value()),
                "write this argument as a quoted string",
            )),
        }
    }
    out
}

/// Values of a named property like `section=` or `kind=`, with spans.
pub(crate) fn props(
    node: &KdlNode,
    key: &str,
    errors: &mut Vec<SchemaError>,
) -> Vec<(String, Range<usize>)> {
    node.entries()
        .iter()
        .filter(|e| e.name().is_some_and(|n| n.value() == key))
        .map(|e| match e.value().as_string() {
            Some(s) => (s.to_string(), entry_range(e)),
            None => {
                errors.push(err(
                    entry_range(e),
                    format!("`{key}` must be a string"),
                    "quote it",
                ));
                (String::new(), entry_range(e))
            }
        })
        .collect()
}

/// The first value of a named property.
pub(crate) fn prop(
    node: &KdlNode,
    key: &str,
    errors: &mut Vec<SchemaError>,
) -> Option<(String, Range<usize>)> {
    props(node, key, errors).into_iter().next()
}

pub(crate) fn children(node: &KdlNode) -> &[KdlNode] {
    node.children().map(|c| c.nodes()).unwrap_or(&[])
}

/// Parse one schema source, such as `espf.schema.kdl`. All problems are
/// reported, each with the byte span of the offending KDL node or argument.
/// Checks include reserved names, tag uniqueness across sections, every `to=`
/// and `see_also` target, and that every element has a summary and at least one
/// example.
pub fn load(src: &str) -> Result<Schema, Vec<SchemaError>> {
    load_all(&[src])
}

/// Parse several schema sources into one schema.
///
/// The core vocabulary lives in `espforge-schema`; a driver crate co-locates
/// its own `# devices` elements beside its code. Composing is the caller's
/// job, so that every crate owns the file it embeds and no crate reaches into
/// a sibling checkout with `include_str!`. Cross-source checks (tag uniqueness,
/// `to=` and `see_also` targets) run once, after every source is parsed, so a
/// fragment may refer to an element declared in another one.
pub fn load_all(sources: &[&str]) -> Result<Schema, Vec<SchemaError>> {
    let mut schema = Schema::new();
    let mut errors = Vec::new();
    for src in sources {
        parse_into(&mut schema, src, &mut errors);
    }

    check_references(&schema, &mut errors);
    check_compositions(&schema, &mut errors);

    if errors.is_empty() {
        Ok(schema)
    } else {
        errors.sort_by_key(|e| e.span.start);
        Err(errors)
    }
}

/// Parse several **named** sources into one schema, recording where each
/// element came from and refusing a tag declared twice.
///
/// This is the runtime-composition entry point: the caller supplies the
/// bundled files first, then project manifests in precedence order. The label
/// is the path a user can open, so a duplicate names both files instead of
/// silently shadowing one. Unlike [`load_all`], a later source may only add
/// elements: sections it does not redeclare are borrowed from the sources
/// before it, which is what a one-element driver manifest needs.
pub fn load_composed(sources: &[(String, String)]) -> Result<Schema, Vec<SchemaError>> {
    let mut schema = Schema::new();
    let mut errors = Vec::new();
    for (label, text) in sources {
        // A fragment may name a section declared by an earlier source, so the
        // temp schema starts with what is already there. Its elements start
        // empty: duplicates are `extend`'s business, with both labels.
        let mut one = Schema::new();
        one.sections = schema.sections.clone();
        parse_into(&mut one, text, &mut errors);
        if let Err(duplicates) = schema.extend(one, label) {
            errors.extend(duplicates);
        }
    }

    check_references(&schema, &mut errors);
    check_compositions(&schema, &mut errors);

    if errors.is_empty() {
        Ok(schema)
    } else {
        errors.sort_by_key(|e| e.span.start);
        Err(errors)
    }
}

impl Schema {
    /// Add `other`'s sections, options and elements to this schema.
    ///
    /// A section already present is kept (a driver fragment reuses the core's
    /// `devices` section); an option or element tag already present is a
    /// conflict, refused with both sources named. `source` labels the elements
    /// this call adds, so a later duplicate can say where the first one came
    /// from.
    pub fn extend(&mut self, other: Schema, source: &str) -> Result<(), Vec<SchemaError>> {
        let mut errors = Vec::new();
        for section in other.sections {
            if self.section_named(&section.name).is_none() {
                self.sections.push(section);
            }
        }
        for option in other.options {
            if self.get(&option.name).is_some() {
                errors.push(err(
                    option.span.clone(),
                    format!("duplicate option `{}`", option.name),
                    format!("`{source}` declares it too; each option is defined once"),
                ));
            } else {
                self.options.push(option);
            }
        }
        for mut element in other.elements {
            if let Some(existing) = self.element(&element.tag) {
                let first = existing.source.as_deref().unwrap_or("an earlier source");
                errors.push(err(
                    element.span.clone(),
                    format!("duplicate tag `{}`: `{source}` and `{first}`", element.tag),
                    format!(
                        "`<{}>` is already declared by `{first}`; a driver tag has one source",
                        element.tag
                    ),
                ));
            } else {
                element.source = Some(source.to_string());
                self.elements.push(element);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Parse one source into `schema`, reporting every problem in it.
fn parse_into(schema: &mut Schema, src: &str, errors: &mut Vec<SchemaError>) {
    let doc = match KdlDocument::parse_v2(src) {
        Ok(d) => d,
        Err(e) => {
            let syntax: Vec<SchemaError> = e
                .diagnostics
                .iter()
                .map(|d| {
                    let s = d.span;
                    err(
                        s.offset()..s.offset() + s.len(),
                        "invalid KDL",
                        d.message.as_deref().unwrap_or("syntax error"),
                    )
                })
                .collect();
            if syntax.is_empty() {
                errors.push(err(0..0, "invalid KDL", "the file is not valid KDL v2"));
            } else {
                errors.extend(syntax);
            }
            return;
        }
    };

    for node in doc.nodes() {
        match node.name().value() {
            "section" => parse_section(node, schema, errors),
            "element" => parse_element(node, schema, errors),
            "driver" => driver::parse_driver(node, schema, errors),
            "required" | "options" => parse_options(node, schema, errors),
            other => errors.push(err(
                node_range(node),
                format!("unknown top-level node `{other}`"),
                "expected `section`, `element` or `options`",
            )),
        }
    }
}

fn parse_section(node: &KdlNode, schema: &mut Schema, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "section has no name",
            "write `section \"name\" ...`",
        ));
        return;
    }
    let (name, name_span) = names.remove(0);
    if RESERVED_NAMES.contains(&name.as_str()) {
        errors.push(err(
            name_span,
            format!("`{name}` is a reserved name"),
            "a section cannot be called help, examples or overview",
        ));
        return;
    }
    if schema.section_named(&name).is_some() {
        errors.push(err(
            name_span,
            format!("duplicate section `{name}`"),
            "each section may be defined once",
        ));
        return;
    }

    let mut summary = None;
    let mut description = None;
    for child in children(node) {
        match child.name().value() {
            "summary" | "description" => {
                let slot = if child.name().value() == "summary" {
                    &mut summary
                } else {
                    &mut description
                };
                let value = positional_strings(child, errors).into_iter().next();
                if slot.is_some() {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` given twice", child.name().value()),
                        "keep one",
                    ));
                } else if let Some((value, _)) = value {
                    *slot = Some(value);
                } else {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` takes a string", child.name().value()),
                        "quote it",
                    ));
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in section `{name}`"),
                "expected `summary` or `description`",
            )),
        }
    }
    for (taken, what) in [(&summary, "summary"), (&description, "description")] {
        if taken.is_none() {
            errors.push(err(
                span.clone(),
                format!("section `{name}` has no {what}"),
                "write it as a quoted string inside the section",
            ));
        }
    }

    schema.sections.push(Section {
        name,
        summary: summary.unwrap_or_default(),
        description: description.unwrap_or_default(),
        span,
    });
}

/// `[a-z][a-z0-9_]*`: an interface name an element can provide.
fn is_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn parse_element(node: &KdlNode, schema: &mut Schema, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "element has no tag",
            "write `element \"tag\" ...`",
        ));
        return;
    }
    let (tag, tag_span) = names.remove(0);
    if RESERVED_NAMES.contains(&tag.as_str()) {
        errors.push(err(
            tag_span,
            format!("`{tag}` is a reserved name"),
            "an element cannot be called help, examples or overview",
        ));
        return;
    }
    if schema.element(&tag).is_some() {
        errors.push(err(
            tag_span,
            format!("duplicate tag `{tag}`"),
            "tags are unique across sections",
        ));
        return;
    }
    let Some((section_name, section_span)) = prop(node, "section", errors) else {
        errors.push(err(
            span,
            format!("element `{tag}` has no section"),
            "add `section=\"name\"` naming a section",
        ));
        return;
    };
    if schema.section_named(&section_name).is_none() {
        errors.push(err(
            section_span,
            format!("element `{tag}` names unknown section `{section_name}`"),
            "define that `section` node first, or fix the name",
        ));
        return;
    }

    let mut summary = None;
    let mut description = None;
    let mut attrs = Vec::new();
    let mut examples = Vec::new();
    let mut see_also = Vec::new();
    let mut provides = None;
    let mut recipe_node: Option<&KdlNode> = None;
    let mut device_recipe_node: Option<&KdlNode> = None;
    let mut service_recipe_node: Option<&KdlNode> = None;
    let mut package_node: Option<&KdlNode> = None;
    let mut expose_nodes: Vec<&KdlNode> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "summary" | "description" => {
                let slot = if child.name().value() == "summary" {
                    &mut summary
                } else {
                    &mut description
                };
                let value = positional_strings(child, errors).into_iter().next();
                if slot.is_some() {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` given twice", child.name().value()),
                        "keep one",
                    ));
                } else if let Some((value, _)) = value {
                    *slot = Some(value);
                } else {
                    errors.push(err(
                        node_range(child),
                        format!("`{}` takes a string", child.name().value()),
                        "quote it",
                    ));
                }
            }
            "attr" => parse_attr(child, &mut attrs, errors),
            "recipe" => {
                if recipe_node.is_some() {
                    errors.push(err(node_range(child), "`recipe` given twice", "keep one"));
                } else {
                    recipe_node = Some(child);
                }
            }
            "device_recipe" => {
                if device_recipe_node.is_some() {
                    errors.push(err(
                        node_range(child),
                        "`device_recipe` given twice",
                        "keep one",
                    ));
                } else {
                    device_recipe_node = Some(child);
                }
            }
            "service_recipe" => {
                if service_recipe_node.is_some() {
                    errors.push(err(
                        node_range(child),
                        "`service_recipe` given twice",
                        "keep one",
                    ));
                } else {
                    service_recipe_node = Some(child);
                }
            }
            "crate" => {
                if package_node.is_some() {
                    errors.push(err(node_range(child), "`crate` given twice", "keep one"));
                } else {
                    package_node = Some(child);
                }
            }
            "expose" => expose_nodes.push(child),
            "provides" => {
                let value = positional_strings(child, errors).into_iter().next();
                if provides.is_some() {
                    errors.push(err(node_range(child), "`provides` given twice", "keep one"));
                } else if let Some((value, _)) = value {
                    if is_interface_name(&value) {
                        provides = Some(value);
                    } else {
                        errors.push(err(
                            node_range(child),
                            format!("`{value}` is not an interface name"),
                            "write `provides \"network\"`: lowercase letters, digits and `_`",
                        ));
                    }
                } else {
                    errors.push(err(
                        node_range(child),
                        "`provides` takes a string",
                        "write `provides \"network\"`",
                    ));
                }
            }
            "example" => parse_example(child, &mut examples, errors),
            "see_also" => {
                for (target, target_span) in positional_strings(child, errors) {
                    if target.split_once('/').is_none() {
                        errors.push(err(
                            target_span,
                            format!("`{target}` is not a `section/leaf` target"),
                            "write for example `devices/ssd1306`",
                        ));
                        continue;
                    }
                    see_also.push(target);
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in element `{tag}`"),
                "expected summary, description, attr, provides, recipe, device_recipe, \
                 service_recipe, crate, expose, example or see_also",
            )),
        }
    }

    // Validated after the walk so the recipes can be checked against every
    // `attr` the element declares, wherever the recipe line sits.
    let recipe =
        recipe_node.and_then(|node| parse_recipe(node, &tag, &section_name, &attrs, errors));
    let device_recipe = device_recipe_node
        .and_then(|node| parse_device_recipe(node, &tag, &section_name, &attrs, errors));
    let service_recipe = service_recipe_node
        .and_then(|node| parse_service_recipe(node, &tag, &section_name, &attrs, errors));
    let package = package_node.and_then(|node| parse_package(node, &tag, &section_name, errors));
    let expose: Vec<Expose> = expose_nodes
        .iter()
        .filter_map(|node| parse_expose(node, errors))
        .collect();
    let mut dependencies: Vec<Dependency> = Vec::new();
    if let Some(package) = &package {
        dependencies.push(Dependency::from_package(package));
    }
    dependencies.extend(expose.iter().map(Dependency::from_expose));
    if let Some(recipe) = &device_recipe {
        for dep in &recipe.deps {
            if !dependencies.iter().any(|known| &known.package == dep) {
                dependencies.push(Dependency {
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
    if (package.is_some() || !expose.is_empty()) && device_recipe.is_none() {
        errors.push(err(
            span.clone(),
            format!("`{tag}` names a crate but has no `device_recipe`"),
            "an external driver is a `device_recipe`; a hand-written element lives in a crate",
        ));
    }

    if summary.is_none() {
        errors.push(err(
            span.clone(),
            format!("element `{tag}` has no summary"),
            "write `summary \"...\"`",
        ));
    }
    if examples.is_empty() {
        errors.push(err(
            span.clone(),
            format!("element `{tag}` has no example"),
            "every element carries at least one `example \"title\" \"fragment\"`",
        ));
    }

    schema.elements.push(Element {
        tag,
        section: section_name,
        summary: summary.unwrap_or_default(),
        description,
        attrs,
        examples,
        see_also,
        provides,
        recipe,
        device_recipe,
        service_recipe,
        package,
        expose,
        dependencies,
        format: Format::V1,
        source: None,
        span,
    });
}

/// Parse the `crate` block of an external driver element.
///
/// The block is data the generated `Cargo.toml` copies: a package name and
/// exactly one source (`version`, `path`, or `git` + `rev`), plus features.
/// Every rule that could produce an unbuildable dependency line is a load
/// error here, with the offending node's span.
fn parse_package(
    node: &KdlNode,
    tag: &str,
    section: &str,
    errors: &mut Vec<SchemaError>,
) -> Option<PackageRef> {
    let span = node_range(node);
    if section != "devices" {
        errors.push(err(
            span,
            "`crate` is for devices",
            "only a `# devices` element can be an external driver package",
        ));
        return None;
    }
    let Some((package, _)) = child_string(node, "package", errors) else {
        errors.push(err(
            span.clone(),
            format!("`crate` in `{tag}` has no `package`"),
            "name the crate, e.g. `package \"espforge-driver-adxl345\"`",
        ));
        return None;
    };
    if package.is_empty()
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        errors.push(err(
            span.clone(),
            format!("`{package}` is not a crate name"),
            "crate names use letters, digits, `-` and `_`",
        ));
        return None;
    }
    let version = child_string(node, "version", errors).map(|(value, _)| value);
    let path = child_string(node, "path", errors).map(|(value, _)| value);
    let git = child_string(node, "git", errors).map(|(value, _)| value);
    let rev = child_string(node, "rev", errors).map(|(value, _)| value);

    let chosen = [version.is_some(), path.is_some(), git.is_some()]
        .into_iter()
        .filter(|chosen| *chosen)
        .count();
    if chosen != 1 {
        errors.push(err(
            span.clone(),
            format!("`crate` in `{tag}` names {chosen} sources"),
            "write exactly one of `version`, `path` or `git`",
        ));
        return None;
    }
    if git.is_some() && rev.is_none() {
        errors.push(err(
            span.clone(),
            format!("`crate` in `{tag}` names a `git` without a `rev`"),
            "pin the revision, e.g. `rev \"<commit>\"`",
        ));
    }
    if rev.is_some() && git.is_none() {
        errors.push(err(
            span.clone(),
            format!("`crate` in `{tag}` names a `rev` without a `git`"),
            "a revision pins a repository; add `git \"<url>\"` or drop `rev`",
        ));
    }
    if path.as_ref().is_some_and(|path| path.is_empty()) {
        errors.push(err(
            span.clone(),
            "`path` is empty",
            "name the crate's directory",
        ));
    }

    let mut features: Vec<String> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "package" | "version" | "path" | "git" | "rev" => {}
            "features" => features.extend(
                positional_strings(child, errors)
                    .into_iter()
                    .map(|(feature, _)| feature),
            ),
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `crate` of `{tag}`"),
                "expected package, version, path, git, rev or features",
            )),
        }
    }

    Some(PackageRef {
        package,
        version,
        path,
        git,
        rev,
        features,
    })
}

/// Parse one `expose` block: a crate the driver's public API comes from.
fn parse_expose(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<Expose> {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "`expose` has no crate",
            "write `expose \"embedded-graphics\" { version \"0.8\" }`",
        ));
        return None;
    }
    let (package, _) = names.remove(0);
    let version = child_string(node, "version", errors).map(|(value, _)| value);
    for child in children(node) {
        if child.name().value() != "version" {
            errors.push(err(
                node_range(child),
                format!("unknown node `{}` in `expose`", child.name().value()),
                "expected `version`",
            ));
        }
    }
    Some(Expose { package, version })
}

/// Parse and validate the `recipe` child of an element.
///
/// Every rule is checked here, with the offending node's span, so a bad recipe
/// is a load error and never a runtime surprise.
fn parse_recipe(
    node: &KdlNode,
    tag: &str,
    section: &str,
    attrs: &[AttrDef],
    errors: &mut Vec<SchemaError>,
) -> Option<Recipe> {
    let span = node_range(node);

    // A recipe is for a device: a peripheral has no pin reference.
    if section != "devices" {
        errors.push(err(
            span,
            "`recipe` is for devices",
            "a peripheral with no pin reference needs an `Element`",
        ));
        return None;
    }

    let Some((pin, _)) = prop(node, "pin", errors) else {
        errors.push(err(
            span.clone(),
            "`recipe` has no `pin`",
            "name the element's `ref` attribute, e.g. `pin=\"gpio\"`",
        ));
        return None;
    };

    // `pin` must name one of this element's `ref` attributes.
    if !attrs
        .iter()
        .any(|attr| attr.name == pin && matches!(attr.kind, AttrKind::Ref { .. }))
    {
        let refs: Vec<String> = attrs
            .iter()
            .filter(|attr| matches!(attr.kind, AttrKind::Ref { .. }))
            .map(|attr| format!("`{}`", attr.name))
            .collect();
        errors.push(err(
            span.clone(),
            format!("`<{tag}>` has no ref attribute `{pin}`"),
            format!(
                "recipe pin must name one of: {}",
                if refs.is_empty() {
                    "(none)".to_string()
                } else {
                    refs.join(", ")
                }
            ),
        ));
        return None;
    }

    let Some((direction_name, direction_span)) = prop(node, "direction", errors) else {
        errors.push(err(
            span.clone(),
            "`recipe` has no `direction`",
            "write `direction=\"input\"` or `direction=\"output\"`",
        ));
        return None;
    };
    let direction = match direction_name.as_str() {
        "input" => Direction::Input,
        "output" => Direction::Output,
        _ => {
            errors.push(err(
                direction_span,
                "`direction` must be `input` or `output`",
                format!("`{direction_name}` is neither"),
            ));
            return None;
        }
    };

    let Some((runtime, runtime_span)) = prop(node, "runtime", errors) else {
        errors.push(err(
            span.clone(),
            "`recipe` has no `runtime`",
            "name the Rust type, e.g. `runtime=\"espforge_runtime::Led\"`",
        ));
        return None;
    };
    if !is_rust_path(&runtime) {
        errors.push(err(
            runtime_span,
            "`runtime` must be a Rust path such as `espforge_runtime::Led`",
            "segments are identifiers separated by `::`",
        ));
        return None;
    }

    let invalid = match prop(node, "invalid", errors) {
        Some((code, code_span)) => match codes::ALL.iter().find(|known| known.as_str() == code) {
            Some(known) => *known,
            None => {
                errors.push(err(
                    code_span,
                    format!("`{code}` is not a diagnostic code this workspace defines"),
                    "see espforge-diagnostics/src/codes.rs",
                ));
                return None;
            }
        },
        None => codes::COMPONENT_INVALID,
    };

    // A recipe device is an id, one pin reference (or a literal `pin=`
    // number) and booleans; anything else needs a hand-written `Element`.
    for attr in attrs {
        if attr.name == pin || matches!(attr.kind, AttrKind::Pin) {
            continue;
        }
        if !matches!(attr.kind, AttrKind::Ident | AttrKind::Bool) {
            errors.push(err(
                attr.span.clone(),
                format!("`{tag}.{}` is `{}`", attr.name, attr.kind.display()),
                "a recipe device is an id, one pin reference and booleans; write an `Element`",
            ));
        }
    }

    Some(Recipe {
        pin,
        direction,
        runtime,
        invalid,
        span,
    })
}

/// Whether `text` is `ident(::ident)*`: the shape of the runtime path a recipe
/// splices into generated Rust.
pub(crate) fn is_rust_path(text: &str) -> bool {
    !text.is_empty()
        && text.split("::").all(|segment| {
            !segment.is_empty()
                && !segment.starts_with(|c: char| c.is_ascii_digit())
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// The `{slot}` names in a Rust type expression, in order.
fn slots(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once('{') {
        let Some((name, after)) = tail.split_once('}') else {
            break;
        };
        out.push(name.trim().to_string());
        rest = after;
    }
    out
}

/// Parse and validate the `device_recipe` child of a `# devices` element.
///
/// The recipe is data: the bus attribute, one slot per remaining attribute,
/// the driver feature and the Rust type and constructor. Filling those slots
/// is `espforge-elements`'s job, and every rule that could make a slot
/// unfillable is a load error here.
fn parse_device_recipe(
    node: &KdlNode,
    tag: &str,
    section: &str,
    attrs: &[AttrDef],
    errors: &mut Vec<SchemaError>,
) -> Option<DeviceRecipe> {
    let span = node_range(node);

    if section != "devices" {
        errors.push(err(
            span,
            "`device_recipe` is for devices",
            "only an element in `# devices` has a bus to attach to",
        ));
        return None;
    }

    if !attrs
        .iter()
        .any(|attr| attr.name == "id" && attr.kind == AttrKind::Ident)
    {
        errors.push(err(
            span.clone(),
            format!("`device_recipe` element `{tag}` has no `id`"),
            "write `attr \"id\" kind=\"ident\" required=#true`",
        ));
    }

    for name in [
        "bus",
        "feature",
        "kind",
        "type",
        "new",
        "taken",
        "endpoint",
        "spi_config",
        "cs_polarity",
    ] {
        if children(node)
            .iter()
            .filter(|child| child.name().value() == name)
            .count()
            > 1
        {
            errors.push(err(
                node_range(node),
                format!("`{name}` given twice"),
                "keep one",
            ));
        }
    }

    let Some((bus, bus_span)) = child_string(node, "bus", errors) else {
        errors.push(err(
            span.clone(),
            "`device_recipe` has no `bus`",
            "name the bus kind, e.g. `bus \"i2c\"`",
        ));
        return None;
    };
    if !matches!(bus.as_str(), "i2c" | "spi" | "uart") {
        errors.push(err(
            bus_span,
            format!("`{bus}` is not a bus kind"),
            "write `bus \"i2c\"`, `bus \"spi\"` or `bus \"uart\"`",
        ));
        return None;
    }
    // The element must expose that peripheral as a `bus` reference, so the
    // recipe and the attributes cannot disagree about what it binds to.
    match attrs.iter().find(|attr| attr.name == "bus") {
        Some(AttrDef {
            kind: AttrKind::Ref { to },
            ..
        }) if to.iter().any(|target| target == &bus) => {}
        _ => {
            errors.push(err(
                span.clone(),
                format!("`<{tag}>` has no `bus` reference to `{bus}`"),
                format!("write `attr \"bus\" kind=\"ref\" to=\"{bus}\" required=#true`"),
            ));
            return None;
        }
    }

    let Some((feature, feature_span)) = child_string(node, "feature", errors) else {
        errors.push(err(
            span.clone(),
            "`device_recipe` has no `feature`",
            "name the `espforge-drivers` feature, e.g. `feature=\"ssd1306\"`",
        ));
        return None;
    };
    if feature.is_empty() {
        errors.push(err(
            feature_span,
            "`feature` is empty",
            "name the `espforge-drivers` feature",
        ));
        return None;
    }

    let kind = child_string(node, "kind", errors)
        .map(|(kind, _)| kind)
        .unwrap_or_else(|| "device".to_string());

    let Some((ty, ty_span)) = child_string(node, "type", errors) else {
        errors.push(err(
            span.clone(),
            "`device_recipe` has no `type`",
            "name the generated field's Rust type, e.g. `type=\"espforge_drivers::Scd4x\"`",
        ));
        return None;
    };

    let Some((new, new_span)) = child_string(node, "new", errors) else {
        errors.push(err(
            span.clone(),
            "`device_recipe` has no `new`",
            "name the constructor, e.g. `new=\"espforge_drivers::Scd4x::new\"`",
        ));
        return None;
    };
    if !is_rust_path(&new) {
        errors.push(err(
            new_span,
            "`new` must be a Rust path such as `espforge_drivers::Scd4x::new`",
            "segments are identifiers separated by `::`",
        ));
        return None;
    }

    let address = parse_needs_address(node, errors);
    let (runtimes, async_mode) = parse_runtimes(node, errors);
    let endpoint = parse_path_child(node, "endpoint", errors);
    let spi_config = children(node)
        .iter()
        .any(|child| child.name().value() == "spi_config");
    let cs_polarity = parse_cs_polarity(node, errors);

    // A per-device transfer configuration only makes sense on SPI, and the
    // element must declare the two attributes the lowerer consumes.
    if spi_config {
        if bus != "spi" {
            errors.push(err(
                span.clone(),
                format!("`{tag}` declares `spi_config` but binds to `{bus}`"),
                "a per-device configuration belongs on an `spi` device",
            ));
        }
        for name in ["mode", "frequency"] {
            if !attrs.iter().any(|attr| attr.name == name) {
                errors.push(err(
                    span.clone(),
                    format!("`{tag}` declares `spi_config` without a `{name}` attribute"),
                    format!("write `attr \"{name}\" ...`, or drop `spi_config`"),
                ));
            }
        }
    }

    // A polarity is a fact about the part's select line, so it is only legal
    // where there is one.
    if cs_polarity != CsPolarity::Low && bus != "spi" {
        errors.push(err(
            span.clone(),
            format!("`{tag}` declares `cs_polarity` but binds to `{bus}`"),
            "chip-select polarity belongs on an `spi` device",
        ));
    }

    // A recipe that needs an address is only valid on an I2C device, and the
    // element must declare the attribute the project fills in.
    if !matches!(address, AddressNeed::None) {
        if bus != "i2c" {
            errors.push(err(
                span.clone(),
                format!("`{tag}` needs an I2C address but binds to `{bus}`"),
                "a `needs_address` recipe belongs on an `i2c` device",
            ));
        }
        if attrs
            .iter()
            .find(|attr| attr.name == "address")
            .map(|attr| &attr.kind)
            != Some(&AttrKind::Hex)
        {
            errors.push(err(
                span.clone(),
                format!("`<{tag}>` needs an `address` attribute"),
                "write `attr \"address\" kind=\"hex\" required=#true`",
            ));
        }
    }

    let mut deps: Vec<String> = Vec::new();
    for child in children(node) {
        if child.name().value() == "deps" {
            deps.extend(
                positional_strings(child, errors)
                    .into_iter()
                    .map(|(dep, _)| dep),
            );
        }
    }

    let mut args: Vec<DeviceArg> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            // The scalars were collected above, by name.
            "bus" | "feature" | "kind" | "type" | "new" | "deps" | "needs_address" | "runtime"
            | "endpoint" | "spi_config" | "cs_polarity" => {}
            "arg" => parse_device_arg(child, tag, attrs, &mut args, errors),
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `device_recipe` of `{tag}`"),
                "expected bus, feature, kind, type, new, deps, needs_address, runtime, \
                 endpoint, spi_config, cs_polarity or arg",
            )),
        }
    }

    // A slot per attribute, and no attribute without a slot: a device recipe
    // that reads an attribute nowhere is a mistake, not a fixed value. `id`,
    // `bus` and the wiring attributes (`address`, `cs`) are consumed by the
    // lowerer itself, not by a constructor slot.
    for attr in attrs {
        if matches!(attr.name.as_str(), "id" | "bus" | "address" | "cs" | "init") {
            continue;
        }
        if spi_config && matches!(attr.name.as_str(), "mode" | "frequency") {
            continue;
        }
        if !args.iter().any(|arg| arg.name == attr.name) {
            errors.push(err(
                attr.span.clone(),
                format!("`{tag}.{}` has no `arg` in `device_recipe`", attr.name),
                "a device recipe names every attribute except `id` and its `bus`",
            ));
        }
    }

    // Every type placeholder names a slot the recipe fills.
    for slot in slots(&ty)
        .into_iter()
        .chain(async_mode.ty.as_deref().map(slots).unwrap_or_default())
    {
        if !args.iter().any(|arg| arg.name == slot) {
            errors.push(err(
                ty_span.clone(),
                format!("`type` uses `{{{slot}}}`, which is not an `arg`"),
                "add that `arg`, or drop the placeholder",
            ));
        }
    }

    Some(DeviceRecipe {
        bus,
        feature,
        kind,
        ty,
        new,
        deps,
        args,
        address,
        address_default: None,
        address_allowed: None,
        parts: Vec::new(),
        runtimes,
        async_mode,
        endpoint,
        spi_config,
        cs_polarity,
        span,
    })
}

/// Parse the `runtime` children of a `device_recipe`.
///
/// `runtime "blocking"` and `runtime "async"` may appear together. No line
/// means blocking (the shape every driver had before the attribute existed);
/// an unknown or repeated value is a load error, so a typo cannot quietly
/// narrow what a driver supports.
fn parse_runtimes(node: &KdlNode, errors: &mut Vec<SchemaError>) -> (Runtimes, AsyncMode) {
    let mut runtimes = Runtimes {
        blocking: false,
        asynchronous: false,
    };
    let mut mode = AsyncMode::default();
    let mut seen = false;
    for child in children(node) {
        if child.name().value() != "runtime" {
            continue;
        }
        seen = true;
        let Some(value) = child.get(0).and_then(|value| value.as_string()) else {
            errors.push(err(
                node_range(child),
                "`runtime` needs a value",
                "write `runtime \"blocking\"` or `runtime \"async\"`",
            ));
            continue;
        };
        match value {
            "blocking" => {
                if runtimes.blocking {
                    errors.push(err(
                        node_range(child),
                        "`runtime \"blocking\"` given twice",
                        "keep one line per mode",
                    ));
                } else {
                    runtimes.blocking = true;
                }
                if !children(child).is_empty() {
                    errors.push(err(
                        node_range(child),
                        "`runtime \"blocking\"` takes no block",
                        "the per-mode `type`/`new` block belongs on `runtime \"async\"`",
                    ));
                }
            }
            "async" => {
                if runtimes.asynchronous {
                    errors.push(err(
                        node_range(child),
                        "`runtime \"async\"` given twice",
                        "keep one line per mode",
                    ));
                } else {
                    runtimes.asynchronous = true;
                }
                for entry in children(child) {
                    let name = entry.name().value();
                    if !matches!(name, "type" | "new" | "feature" | "endpoint") {
                        errors.push(err(
                            node_range(entry),
                            format!("unknown node `{name}` in `runtime \"async\"`"),
                            "expected type, new, feature or endpoint",
                        ));
                        continue;
                    }
                    let Some((text, span)) = child_string(child, name, errors) else {
                        continue;
                    };
                    if matches!(name, "new" | "endpoint") && !is_rust_path(&text) {
                        errors.push(err(
                            span,
                            format!("`{name}` must be a Rust path"),
                            "segments are identifiers separated by `::`",
                        ));
                        continue;
                    }
                    if name == "feature" && text.is_empty() {
                        errors.push(err(span, "`feature` is empty", "name the async feature"));
                        continue;
                    }
                    let slot = match name {
                        "type" => &mut mode.ty,
                        "new" => &mut mode.new,
                        "feature" => &mut mode.feature,
                        _ => &mut mode.endpoint,
                    };
                    if slot.is_some() {
                        errors.push(err(
                            span,
                            format!("`{name}` given twice in `runtime \"async\"`"),
                            "keep one",
                        ));
                    } else {
                        *slot = Some(text);
                    }
                }
                if mode.ty.is_none() || mode.new.is_none() {
                    errors.push(err(
                        node_range(child),
                        "`runtime \"async\"` needs `type` and `new`",
                        "name both the async field type and its constructor",
                    ));
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("`{other}` is not a bus mode"),
                "write `runtime \"blocking\"` or `runtime \"async\"`",
            )),
        }
    }
    if seen {
        (runtimes, mode)
    } else {
        (Runtimes::blocking(), mode)
    }
}

/// The first positional string of the optional child `name`, validated as a
/// Rust path when present.
fn parse_path_child(node: &KdlNode, name: &str, errors: &mut Vec<SchemaError>) -> Option<String> {
    let (text, span) = child_string(node, name, errors)?;
    if !is_rust_path(&text) {
        errors.push(err(
            span,
            format!("`{name}` must be a Rust path"),
            "segments are identifiers separated by `::`",
        ));
        return None;
    }
    Some(text)
}

/// The optional `cs_polarity` child: `"low"` (the default) or `"high"`.
fn parse_cs_polarity(node: &KdlNode, errors: &mut Vec<SchemaError>) -> CsPolarity {
    let Some((text, span)) = child_string(node, "cs_polarity", errors) else {
        return CsPolarity::Low;
    };
    match text.as_str() {
        "low" => CsPolarity::Low,
        "high" => CsPolarity::High,
        other => {
            errors.push(err(
                span,
                format!("`{other}` is not a chip-select polarity"),
                "write `cs_polarity \"low\"` or `cs_polarity \"high\"`",
            ));
            CsPolarity::Low
        }
    }
}

/// Parse the `needs_address` child of a `device_recipe`, which is optional.
///
/// Bare (`needs_address`) means the project's `.espf` picks any usable
/// address; with a value (`needs_address "0x62"`) the part fixes it. A value
/// outside [`USABLE_I2C_ADDRESSES`] is a load error, with its own span.
fn parse_needs_address(node: &KdlNode, errors: &mut Vec<SchemaError>) -> AddressNeed {
    let mut seen: Option<&KdlNode> = None;
    for child in children(node) {
        if child.name().value() != "needs_address" {
            continue;
        }
        if seen.is_some() {
            errors.push(err(
                node_range(child),
                "`needs_address` given twice",
                "keep one",
            ));
            continue;
        }
        seen = Some(child);
    }
    let Some(child) = seen else {
        return AddressNeed::None;
    };

    let values = positional_strings(child, errors);
    match values.as_slice() {
        [] => AddressNeed::Any,
        [(text, at)] => match hex_byte(text) {
            Some(address) if USABLE_I2C_ADDRESSES.contains(&address) => AddressNeed::Fixed(address),
            _ => {
                errors.push(err(
                    at.clone(),
                    format!("`{text}` is not a usable I2C address"),
                    "write a 7-bit address in 0x08..=0x77, for example `0x62`",
                ));
                AddressNeed::None
            }
        },
        _ => {
            errors.push(err(
                node_range(child),
                "`needs_address` takes one address",
                "write `needs_address` or `needs_address \"0x62\"`",
            ));
            AddressNeed::None
        }
    }
}

/// A `0x..` or decimal byte.
pub(crate) fn hex_byte(text: &str) -> Option<u8> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// The first positional string of the child node `name`, with its span.
///
/// A missing child is `None`; a child with no string is a schema error.
pub(crate) fn child_string(
    node: &KdlNode,
    name: &str,
    errors: &mut Vec<SchemaError>,
) -> Option<(String, Range<usize>)> {
    let child = children(node)
        .iter()
        .find(|child| child.name().value() == name)?;
    match positional_strings(child, errors).into_iter().next() {
        Some(value) => Some(value),
        None => {
            errors.push(err(
                node_range(child),
                format!("`{name}` takes a string"),
                "quote it",
            ));
            None
        }
    }
}

/// Parse and validate one `arg` slot of a [`DeviceRecipe`].
fn parse_device_arg(
    node: &KdlNode,
    tag: &str,
    attrs: &[AttrDef],
    args: &mut Vec<DeviceArg>,
    errors: &mut Vec<SchemaError>,
) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "`arg` has no name", "write `arg \"attr\" ...`"));
        return;
    }
    let (name, name_span) = names.remove(0);
    if args.iter().any(|arg| arg.name == name) {
        errors.push(err(
            name_span,
            format!("duplicate `arg` `{name}`"),
            "each argument slot is declared once",
        ));
        return;
    }
    let Some(attr) = attrs.iter().find(|attr| attr.name == name) else {
        errors.push(err(
            name_span,
            format!("`<{tag}>` has no attribute `{name}`"),
            "an `arg` reads one of the element's own attributes",
        ));
        return;
    };

    let pin = prop(node, "pin", errors).map(|(template, _)| template);
    let mut map: Vec<(String, String)> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "map" => {
                let strings = positional_strings(child, errors);
                if strings.len() != 2 {
                    errors.push(err(
                        node_range(child),
                        "`map` takes a spelling and an expression",
                        "write `map \"128x64\" \"espforge_drivers::Oled128x64\"`",
                    ));
                    continue;
                }
                map.push((strings[0].0.clone(), strings[1].0.clone()));
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `arg` `{name}`"),
                "expected `map`",
            )),
        }
    }

    let direction = match prop(node, "direction", errors) {
        Some((direction, direction_span)) => match direction.as_str() {
            "input" => PinDirection::Input,
            "output" => PinDirection::Output,
            "bidirectional" => PinDirection::Bidirectional,
            other => {
                errors.push(err(
                    direction_span,
                    "`direction` must be `input`, `output` or `bidirectional`",
                    format!("`{other}` is none of them"),
                ));
                PinDirection::Output
            }
        },
        None => PinDirection::Output,
    };

    match (&pin, map.is_empty()) {
        (Some(_), false) => {
            errors.push(err(
                span,
                format!("`arg \"{name}\"` has both `pin=` and a `map`"),
                "a slot is either a claimed pin or a mapped value",
            ));
            return;
        }
        (None, true) => {
            errors.push(err(
                span,
                format!("`arg \"{name}\"` has neither `pin=` nor a `map`"),
                "write `pin=\"..{n}..\"` or `map \"spelling\" \"expression\"`",
            ));
            return;
        }
        _ => {}
    }

    if let Some(template) = &pin {
        if !matches!(attr.kind, AttrKind::Ref { .. }) {
            errors.push(err(
                name_span,
                format!("`{tag}.{name}` is `{}`", attr.kind.display()),
                "a `pin` slot reads a `ref` attribute, usually to `gpio`",
            ));
        }
        if !template.contains("{n}") {
            errors.push(err(
                span.clone(),
                format!("`arg \"{name}\"` has no `{{n}}` in its `pin=`"),
                "the claimed pin number is substituted for `{n}`",
            ));
        }
    } else {
        // Every value the attribute allows is mapped, and nothing else is:
        // that is what keeps a `choice` from reaching emission unmapped.
        let spellings: Vec<String> = match &attr.kind {
            AttrKind::Choice(choices) => choices.clone(),
            AttrKind::Bool => vec!["true".to_string(), "false".to_string()],
            other => {
                errors.push(err(
                    name_span,
                    format!("`{tag}.{name}` is `{}`", other.display()),
                    "a `map` slot reads a `choice` or `bool` attribute",
                ));
                Vec::new()
            }
        };
        for spelling in &spellings {
            if !map.iter().any(|(mapped, _)| mapped == spelling) {
                errors.push(err(
                    span.clone(),
                    format!("`arg \"{name}\"` has no mapping for `{spelling}`"),
                    "every value the attribute allows needs a `map`",
                ));
            }
        }
        for (mapped, _) in &map {
            if !spellings.is_empty() && !spellings.contains(mapped) {
                errors.push(err(
                    span.clone(),
                    format!("`{mapped}` is not a value of `{tag}.{name}`"),
                    format!("choices: {}", spellings.join(", ")),
                ));
            }
        }
    }

    args.push(DeviceArg {
        name,
        map: (!map.is_empty()).then_some(map),
        pin,
        direction,
        span,
    });
}

/// Parse and validate the `service_recipe` child of a `# services` element.
///
/// A service is software over a network provider, so the recipe names that
/// provider's attribute, the runtime feature, the field type and constructor,
/// and one `field` or `arg` per remaining attribute. Every rule is checked
/// here, with the offending node's span, so a bad recipe is a load error.
fn parse_service_recipe(
    node: &KdlNode,
    tag: &str,
    section: &str,
    attrs: &[AttrDef],
    errors: &mut Vec<SchemaError>,
) -> Option<ServiceRecipe> {
    let span = node_range(node);

    if section != "services" {
        errors.push(err(
            span,
            "`service_recipe` is for services",
            "only an element in `# services` rides on a network provider",
        ));
        return None;
    }

    if !attrs
        .iter()
        .any(|attr| attr.name == "id" && attr.kind == AttrKind::Ident)
    {
        errors.push(err(
            span.clone(),
            format!("`service_recipe` element `{tag}` has no `id`"),
            "write `attr \"id\" kind=\"ident\" required=#true`",
        ));
    }

    for name in [
        "network",
        "feature",
        "type",
        "new",
        "storage",
        "config",
        "heap_bytes",
        "requires_embassy",
    ] {
        if children(node)
            .iter()
            .filter(|child| child.name().value() == name)
            .count()
            > 1
        {
            errors.push(err(
                node_range(node),
                format!("`{name}` given twice"),
                "keep one",
            ));
        }
    }

    let Some((network, network_span)) = child_string(node, "network", errors) else {
        errors.push(err(
            span.clone(),
            "`service_recipe` has no `network`",
            "name the element's `ref` attribute, e.g. `network \"network\"`",
        ));
        return None;
    };
    if !attrs
        .iter()
        .any(|attr| attr.name == network && matches!(attr.kind, AttrKind::Ref { .. }))
    {
        errors.push(err(
            network_span,
            format!("`<{tag}>` has no ref attribute `{network}`"),
            "`network` must name the element's `$` reference attribute",
        ));
        return None;
    }

    let Some((feature, feature_span)) = child_string(node, "feature", errors) else {
        errors.push(err(
            span.clone(),
            "`service_recipe` has no `feature`",
            "name the `espforge-runtime` feature, e.g. `feature \"ntp\"`",
        ));
        return None;
    };
    if feature.is_empty() {
        errors.push(err(
            feature_span,
            "`feature` is empty",
            "name the `espforge-runtime` feature",
        ));
        return None;
    }

    let Some((ty, ty_span)) = child_string(node, "type", errors) else {
        errors.push(err(
            span.clone(),
            "`service_recipe` has no `type`",
            "name the field's Rust type, e.g. `type \"espforge_runtime::NtpClient\"`",
        ));
        return None;
    };
    if !is_rust_path(&ty) {
        errors.push(err(
            ty_span,
            "`type` must be a Rust path such as `espforge_runtime::NtpClient`",
            "segments are identifiers separated by `::`",
        ));
        return None;
    }

    let Some((new, new_span)) = child_string(node, "new", errors) else {
        errors.push(err(
            span.clone(),
            "`service_recipe` has no `new`",
            "name the constructor, e.g. `new \"espforge_runtime::NtpClient::new\"`",
        ));
        return None;
    };
    if !is_rust_path(&new) {
        errors.push(err(
            new_span,
            "`new` must be a Rust path such as `espforge_runtime::NtpClient::new`",
            "segments are identifiers separated by `::`",
        ));
        return None;
    }

    let storage = match child_string(node, "storage", errors) {
        Some((storage, storage_span)) => {
            if !is_rust_path(&storage) {
                errors.push(err(
                    storage_span,
                    "`storage` must be a Rust path such as `espforge_runtime::HttpClientStorage`",
                    "segments are identifiers separated by `::`",
                ));
                return None;
            }
            Some(storage)
        }
        None => None,
    };

    let config = match child_string(node, "config", errors) {
        Some((config, config_span)) => {
            if !is_rust_path(&config) {
                errors.push(err(
                    config_span,
                    "`config` must be a Rust path such as `espforge_runtime::NtpConfig`",
                    "segments are identifiers separated by `::`",
                ));
                return None;
            }
            Some(config)
        }
        None => None,
    };

    let heap_bytes = match children(node)
        .iter()
        .find(|child| child.name().value() == "heap_bytes")
    {
        Some(child) => match positional_u32(child, errors) {
            Some(0) => {
                errors.push(err(
                    node_range(child),
                    "`heap_bytes` must be positive",
                    "give the number of bytes the service needs",
                ));
                None
            }
            bytes => bytes,
        },
        None => None,
    };

    let requires_embassy = child_flag(node, "requires_embassy", true, errors);

    let mut fields: Vec<ServiceField> = Vec::new();
    let mut args: Vec<String> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            // The scalars were collected above, by name.
            "network" | "feature" | "type" | "new" | "storage" | "config" | "heap_bytes"
            | "requires_embassy" => {}
            "field" => parse_service_field(child, tag, attrs, &mut fields, errors),
            "arg" => {
                let mut names = positional_strings(child, errors);
                if names.is_empty() {
                    errors.push(err(
                        node_range(child),
                        "`arg` has no name",
                        "write `arg \"attr\"`",
                    ));
                    continue;
                }
                let (name, name_span) = names.remove(0);
                if args.contains(&name) {
                    errors.push(err(
                        name_span,
                        format!("duplicate `arg` `{name}`"),
                        "each argument is declared once",
                    ));
                } else if !attrs.iter().any(|attr| attr.name == name) {
                    errors.push(err(
                        name_span,
                        format!("`<{tag}>` has no attribute `{name}`"),
                        "an `arg` reads one of the element's own attributes",
                    ));
                } else {
                    args.push(name);
                }
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in `service_recipe` of `{tag}`"),
                "expected network, feature, type, new, storage, config, heap_bytes, \
                 requires_embassy, field or arg",
            )),
        }
    }

    // One `const` field per recipe: it names the `::<value>` the recipe's
    // types take, so a second one would have nowhere to go.
    let consts: Vec<&ServiceField> = fields.iter().filter(|field| field.is_const).collect();
    for field in consts.iter().skip(1) {
        errors.push(err(
            field.span.clone(),
            "more than one `const` field",
            "a recipe parameterizes its Rust types with one value",
        ));
    }

    // A slot per attribute, and no attribute without a slot: a service recipe
    // that reads an attribute nowhere is a mistake, not a fixed value.
    for attr in attrs {
        if attr.name == "id" || attr.name == network {
            continue;
        }
        if !fields.iter().any(|field| field.name == attr.name) && !args.contains(&attr.name) {
            errors.push(err(
                attr.span.clone(),
                format!(
                    "`{tag}.{}` has no `field` or `arg` in `service_recipe`",
                    attr.name
                ),
                "a service recipe names every attribute except `id` and its `network`",
            ));
        }
    }

    Some(ServiceRecipe {
        network,
        feature,
        ty,
        new,
        storage,
        config,
        heap_bytes,
        requires_embassy,
        fields,
        args,
        span,
    })
}

/// Parse and validate one `field` of a [`ServiceRecipe`].
fn parse_service_field(
    node: &KdlNode,
    tag: &str,
    attrs: &[AttrDef],
    fields: &mut Vec<ServiceField>,
    errors: &mut Vec<SchemaError>,
) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "`field` has no name", "write `field \"attr\"`"));
        return;
    }
    let (name, name_span) = names.remove(0);
    if fields.iter().any(|field| field.name == name) {
        errors.push(err(
            name_span,
            format!("duplicate `field` `{name}`"),
            "each config field is declared once",
        ));
        return;
    }
    if !attrs.iter().any(|attr| attr.name == name) {
        errors.push(err(
            name_span,
            format!("`<{tag}>` has no attribute `{name}`"),
            "a `field` reads one of the element's own attributes",
        ));
        return;
    }

    let expr = prop(node, "as", errors).map(|(expr, _)| expr);
    let min = prop_u32(node, "min", errors);
    let range = match prop(node, "range", errors) {
        Some((text, range_span)) => match parse_range(&text) {
            Some((low, high)) if low <= high => Some((low, high)),
            _ => {
                errors.push(err(
                    range_span,
                    format!("range `{text}` is not `low..=high`"),
                    "write two unsigned integers separated by `..=`, as in `range=\"1..=65535\"`",
                ));
                None
            }
        },
        None => None,
    };
    let non_empty = prop_flag(node, "non_empty");
    let is_const = prop_flag(node, "const");

    if is_const {
        let declared = attrs
            .iter()
            .find(|attr| attr.name == name)
            .expect("the attribute was found above");
        if expr.is_some() {
            errors.push(err(
                span.clone(),
                format!("`const` field `{name}` has an `as=`"),
                "a `const` value is not written into the config, so it has no expression",
            ));
        }
        if declared.kind != AttrKind::U32 {
            errors.push(err(
                span.clone(),
                format!(
                    "`const` needs a `u32` attribute, but `{tag}.{name}` is {}",
                    declared.kind.display()
                ),
                "declare the attribute `kind=\"u32\"`",
            ));
        }
        if declared.default.is_none() {
            errors.push(err(
                span.clone(),
                format!("`const` field `{name}` needs a `default`"),
                "a project that omits the attribute still needs a value",
            ));
        }
    }

    for child in children(node) {
        errors.push(err(
            node_range(child),
            format!(
                "unknown node `{}` in `field` `{name}`",
                child.name().value()
            ),
            "a `field` is a name plus `as=`, `min=`, `range=`, `non_empty=` and \
             `const=` properties",
        ));
    }

    fields.push(ServiceField {
        name,
        expr,
        min,
        range,
        non_empty,
        is_const,
        span,
    });
}

/// A numeric property like `min=4096`, with its span.
fn prop_u32(node: &KdlNode, key: &str, errors: &mut Vec<SchemaError>) -> Option<u32> {
    let entry = node
        .entries()
        .iter()
        .find(|entry| entry.name().is_some_and(|name| name.value() == key))?;
    match entry.value().as_integer() {
        Some(value) if (0..=u32::MAX as i128).contains(&value) => Some(value as u32),
        Some(value) => {
            errors.push(err(
                entry_range(entry),
                format!("`{key}` must be an unsigned 32-bit number"),
                format!("`{value}` is out of range"),
            ));
            None
        }
        None => match entry
            .value()
            .as_string()
            .and_then(|text| text.parse::<u32>().ok())
        {
            Some(value) => Some(value),
            None => {
                errors.push(err(
                    entry_range(entry),
                    format!("`{key}` must be an unsigned 32-bit number"),
                    "write it as an integer, as in `min=4096`",
                ));
                None
            }
        },
    }
}

/// A boolean property like `non_empty=#true`; absent means false.
fn prop_flag(node: &KdlNode, key: &str) -> bool {
    node.entries()
        .iter()
        .filter(|entry| entry.name().is_some_and(|name| name.value() == key))
        .any(|entry| entry.value().as_bool() != Some(false))
}

/// The first positional argument of `node` as an unsigned 32-bit number.
fn positional_u32(node: &KdlNode, errors: &mut Vec<SchemaError>) -> Option<u32> {
    let Some(entry) = node.entries().iter().find(|entry| entry.name().is_none()) else {
        errors.push(err(
            node_range(node),
            format!("`{}` takes a number", node.name().value()),
            "write it as an unsigned integer, as in `heap_bytes 98304`",
        ));
        return None;
    };
    match entry.value().as_integer() {
        Some(value) if (0..=u32::MAX as i128).contains(&value) => Some(value as u32),
        Some(value) => {
            errors.push(err(
                entry_range(entry),
                format!(
                    "`{}` must be an unsigned 32-bit number",
                    node.name().value()
                ),
                format!("`{value}` is out of range"),
            ));
            None
        }
        None => match entry
            .value()
            .as_string()
            .and_then(|text| text.parse::<u32>().ok())
        {
            Some(value) => Some(value),
            None => {
                errors.push(err(
                    entry_range(entry),
                    format!("`{}` takes a number", node.name().value()),
                    "write it as an unsigned integer, as in `heap_bytes 98304`",
                ));
                None
            }
        },
    }
}

/// A bare or `#true`/`#false` child flag, defaulting to `default` when absent.
pub(crate) fn child_flag(
    node: &KdlNode,
    name: &str,
    default: bool,
    errors: &mut Vec<SchemaError>,
) -> bool {
    let Some(child) = children(node)
        .iter()
        .find(|child| child.name().value() == name)
    else {
        return default;
    };
    let Some(entry) = child.entries().iter().find(|entry| entry.name().is_none()) else {
        return true;
    };
    match entry.value().as_bool() {
        Some(value) => value,
        None => match entry.value().as_string() {
            Some("true") => true,
            Some("false") => false,
            _ => {
                errors.push(err(
                    entry_range(entry),
                    format!("`{name}` takes #true or #false"),
                    "KDL v2 booleans are #true / #false",
                ));
                default
            }
        },
    }
}

pub(crate) fn parse_example(
    node: &KdlNode,
    examples: &mut Vec<Example>,
    errors: &mut Vec<SchemaError>,
) {
    let strings = positional_strings(node, errors);
    if strings.len() != 2 {
        if strings.len() == 1 {
            errors.push(err(
                node_range(node),
                "`example` takes title and fragment",
                "write `example \"title\" #\"<tag ... />\"#`",
            ));
        }
        return;
    }
    let (title, title_span) = &strings[0];
    if examples.iter().any(|ex| &ex.title == title) {
        errors.push(err(
            title_span.clone(),
            format!("duplicate example title `{title}`"),
            "give each example a different title",
        ));
        return;
    }
    examples.push(Example {
        title: title.clone(),
        fragment: strings[1].0.clone(),
    });
}

pub(crate) fn parse_attr(node: &KdlNode, attrs: &mut Vec<AttrDef>, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(span, "attr has no name", "write `attr \"name\" ...`"));
        return;
    }
    let (name, name_span) = names.remove(0);
    if attrs.iter().any(|a| a.name == name) {
        errors.push(err(
            name_span,
            format!("duplicate attribute `{name}`"),
            "each attribute may be declared once per element",
        ));
        return;
    }
    let Some((kind_name, kind_span)) = prop(node, "kind", errors) else {
        errors.push(err(span, "attr has no kind", "add `kind=\"..\"`"));
        return;
    };

    let mut required = false;
    for e in node
        .entries()
        .iter()
        .filter(|e| e.name().is_some_and(|n| n.value() == "required"))
    {
        match e.value().as_bool() {
            Some(b) => required = b,
            None => errors.push(err(
                entry_range(e),
                "`required` must be #true or #false",
                "KDL v2 booleans are #true / #false",
            )),
        }
    }

    let default = props(node, "default", errors)
        .into_iter()
        .next()
        .map(|(d, _)| d);
    let range = match props(node, "range", errors).into_iter().next() {
        Some((text, span)) => match parse_range(&text) {
            Some((low, high)) if low <= high => Some((low, high)),
            Some((low, high)) => {
                errors.push(err(
                    span,
                    format!("range `{low}..={high}` is reversed"),
                    "write the lower bound first, as in `range=\"0..=3\"`",
                ));
                None
            }
            None => {
                errors.push(err(
                    span,
                    format!("range `{text}` is not `low..=high`"),
                    "write two unsigned integers separated by `..=`, as in `range=\"0..=3\"`",
                ));
                None
            }
        },
        None => None,
    };
    let mut choices = Vec::new();
    let mut help = None;
    for child in children(node) {
        match child.name().value() {
            "choices" => {
                for (choice, _) in positional_strings(child, errors) {
                    choices.push(choice);
                }
            }
            "help" => match positional_strings(child, errors).into_iter().next() {
                Some((h, _)) => help = Some(h),
                None => errors.push(err(node_range(child), "`help` takes a string", "quote it")),
            },
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in attribute `{name}`"),
                "expected `choices` or `help`",
            )),
        }
    }

    let to: Vec<String> = props(node, "to", errors)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let of = props(node, "of", errors).into_iter().next();
    let kind = build_kind(&name, &kind_name, kind_span, choices, to, of, errors);

    attrs.push(AttrDef {
        name,
        kind,
        required,
        default,
        range,
        help,
        span,
    });
}

/// Parse a `range="0..=3"` property into its inclusive bounds.
pub(crate) fn parse_range(text: &str) -> Option<(u32, u32)> {
    let (low, high) = text.split_once("..=")?;
    Some((low.trim().parse().ok()?, high.trim().parse().ok()?))
}

/// Turn the declared pieces into an `AttrKind`, erroring on shapes that do
/// not fit (choice without choices, ref without `to=`, list without `of=`).
fn build_kind(
    name: &str,
    kind_name: &str,
    kind_span: Range<usize>,
    choices: Vec<String>,
    to: Vec<String>,
    of: Option<(String, Range<usize>)>,
    errors: &mut Vec<SchemaError>,
) -> AttrKind {
    let kind: Option<AttrKind> = match kind_name {
        "ident" => Some(AttrKind::Ident),
        "str" => Some(AttrKind::Str),
        "pin" => Some(AttrKind::Pin),
        "freq" => Some(AttrKind::Freq),
        "hex" => Some(AttrKind::Hex),
        "u32" => Some(AttrKind::U32),
        "bool" => Some(AttrKind::Bool),
        "pin_or_ref" => Some(AttrKind::PinOrRef),
        "choice" => {
            if choices.is_empty() {
                errors.push(err(
                    kind_span.clone(),
                    format!("choice attribute `{name}` has no choices"),
                    "add `choices \"a\" \"b\"`",
                ));
            }
            Some(AttrKind::Choice(choices))
        }
        "ref" => {
            if to.is_empty() {
                errors.push(err(
                    kind_span.clone(),
                    format!("ref attribute `{name}` has no `to=`"),
                    "write `kind=\"ref\" to=\"gpio\"`",
                ));
            }
            Some(AttrKind::Ref { to })
        }
        "list" => match of {
            Some((of_name, of_span)) => {
                let inner = build_kind(
                    name,
                    &of_name,
                    of_span,
                    Vec::new(),
                    Vec::new(),
                    None,
                    errors,
                );
                Some(AttrKind::List(Box::new(inner)))
            }
            None => {
                errors.push(err(
                    kind_span.clone(),
                    format!("list attribute `{name}` has no `of=`"),
                    "write `kind=\"list\" of=\"str\"`",
                ));
                None
            }
        },
        other => {
            errors.push(err(
                kind_span.clone(),
                format!("unknown kind \"{other}\""),
                "expected ident, str, pin, freq, hex, u32, bool, choice, ref, pin_or_ref or list",
            ));
            None
        }
    };
    kind.unwrap_or(AttrKind::Str)
}

fn parse_options(node: &KdlNode, schema: &mut Schema, errors: &mut Vec<SchemaError>) {
    match node.name().value() {
        // Kept as an accepted top-level node so the option half and the
        // element half share one file; this slice has no `required` answers.
        "required" => {}
        "options" => {
            for child in children(node) {
                if child.name().value() != "option" {
                    errors.push(err(
                        node_range(child),
                        format!("unknown node `{}` in `options`", child.name().value()),
                        "expected `option`",
                    ));
                    continue;
                }
                parse_option(child, schema, errors);
            }
        }
        _ => {}
    }
}

fn parse_option(node: &KdlNode, schema: &mut Schema, errors: &mut Vec<SchemaError>) {
    let span = node_range(node);
    let mut names = positional_strings(node, errors);
    if names.is_empty() {
        errors.push(err(
            span,
            "option has no name",
            "write `option \"name\" ...`",
        ));
        return;
    }
    let (name, name_span) = names.remove(0);
    if schema.get(&name).is_some() {
        errors.push(err(
            name_span,
            format!("duplicate option `{name}`"),
            "each option may be defined once",
        ));
        return;
    }

    let mut def = OptionDef {
        name: name.clone(),
        display_name: None,
        help: None,
        kind: Kind::Bool,
        default: None,
        requires: Requires::default(),
        compat: Compat::default(),
        selection_group: None,
        derived: false,
        span,
    };
    let mut kind_name = "bool".to_string();
    let mut kind_span = def.span.clone();
    let mut default_entry: Option<&KdlEntry> = None;

    for e in node.entries().iter().filter(|e| e.name().is_some()) {
        let key = e.name().map(|k| k.value()).unwrap_or("");
        let v = e.value();
        match key {
            "display_name" | "selection_group" | "kind" => match v.as_string() {
                Some(s) => match key {
                    "display_name" => def.display_name = Some(s.to_string()),
                    "selection_group" => def.selection_group = Some(s.to_string()),
                    _ => {
                        kind_name = s.to_string();
                        kind_span = entry_range(e);
                    }
                },
                None => errors.push(err(
                    entry_range(e),
                    format!("`{key}` must be a string"),
                    "quote it",
                )),
            },
            "requires_nightly" | "derived" => match v.as_bool() {
                Some(b) if key == "derived" => def.derived = b,
                Some(b) => def.requires.nightly = b,
                None => errors.push(err(
                    entry_range(e),
                    format!("`{key}` must be #true or #false"),
                    "KDL v2 booleans are #true / #false",
                )),
            },
            "default" => default_entry = Some(e),
            other => errors.push(err(
                entry_range(e),
                format!("unknown property `{other}` on option `{name}`"),
                "expected display_name, selection_group, requires_nightly, derived, kind or default",
            )),
        }
    }

    let mut choices: Vec<String> = Vec::new();
    for child in children(node) {
        match child.name().value() {
            "help" => {
                if let Some((h, _)) = positional_strings(child, errors).into_iter().next() {
                    def.help = Some(h);
                }
            }
            "requires" => {
                for (target, _) in positional_strings(child, errors) {
                    def.requires.options.push(target);
                }
            }
            "requires_tools" => {
                for (tool, _) in positional_strings(child, errors) {
                    def.requires.tools.push(tool);
                }
            }
            "choices" => {
                for (choice, _) in positional_strings(child, errors) {
                    choices.push(choice);
                }
            }
            "compatible" => {
                let mut chips = Vec::new();
                for c in children(child) {
                    if c.name().value() == "chip" {
                        for (chip, _) in positional_strings(c, errors) {
                            chips.push(chip);
                        }
                    } else {
                        errors.push(err(
                            node_range(c),
                            format!("unknown node `{}` in `compatible`", c.name().value()),
                            "expected `chip \"name\" ...`",
                        ));
                    }
                }
                def.compat.chips = Some(chips);
            }
            other => errors.push(err(
                node_range(child),
                format!("unknown node `{other}` in option `{name}`"),
                "expected help, requires, requires_tools, compatible or choices",
            )),
        }
    }

    def.kind = match kind_name.as_str() {
        "bool" => Kind::Bool,
        "string" => Kind::String,
        "list" => Kind::List,
        "choice" => {
            if choices.is_empty() {
                errors.push(err(
                    kind_span.clone(),
                    format!("choice option `{name}` has no choices"),
                    "add `choices \"a\" \"b\"` inside the option",
                ));
            }
            Kind::Choice(choices)
        }
        other => {
            errors.push(err(
                kind_span,
                format!("unknown kind \"{other}\""),
                "expected bool, string, choice or list",
            ));
            Kind::Bool
        }
    };

    def.default = match (&def.kind, default_entry) {
        (Kind::Bool, None) => Some(OptValue::Bool(false)),
        (Kind::Bool, Some(e)) => match e.value().as_bool() {
            Some(b) => Some(OptValue::Bool(b)),
            None => {
                errors.push(err(
                    entry_range(e),
                    "bool default must be #true or #false",
                    "KDL v2 booleans are #true / #false",
                ));
                None
            }
        },
        (Kind::String, Some(e)) => match e.value().as_string() {
            Some(s) => Some(OptValue::Str(s.to_string())),
            None => {
                errors.push(err(
                    entry_range(e),
                    "string default must be a string",
                    "quote it",
                ));
                None
            }
        },
        (Kind::Choice(cs), Some(e)) => match e.value().as_string() {
            Some(s) if cs.iter().any(|c| c == s) => Some(OptValue::Str(s.to_string())),
            _ => {
                errors.push(err(
                    entry_range(e),
                    "default is not one of the choices",
                    format!("choices: {}", cs.join(", ")),
                ));
                None
            }
        },
        (Kind::List, Some(e)) => {
            errors.push(err(
                entry_range(e),
                "list options cannot have a default",
                "remove `default=`",
            ));
            None
        }
        (_, None) => None,
    };

    schema.options.push(def);
}

/// Check `to=` and `see_also` targets after every node is known.
fn check_references(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for element in &schema.elements {
        for attr in &element.attrs {
            if let AttrKind::Ref { to } = &attr.kind {
                for target in to {
                    let known = schema.element(target).is_some()
                        || schema
                            .elements
                            .iter()
                            .any(|element| element.provides.as_deref() == Some(target.as_str()));
                    if !known {
                        errors.push(err(
                            attr.span.clone(),
                            format!(
                                "`{}` refers to unknown element or interface `{target}`",
                                attr.name
                            ),
                            "`to=` must name an element in this file, or an interface one provides",
                        ));
                    }
                }
            }
        }
        for target in &element.see_also {
            let known = target.split_once('/').is_some_and(|(section, tag)| {
                schema.section_named(section).is_some() && schema.element(tag).is_some()
            });
            if !known {
                errors.push(err(
                    element.span.clone(),
                    format!("`see_also \"{target}\"` names no element"),
                    "write `section/tag` for an element defined in this file",
                ));
            }
        }
    }
}

/// Check `compose` blocks after every node is known.
///
/// A part is a driver on the composite's bus, mapped attribute by attribute.
/// The expansion is one level deep, so a part's pin mapping always names a
/// name the spec element writes.
fn check_compositions(schema: &Schema, errors: &mut Vec<SchemaError>) {
    for element in &schema.elements {
        let Some(recipe) = &element.device_recipe else {
            continue;
        };
        for part in &recipe.parts {
            let Some(part_element) = schema.element(&part.tag) else {
                errors.push(err(
                    part.span.clone(),
                    format!("`compose` names unknown driver `{}`", part.tag),
                    "`compose` names a `driver` declared in this schema or a project manifest",
                ));
                continue;
            };
            let Some(part_recipe) = &part_element.device_recipe else {
                errors.push(err(
                    part.span.clone(),
                    format!("`<{}>` is not a driver", part.tag),
                    "`compose` builds a device, so the tag must declare a device recipe",
                ));
                continue;
            };
            if part_recipe.bus != recipe.bus {
                errors.push(err(
                    part.span.clone(),
                    format!(
                        "`<{}>` drives an <{}> bus, but `{}` composes it on <{}>",
                        part.tag, part_recipe.bus, element.tag, recipe.bus
                    ),
                    "a part rides the composite's bus; a part on another bus is a separate element",
                ));
                continue;
            }
            if let Some(inner) = part_recipe.parts.first() {
                errors.push(err(
                    part.span.clone(),
                    format!(
                        "`<{}>` composes `<{}>` too, so `{}` would nest two levels",
                        part.tag, inner.tag, element.tag
                    ),
                    "a composed driver must not compose: the pin mapping would name a name the \
                     spec does not write",
                ));
                continue;
            }

            for attr in &part.attrs {
                if !part_element
                    .attrs
                    .iter()
                    .any(|known| known.name == attr.name)
                {
                    let known: Vec<&str> = part_element
                        .attrs
                        .iter()
                        .map(|known| known.name.as_str())
                        .collect();
                    errors.push(err(
                        attr.span.clone(),
                        format!("`<{}>` has no attribute `{}`", part.tag, attr.name),
                        format!("the part declares: {}", known.join(", ")),
                    ));
                    continue;
                }
                if let PartSource::From(name) = &attr.source
                    && !element.attrs.iter().any(|known| known.name == *name)
                {
                    errors.push(err(
                        attr.span.clone(),
                        format!("`{name}` is not an attribute of `{}`", element.tag),
                        "`from=` names one of the composite's own `attr`s or `pin`s",
                    ));
                }
            }

            for attr in &part_element.attrs {
                if !attr.required || matches!(attr.name.as_str(), "id" | "bus") {
                    continue;
                }
                if !part.attrs.iter().any(|mapped| mapped.name == attr.name) {
                    errors.push(err(
                        part.span.clone(),
                        format!("`compose \"{}\"` does not fill `{}`", part.tag, attr.name),
                        format!(
                            "`<{}>` requires `{}`, and the part declares no default for it",
                            part.tag, attr.name
                        ),
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
section "devices" {
    summary "How a peripheral is used"
    description "A device binds a peripheral to a purpose."
}
section "peripherals" {
    summary "The chip's hardware blocks"
    description "Each one gets an id."
}
element "gpio" section="peripherals" {
    summary "A single GPIO pin"
    attr "id" kind="ident" required=#true
    attr "pin" kind="pin" required=#true
    example "minimal" #"<gpio id="gpio4" pin="4" />"#
}
element "led" section="devices" {
    summary "An LED on a GPIO output"
    attr "id" kind="ident" required=#true
    attr "gpio" kind="ref" to="gpio" required=#true
    example "minimal" #"<led id="led" gpio="$gpio4" />"#
    see_also "peripherals/gpio"
}
options {
    option "alloc" default=#true { help "heap" }
    option "embassy" derived=#true { help "async" }
    option "mode" kind="choice" default="fast" { choices "fast" "slow" }
}
"##;

    #[test]
    fn loads_sections_elements_and_options() {
        let schema = load(SAMPLE).unwrap();
        assert_eq!(schema.sections.len(), 2);
        assert_eq!(schema.elements.len(), 2);
        let led = schema.element("led").unwrap();
        assert_eq!(led.section, "devices");
        assert_eq!(led.examples[0].title, "minimal");
        assert_eq!(led.see_also, ["peripherals/gpio"]);
        let gpio = schema.element("gpio").unwrap();
        assert_eq!(gpio.attrs.len(), 2);
        assert!(gpio.attrs[1].required);

        let alloc = schema.get("alloc").unwrap();
        assert_eq!(alloc.default, Some(OptValue::Bool(true)));
        assert!(!alloc.derived);
        assert!(schema.get("embassy").unwrap().derived);
        assert_eq!(
            schema.get("mode").unwrap().default,
            Some(OptValue::Str("fast".into()))
        );
    }

    #[test]
    fn the_embedded_schema_loads_and_has_the_phase_one_vocabulary() {
        let schema = schema();
        for section in ["espforge", "peripherals", "devices", "services"] {
            assert!(schema.section_named(section).is_some(), "{section}");
        }
        // The `# devices` elements live beside the drivers that implement
        // them (`espforge-drivers/drivers.schema.kdl`), so this file is the
        // core vocabulary only: 19 elements.
        assert_eq!(schema.elements.len(), 19);
        for tag in [
            "project",
            "chip",
            "runtime",
            "console",
            "ota",
            "options",
            "gpio",
            "i2c",
            "spi",
            "uart",
            "wifi",
            "led",
            "button",
            "wifi_station",
            "http_client",
            "https_client",
            "ws_client",
            "wss_client",
            "ntp",
        ] {
            assert!(schema.element(tag).is_some(), "{tag}");
        }
        assert_eq!(schema.elements_in("peripherals").count(), 5);
        assert_eq!(schema.elements_in("devices").count(), 2);
        assert_eq!(schema.elements_in("services").count(), 6);

        // A provider declares the interface; a client names it, not a tag.
        let station = schema.element("wifi_station").unwrap();
        assert_eq!(station.provides.as_deref(), Some("network"));
        let http = schema.element("http_client").unwrap();
        let network = http.attrs.iter().find(|a| a.name == "network").unwrap();
        assert_eq!(
            network.kind,
            AttrKind::Ref {
                to: vec!["network".into()]
            }
        );

        let gpio = schema.element("gpio").unwrap();
        assert_eq!(gpio.section, "peripherals");
        let names: Vec<&str> = gpio.attrs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["id", "pin", "direction"]);
        assert!(gpio.attrs[0].required);
        assert!(gpio.attrs[1].required);
        assert!(!gpio.attrs[2].required);
        assert_eq!(
            gpio.attrs[2].kind,
            AttrKind::Choice(vec![
                "output".into(),
                "input".into(),
                "bidirectional".into()
            ])
        );

        let led = schema.element("led").unwrap();
        assert_eq!(led.section, "devices");
        assert_eq!(led.see_also, ["peripherals/gpio"]);
        let gpio_attr = led.attrs.iter().find(|a| a.name == "gpio").unwrap();
        assert_eq!(
            gpio_attr.kind,
            AttrKind::Ref {
                to: vec!["gpio".into()]
            }
        );
        // `pin="18"` stands alone, so neither pin named is required.
        assert!(!gpio_attr.required);
        let pin_attr = led.attrs.iter().find(|a| a.name == "pin").unwrap();
        assert_eq!(pin_attr.kind, AttrKind::Pin);
        assert!(!pin_attr.required);

        let button = schema.element("button").unwrap();
        assert_eq!(button.section, "devices");
        assert_eq!(button.see_also, ["peripherals/gpio"]);
        let button_gpio = button.attrs.iter().find(|a| a.name == "gpio").unwrap();
        assert_eq!(
            button_gpio.kind,
            AttrKind::Ref {
                to: vec!["gpio".into()]
            }
        );
        assert!(!button_gpio.required);
        let pull_up = button.attrs.iter().find(|a| a.name == "pull_up").unwrap();
        assert_eq!(pull_up.kind, AttrKind::Bool);
        assert!(!pull_up.required);
        assert_eq!(pull_up.default.as_deref(), Some("false"));
        // Only the options helloworld depends on; `wifi` arrives with the
        // radio that computes it.
        let names: Vec<&str> = schema.options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "unstable-hal",
                "alloc",
                "embassy",
                "wifi",
                "ota",
                "probe-rs",
                "stack-smashing-protection",
                "wokwi",
                "keep-zig-source"
            ]
        );
        let wifi = schema.get("wifi").unwrap();
        assert!(wifi.derived, "`wifi` follows the radio, not <options>");
        assert_eq!(wifi.requires.options, ["alloc", "unstable-hal", "embassy"]);
        assert!(
            wifi.compat.chips.is_none(),
            "a chip without a radio is refused by <wifi>, not by an option list"
        );
        assert_eq!(
            schema.get("unstable-hal").unwrap().default,
            Some(OptValue::Bool(true))
        );
        assert_eq!(
            schema.get("alloc").unwrap().default,
            Some(OptValue::Bool(true))
        );
        assert!(schema.get("embassy").unwrap().derived);
        assert_eq!(
            schema.get("wokwi").unwrap().default,
            Some(OptValue::Bool(true))
        );
        assert_eq!(
            schema.get("probe-rs").unwrap().default,
            Some(OptValue::Bool(false))
        );
        assert_eq!(
            schema.get("keep-zig-source").unwrap().default,
            Some(OptValue::Bool(false))
        );
    }

    /// A schema with one attribute whose `range` the test varies.
    fn ranged(range: &str) -> String {
        format!(
            r##"
section "peripherals" {{ summary "s"; description "d" }}
element "spi" section="peripherals" {{
    summary "SPI"
    attr "id" kind="ident" required=#true
    attr "mode" kind="u32" default="0" range="{range}"
    example "e" #"<spi id="spi2" />"#
}}
"##
        )
    }

    #[test]
    fn a_good_range_is_parsed_into_inclusive_bounds() {
        let schema = load(&ranged("0..=3")).unwrap();
        let mode = schema
            .element("spi")
            .unwrap()
            .attrs
            .iter()
            .find(|a| a.name == "mode")
            .unwrap();
        assert_eq!(mode.range, Some((0, 3)));
    }

    #[test]
    fn a_reversed_range_is_a_schema_error() {
        let errors = load(&ranged("3..=0")).unwrap_err();
        assert!(
            errors.iter().any(|e| e.message.contains("reversed")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_non_numeric_range_is_a_schema_error() {
        let errors = load(&ranged("zero..=three")).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is not `low..=high`")),
            "{errors:?}"
        );
    }

    #[test]
    fn every_example_fragment_parses_with_espf() {
        // A fragment is the element's own lines; the contract test wraps each
        // one in the five required sections, so this does too.
        fn wrapped(fragment: &str) -> String {
            if fragment.trim_start().starts_with('#') {
                fragment.to_string()
            } else {
                format!("# espforge\n{fragment}\n# peripherals\n# devices\n# services\n")
            }
        }

        let schema = schema();
        for element in &schema.elements {
            for example in &element.examples {
                let source = wrapped(&example.fragment);
                let parsed = espf::parser::parse_espf(&source);
                assert!(
                    parsed.structure_errors.is_empty(),
                    "`{}` example `{}` has structure errors: {:?}",
                    element.tag,
                    example.title,
                    parsed.structure_errors
                );
                assert!(
                    parsed.junk.is_empty(),
                    "`{}` example `{}` has junk: {:?}",
                    element.tag,
                    example.title,
                    parsed.junk
                );
            }
        }
    }

    #[test]
    fn bad_schemas_report_every_problem_with_a_span() {
        let src = r##"
section "peripherals" { summary "s"; description "d" }
element "led" section="peripherals" {
    summary "An LED"
    attr "gpio" kind="ref" to="gpio"
    example "one" #"<led id="l" />"#
}
"##;
        let errors = load(src).unwrap_err();
        let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert!(
            messages
                .iter()
                .any(|m| m.contains("unknown element or interface `gpio`")),
            "{messages:?}"
        );
        assert!(
            errors.iter().all(|e| !e.label.is_empty()),
            "every error explains itself"
        );
    }

    #[test]
    fn reserved_names_and_missing_summaries_are_errors() {
        let src = r##"
section "devices" { summary "s"; description "d" }
element "help" section="devices" {
    summary "s"
    example "e" #"<help />"#
}
element "bare" section="devices" {
    attr "id" kind="ident"
}
"##;
        let errors = load(src).unwrap_err();
        let messages: Vec<&str> = errors.iter().map(|e| e.message.as_str()).collect();
        assert!(
            messages.iter().any(|m| m.contains("reserved name")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("no summary")),
            "{messages:?}"
        );
        assert!(
            messages.iter().any(|m| m.contains("no example")),
            "{messages:?}"
        );
    }

    #[test]
    fn see_also_must_name_an_existing_element() {
        let src = r##"
section "devices" { summary "s"; description "d" }
element "led" section="devices" {
    summary "An LED"
    example "e" #"<led id="l" />"#
    see_also "peripherals/nope"
}
"##;
        let errors = load(src).unwrap_err();
        assert!(errors[0].message.contains("names no element"), "{errors:?}");
    }

    /// A schema with one peripheral and one recipe device over it.
    const RECIPE: &str = r##"
section "peripherals" { summary "s"; description "d" }
section "devices" { summary "s"; description "d" }
element "gpio" section="peripherals" {
    summary "A pin"
    attr "id" kind="ident" required=#true
    attr "pin" kind="pin" required=#true
    example "minimal" #"<gpio id="g" pin="4" />"#
}
element "led" section="devices" {
    summary "An LED"
    recipe pin="gpio" direction="output" runtime="espforge_runtime::Led" invalid="E0601"
    attr "id" kind="ident" required=#true
    attr "gpio" kind="ref" to="gpio" required=#true
    attr "active_low" kind="bool" default="false"
    example "minimal" #"<led id="l" gpio="$g" />"#
}
"##;

    /// A one-line failure, for tests that only care what went wrong.
    fn only_message(src: &str) -> String {
        let errors = load(src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        errors[0].message.clone()
    }

    /// A provider and a client whose slot count is a `const` field.
    const SERVICE_RECIPE: &str = r##"
section "services" { summary "s"; description "d" }
element "station" section="services" {
    summary "A provider"
    provides "network"
    attr "id" kind="ident" required=#true
    example "minimal" #"<station id="home" />"#
}
element "client" section="services" {
    summary "A client"
    attr "id" kind="ident" required=#true
    attr "network" kind="ref" to="network" required=#true
    attr "connections" kind="u32" default="1"
    service_recipe {
        network "network"
        feature "http-client"
        type "espforge_runtime::HttpClient"
        new "espforge_runtime::HttpClient::new_in"
        storage "espforge_runtime::HttpClientStorage"
        config "espforge_runtime::HttpClientConfig"
        field "connections" const=#true range="1..=4"
    }
    example "minimal" #"<client id="c" network="$home" />"#
}
"##;

    #[test]
    fn a_const_field_parameterizes_the_recipe() {
        let schema = load(SERVICE_RECIPE).unwrap();
        let recipe = schema
            .element("client")
            .unwrap()
            .service_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.fields.len(), 1);
        assert!(recipe.fields[0].is_const);
        assert_eq!(recipe.fields[0].range, Some((1, 4)));
        assert_eq!(
            recipe.storage.as_deref(),
            Some("espforge_runtime::HttpClientStorage")
        );
    }

    #[test]
    fn a_const_field_needs_a_u32_attribute_with_a_default_and_no_expression() {
        let src = SERVICE_RECIPE.replace(
            "attr \"connections\" kind=\"u32\" default=\"1\"",
            "attr \"connections\" kind=\"str\" default=\"1\"",
        );
        assert_eq!(
            only_message(&src),
            "`const` needs a `u32` attribute, but `client.connections` is string"
        );

        let src = SERVICE_RECIPE.replace(
            "attr \"connections\" kind=\"u32\" default=\"1\"",
            "attr \"connections\" kind=\"u32\"",
        );
        assert_eq!(
            only_message(&src),
            "`const` field `connections` needs a `default`"
        );

        let src = SERVICE_RECIPE.replace(
            "field \"connections\" const=#true",
            "field \"connections\" const=#true as=\"connections\"",
        );
        assert_eq!(
            only_message(&src),
            "`const` field `connections` has an `as=`"
        );
    }

    #[test]
    fn a_recipe_has_one_const_field() {
        let src = SERVICE_RECIPE
            .replace(
                "    attr \"connections\" kind=\"u32\" default=\"1\"\n",
                "    attr \"connections\" kind=\"u32\" default=\"1\"\n    attr \"slots\" kind=\"u32\" default=\"1\"\n",
            )
            .replace(
                "        field \"connections\" const=#true range=\"1..=4\"\n",
                "        field \"connections\" const=#true range=\"1..=4\"\n        field \"slots\" const=#true\n",
            );
        assert_eq!(only_message(&src), "more than one `const` field");
    }

    #[test]
    fn a_full_recipe_parses_into_for_facts() {
        let schema = load(RECIPE).unwrap();
        let recipe = schema.element("led").unwrap().recipe.as_ref().unwrap();
        assert_eq!(recipe.pin, "gpio");
        assert_eq!(recipe.direction, Direction::Output);
        assert_eq!(recipe.runtime, "espforge_runtime::Led");
        assert_eq!(recipe.invalid, codes::COMPONENT_INVALID_LED);
        assert!(recipe.span.start > 0 && recipe.span.end > recipe.span.start);
        // A non-recipe element carries none.
        assert!(schema.element("gpio").unwrap().recipe.is_none());
    }

    #[test]
    fn a_recipe_without_invalid_takes_the_generic_code() {
        let src = RECIPE.replace(" invalid=\"E0601\"", "");
        let schema = load(&src).unwrap();
        let recipe = schema.element("led").unwrap().recipe.as_ref().unwrap();
        assert_eq!(recipe.invalid, codes::COMPONENT_INVALID);
        assert_eq!(codes::COMPONENT_INVALID.as_str(), "E0615");
    }

    #[test]
    fn a_recipe_outside_devices_is_refused() {
        let src = r##"
section "peripherals" { summary "s"; description "d" }
section "devices" { summary "s"; description "d" }
element "gpio" section="peripherals" {
    summary "A pin"
    recipe pin="pin" direction="output" runtime="espforge_runtime::Led"
    attr "id" kind="ident" required=#true
    attr "pin" kind="pin" required=#true
    example "minimal" #"<gpio id="g" pin="4" />"#
}
"##;
        assert_eq!(only_message(src), "`recipe` is for devices");
    }

    #[test]
    fn pin_must_name_a_ref_attribute_and_lists_the_alternatives() {
        let src = RECIPE.replace("pin=\"gpio\"", "pin=\"nope\"");
        let errors = load(&src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].message, "`<led>` has no ref attribute `nope`");
        assert_eq!(errors[0].label, "recipe pin must name one of: `gpio`");
    }

    #[test]
    fn a_non_ident_non_bool_attribute_is_refused() {
        let src = RECIPE.replace(
            "    attr \"active_low\" kind=\"bool\" default=\"false\"\n",
            "    attr \"active_low\" kind=\"bool\" default=\"false\"\n    attr \"address\" kind=\"u32\"\n",
        );
        let errors = load(&src).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].message, "`led.address` is `u32`");
        assert!(
            errors[0].label.contains("write an `Element`"),
            "{}",
            errors[0].label
        );
    }

    #[test]
    fn runtime_must_be_a_rust_path() {
        let src = RECIPE.replace("espforge_runtime::Led", "Led!");
        assert_eq!(
            only_message(&src),
            "`runtime` must be a Rust path such as `espforge_runtime::Led`"
        );
        // A trailing `::` is not a path either.
        let src = RECIPE.replace("espforge_runtime::Led", "espforge_runtime::");
        assert_eq!(
            only_message(&src),
            "`runtime` must be a Rust path such as `espforge_runtime::Led`"
        );
    }

    #[test]
    fn invalid_must_be_a_workspace_code() {
        let src = RECIPE.replace("invalid=\"E0601\"", "invalid=\"E9999\"");
        assert_eq!(
            only_message(&src),
            "`E9999` is not a diagnostic code this workspace defines"
        );
    }

    #[test]
    fn direction_must_be_input_or_output() {
        let src = RECIPE.replace("direction=\"output\"", "direction=\"sideways\"");
        assert_eq!(
            only_message(&src),
            "`direction` must be `input` or `output`"
        );
    }

    #[test]
    fn a_second_recipe_node_is_refused() {
        let src = RECIPE.replace(
            "    recipe pin=\"gpio\" direction=\"output\" runtime=\"espforge_runtime::Led\" invalid=\"E0601\"\n",
            "    recipe pin=\"gpio\" direction=\"output\" runtime=\"espforge_runtime::Led\" invalid=\"E0601\"\n    recipe pin=\"gpio\" direction=\"output\" runtime=\"espforge_runtime::Led\"\n",
        );
        assert_eq!(only_message(&src), "`recipe` given twice");
    }

    /// A schema with one I2C peripheral and one device bound to it.
    const DEVICE_RECIPE: &str = r##"
section "peripherals" { summary "s"; description "d" }
section "devices" { summary "s"; description "d" }
element "i2c" section="peripherals" {
    summary "An I2C bus"
    attr "id" kind="ident" required=#true
    example "minimal" #"<i2c id="i2c0" />"#
}
element "sensor" section="devices" {
    summary "A sensor"
    device_recipe {
        bus "i2c"
        feature "sensor"
        kind "sensor"
        type "espforge_drivers::Sensor"
        new "espforge_drivers::Sensor::new"
        needs_address "0x62"
    }
    attr "id" kind="ident" required=#true
    attr "bus" kind="ref" to="i2c" required=#true
    attr "address" kind="hex" required=#true
    example "minimal" #"<sensor id="air" bus="$i2c0" address="0x62" />"#
}
"##;

    #[test]
    fn needs_address_parses_bare_and_with_a_fixed_value() {
        let schema = load(DEVICE_RECIPE).unwrap();
        let recipe = schema
            .element("sensor")
            .unwrap()
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.address, AddressNeed::Fixed(0x62));

        // Bare: any usable address the `.espf` declares.
        let src = DEVICE_RECIPE.replace(" needs_address \"0x62\"", " needs_address");
        let schema = load(&src).unwrap();
        let recipe = schema
            .element("sensor")
            .unwrap()
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.address, AddressNeed::Any);

        // Absent: the device takes whatever handle it is given.
        let src = DEVICE_RECIPE.replace("        needs_address \"0x62\"\n", "");
        let schema = load(&src).unwrap();
        let recipe = schema
            .element("sensor")
            .unwrap()
            .device_recipe
            .as_ref()
            .unwrap();
        assert_eq!(recipe.address, AddressNeed::None);
    }

    #[test]
    fn a_reserved_fixed_address_is_refused() {
        let src = DEVICE_RECIPE.replace("\"0x62\"", "\"0x7F\"");
        assert_eq!(only_message(&src), "`0x7F` is not a usable I2C address");
    }

    #[test]
    fn a_second_needs_address_is_refused() {
        let src = DEVICE_RECIPE.replace(
            "        needs_address \"0x62\"\n",
            "        needs_address \"0x62\"\n        needs_address \"0x62\"\n",
        );
        assert_eq!(only_message(&src), "`needs_address` given twice");
    }
}
