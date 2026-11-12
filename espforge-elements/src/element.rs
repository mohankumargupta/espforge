//! The element framework: what an element implements, and the contexts it gets.
//!
//! An [`Element`] owns one schema tag. It reads that tag's generated
//! [`Attrs`](crate::generated), reports problems through [`LowerCx`], and
//! pushes a [`Device`] whose [`Lowered`] implementation emits its own Rust
//! through [`EmitCx`]. Nothing here knows the name of a single element.
//!
//! Diagnostics are collected, not returned at the first one, like the core
//! reader: [`LowerCx::report`] pushes and returns the [`Reported`] marker an
//! element uses to stop its own lowering.

use std::ops::Range;

use espf::context::ParsedNode;
use espforge_diagnostics::codes;
use espforge_diagnostics::{DiagCode, Diagnostic};
use espforge_schema::Schema;
use proc_macro2::TokenStream;
use quote::format_ident;

use crate::model::{
    Board, Bus, BusId, ClaimedPin, Conflict, ConflictKind, Device, GpioDecl, Ident, Meta, Owner,
    PeripheralRefusal, Pin, PinUse, ResourceGraph, Service,
};

/// The names the generated code already binds, so an id that matches one of
/// them would produce a `rustc` error in `main.rs`/`lib.rs` instead of a
/// located diagnostic.
///
/// The list is the contract between this check and the emitter: a new local in
/// `main()` or a new `Context` field must be added here, and a test in
/// `espforge-emit` fails until it is.
pub const RESERVED_IDENTS: &[&str] = &[
    // `main()` locals and `Context` fields.
    "registry",
    "peripherals",
    "logger",
    "delay",
    "ctx",
    "devices",
    "services",
    "spawner",
    // Crate items and the `app.rs` contract.
    "app",
    "main",
    "setup",
    "forever",
    "Context",
    "CTX",
];

/// Which `# section` an element was lowered from.
///
/// Only the reserved-id check reads it: each section family has its own code,
/// so a reader can tell a device id from a service id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Peripherals,
    Devices,
    Services,
}

impl Section {
    /// The code a reserved id in this section reports.
    fn reserved_id_code(self) -> DiagCode {
        match self {
            // Peripherals have no reserved-id family of their own: `<gpio>` ids
            // are never bound as fields, so they reuse the legacy diagnostics code.
            Section::Peripherals => codes::COMPONENT_RESERVED_ID,
            Section::Services => codes::SERVICE_RESERVED_ID,
            Section::Devices => codes::DEVICE_RESERVED_ID,
        }
    }

    /// The word the diagnostic uses for this section.
    fn noun(self) -> &'static str {
        match self {
            Section::Peripherals => "peripheral",
            Section::Devices => "device",
            Section::Services => "service",
        }
    }
}

/// One element tag: reads its node and lowers it onto the board.
pub trait Element: Sync {
    /// The schema tag this element implements (`gpio`). It must name an
    /// element in `espf.schema.kdl`.
    fn tag(&self) -> &str;

    /// Read `node` and record what it lowers to in `cx`. A failure has
    /// already been pushed into `cx` and is reported through [`Reported`].
    ///
    /// `'s` is the schema lifetime: the element looks its own definition up in
    /// the context, which is why the context's schema must outlive the call.
    fn lower<'s>(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'s>) -> Result<(), Reported>;
}

/// A lowered element: it can name itself, emit its Rust, and answer the
/// overlay's `{{ <id>.<attr> }}` tokens.
pub trait Lowered {
    /// The element's id.
    fn id(&self) -> &Ident;

    /// Add this element's `Devices` or `Services` field and initialiser to `cx`.
    fn emit(&self, board: &Board, cx: &mut EmitCx);

    /// The value of `{{ <id>.<attr> }}` in an example overlay, if this
    /// element can answer `attr`.
    fn overlay_attr(&self, _attr: &str) -> Option<String> {
        None
    }
}

/// Proof that an element pushed a diagnostic into its [`LowerCx`].
///
/// Zero-sized on purpose: there is no way to fail lowering without saying
/// why, so a caller never sees a silent `Err`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reported;

/// Why a pin claim was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinRefusal {
    /// Another element owns the pin, or owns it for the opposite direction.
    Conflict(Conflict),
    /// The chip cannot do this on that pin. The diagnostic is already pushed.
    Capability,
}

impl PinRefusal {
    /// Push the refusal into `cx` and return the marker that stops the element.
    pub fn report(self, cx: &mut LowerCx<'_>) -> Reported {
        match self {
            PinRefusal::Conflict(conflict) => cx.report_conflict(&conflict),
            PinRefusal::Capability => Reported,
        }
    }
}

/// The lowering context: everything an element reads and everything it records.
///
/// It carries the schema this run lowers against, borrowed from the [`Session`]
/// that composed it — never a process global. A recipe element resolves its own
/// definition through [`Self::schema`], so two sessions own two different
/// definitions of one tag.
///
/// [`Session`]: crate::schema::Session
pub struct LowerCx<'s> {
    /// The schema this run lowers against: the bundled files plus the project's
    /// own drivers.
    schema: &'s Schema,
    errors: Vec<Diagnostic>,
    /// Warnings, kept apart from `errors` so they never stop lowering.
    warnings: Vec<Diagnostic>,
    gpios: Vec<GpioDecl>,
    resources: ResourceGraph,
    devices: Vec<Device>,
    services: Vec<Service>,
    /// `<wifi>` radio ids and the station riding on each, in source order.
    radios: Vec<(String, Option<String>)>,
    /// Interfaces each service provides, as `(interface, id)`
    /// (`provides "network"`): the things a service's `network=` reference can
    /// name. A tag is not recorded here — the reference resolves by capability.
    providers: Vec<(String, String)>,
    /// The chip and runtime the board is built for. A few element rules need
    /// them (a chip without a radio, a service under the blocking runtime).
    chip: String,
    runtime: crate::model::Runtime,
}

impl Default for LowerCx<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl LowerCx<'static> {
    /// The context for the bundled schema, which is what tests and hand-built
    /// boards lower against.
    pub fn new() -> Self {
        Self::with_schema(crate::schema::schema())
    }
}

impl<'s> LowerCx<'s> {
    /// The context for one run's schema.
    pub fn with_schema(schema: &'s Schema) -> Self {
        Self {
            schema,
            errors: Vec::new(),
            warnings: Vec::new(),
            gpios: Vec::new(),
            resources: ResourceGraph::default(),
            devices: Vec::new(),
            services: Vec::new(),
            radios: Vec::new(),
            providers: Vec::new(),
            chip: String::new(),
            runtime: crate::model::Runtime::Blocking,
        }
    }

    /// Record the chip and runtime the board is built for.
    pub fn set_meta(&mut self, meta: &Meta) {
        self.chip = meta.chip.clone();
        self.runtime = meta.runtime;
    }

    /// The chip the board is built for.
    pub fn chip(&self) -> &str {
        &self.chip
    }

    /// The async model the board is built for.
    pub fn runtime(&self) -> crate::model::Runtime {
        self.runtime
    }

    /// The schema this run lowers against.
    pub fn schema(&self) -> &'s Schema {
        self.schema
    }

    /// Record a located problem and return the marker an element uses to stop.
    pub fn report(
        &mut self,
        code: DiagCode,
        span: Range<usize>,
        message: impl Into<String>,
        label: impl Into<String>,
    ) -> Reported {
        self.errors
            .push(Diagnostic::error(code, span, message, label));
        Reported
    }

    /// Record a warning. It does not stop lowering and does not make the
    /// build fail; it rides on the board so the pipeline can print it.
    pub fn warn(
        &mut self,
        code: DiagCode,
        span: Range<usize>,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.warnings
            .push(Diagnostic::warning(code, span, message, label));
    }

    /// Record a lowered `<wifi>` radio.
    pub fn add_radio(&mut self, id: impl Into<String>) {
        self.radios.push((id.into(), None));
    }

    /// Whether `<wifi id="name">` was lowered.
    pub fn has_radio(&self, name: &str) -> bool {
        self.radios.iter().any(|(radio, _)| radio == name)
    }

    /// The station already riding on `radio`, if any.
    pub fn radio_station(&self, radio: &str) -> Option<&str> {
        self.radios
            .iter()
            .find(|(id, _)| id == radio)
            .and_then(|(_, station)| station.as_deref())
    }

    /// Record the station riding on `radio`.
    pub fn set_radio_station(&mut self, radio: &str, station: impl Into<String>) {
        if let Some((_, slot)) = self.radios.iter_mut().find(|(id, _)| id == radio) {
            *slot = Some(station.into());
        }
    }

    /// Record that `id` provides `interface` (`provides "network"`), so a
    /// service's `network=` reference resolves by capability, not by tag.
    pub fn add_provider(&mut self, interface: &str, id: impl Into<String>) {
        self.providers.push((interface.to_string(), id.into()));
    }

    /// Whether `name` is a service that provides `interface`.
    pub fn has_provider(&self, interface: &str, name: &str) -> bool {
        self.providers
            .iter()
            .any(|(provided, id)| provided == interface && id == name)
    }

    /// The `<gpio>` a device refers to by id.
    pub fn gpio(&self, name: &str) -> Option<&GpioDecl> {
        self.gpios.iter().find(|gpio| gpio.id.raw == name)
    }

    /// Record a lowered `<gpio>`.
    pub fn add_gpio(&mut self, decl: GpioDecl) {
        self.gpios.push(decl);
    }

    /// Every lowered `<gpio>`, in source order.
    pub fn gpios(&self) -> &[GpioDecl] {
        &self.gpios
    }

    /// Append a bus and return its id.
    pub fn add_bus(&mut self, bus: Bus) -> BusId {
        self.resources.add_bus(bus)
    }

    pub fn bus(&self, id: BusId) -> Option<&Bus> {
        self.resources.bus(id)
    }

    pub fn bus_mut(&mut self, id: BusId) -> Option<&mut Bus> {
        self.resources.bus_mut(id)
    }

    pub fn bus_by_name(&self, name: &str) -> Option<BusId> {
        self.resources.bus_by_name(name)
    }

    pub fn buses(&self) -> &[Bus] {
        self.resources.buses()
    }

    /// Claim a physical peripheral name (`UART1`), reporting a refusal.
    ///
    /// Two refusals, two wordings: another element with the same name is the
    /// legacy `peripheral clash`, underlined at the first claim; a name the
    /// generated runtime holds is a located error on the element that cannot
    /// have it, naming the holder and the way out.
    pub fn claim_peripheral(
        &mut self,
        peripheral: impl Into<String>,
        owner: Owner,
        span: Range<usize>,
    ) -> Result<(), Reported> {
        let refusal = match self
            .resources
            .claim_peripheral(peripheral, owner, span.clone())
        {
            Ok(()) => return Ok(()),
            Err(refusal) => refusal,
        };
        Err(match refusal {
            PeripheralRefusal::Clash(clash) => self.report(
                codes::CONFLICT_PERIPHERAL,
                clash.existing.span.clone(),
                "peripheral clash",
                format!(
                    "{} and {} both use {}",
                    element_name(&clash.existing.owner),
                    element_name(&clash.incoming.owner),
                    clash.peripheral
                ),
            ),
            PeripheralRefusal::Held(clash) => self.report(
                codes::CONFLICT_RUNTIME_HELD,
                span,
                format!("{} is held by {}", clash.peripheral, clash.held.holder),
                format!(
                    "{} cannot use it; {}",
                    element_name(&clash.owner),
                    clash.held.remedy
                ),
            ),
        })
    }

    /// Record a resource the generated runtime already holds (`UART0`).
    pub fn hold(
        &mut self,
        resource: impl Into<String>,
        holder: impl Into<String>,
        remedy: impl Into<String>,
    ) {
        self.resources.hold(resource, holder, remedy);
    }

    /// Whether anything has been reported so far.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Index into the claim journal, taken immediately before one element's
    /// lowering call. See [`ResourceGraph::checkpoint`].
    pub fn checkpoint(&self) -> usize {
        self.resources.checkpoint()
    }

    /// Undo every claim made since `checkpoint`, newest first.
    pub fn rollback(&mut self, checkpoint: usize) {
        self.resources.rollback(checkpoint);
    }

    /// Claim `pin` for `owner`, after the chip's own pin rules.
    ///
    /// A refusal is either the graph's conflict (both sides carried, so the
    /// element can keep its own wording) or a hardware capability problem,
    /// which is reported here and leaves no claim behind.
    pub fn claim_pin(
        &mut self,
        pin: Pin,
        owner: Owner,
        span: Range<usize>,
        need: PinUse,
    ) -> Result<ClaimedPin, PinRefusal> {
        if self.check_pin_caps(pin, &span, need) {
            return Err(PinRefusal::Capability);
        }
        self.resources
            .claim(pin, owner, span, need)
            .map_err(PinRefusal::Conflict)
    }

    /// Apply the chip's pin table to one claim, reporting what it forbids.
    ///
    /// Returns whether the claim was refused. The checks run before the graph
    /// records anything, so a refused pin is never left claimed and there is
    /// nothing for the element's rollback to undo.
    fn check_pin_caps(&mut self, pin: Pin, span: &Range<usize>, need: PinUse) -> bool {
        let Some(caps) = crate::pin_caps::caps(&self.chip) else {
            self.report(
                codes::PIN_CAPS_MISSING,
                span.clone(),
                format!("no pin table for chip `{}`", self.chip),
                "espforge does not know this chip's pins, and will not guess",
            );
            return true;
        };
        if pin.0 >= caps.pins {
            self.report(
                codes::GPIO_NOT_ON_CHIP,
                span.clone(),
                format!("GPIO{} does not exist on {}", pin.0, self.chip),
                format!("this chip has GPIO0..GPIO{}", caps.pins - 1),
            );
            return true;
        }
        if caps.flash.contains(&pin.0) {
            self.report(
                codes::GPIO_RESERVED_FLASH,
                span.clone(),
                format!("GPIO{} is reserved for SPI flash on {}", pin.0, self.chip),
                "this pin is wired to the flash/PSRAM bus; pick another GPIO",
            );
            return true;
        }
        if matches!(need, PinUse::Output | PinUse::Bidirectional)
            && caps.input_only.contains(&pin.0)
        {
            self.report(
                codes::GPIO_NOT_OUTPUT_CAPABLE,
                span.clone(),
                format!("GPIO{} is input-only on {}", pin.0, self.chip),
                "drive this pin as an input, or pick an output-capable GPIO",
            );
            return true;
        }
        if caps.strapping.contains(&pin.0) {
            self.warn(
                codes::GPIO_STRAPPING,
                span.clone(),
                format!("GPIO{} is a strapping pin on {}", pin.0, self.chip),
                "its level at boot selects a boot mode; check the board before driving it",
            );
        }
        false
    }

    /// Report a refused pin claim in the corpus wording.
    pub fn report_conflict(&mut self, conflict: &Conflict) -> Reported {
        let report = conflict_diagnostic(conflict);
        self.report(report.code, report.span, report.message, report.label)
    }

    /// Record a lowered device.
    pub fn push_device(&mut self, device: Device) {
        self.devices.push(device);
    }

    /// Every lowered device, in source order.
    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// Record a lowered service.
    pub fn push_service(&mut self, service: Service) {
        self.services.push(service);
    }

    /// Every lowered service, in source order.
    pub fn services(&self) -> &[Service] {
        &self.services
    }

    /// The node's id, with the span of the id attribute's value.
    ///
    /// Raw: it says nothing about whether the id can be bound in generated
    /// code. Elements that do bind it call [`Self::checked_ident`].
    pub fn ident(&self, node: &ParsedNode<'_>) -> Ident {
        let span = node
            .get_attr_spanned("id")
            .map(|attr| attr.val_offset..attr.val_offset + attr.val.len())
            .unwrap_or(node.offset..node.offset);
        Ident::new(node.id, span)
    }

    /// [`Self::ident`], or one diagnostic when the id names something the
    /// generated code already binds.
    ///
    /// Three spellings are refused: a name from [`RESERVED_IDENTS`], a leading
    /// `__` (the prefix the generated storage statics use), and a Rust 2024
    /// keyword. The diagnostic points at the id value, not the node, so the fix
    /// is a rename of exactly that text. An id that is not an identifier at all
    /// is left to the section's own rule, which already words it for its
    /// section (a `9led` reads as an invalid device id, not a reserved one).
    pub fn checked_ident(
        &mut self,
        node: &ParsedNode<'_>,
        section: Section,
    ) -> Result<Ident, Reported> {
        let id = self.ident(node);
        let name = id.rust_name();
        let problem = if RESERVED_IDENTS.contains(&name.as_str()) {
            Some(format!(
                "`{name}` is a name the generated code binds; rename the id"
            ))
        } else if name.starts_with("__") {
            Some(
                "ids starting with `__` are reserved for the generated storage; rename the id"
                    .to_string(),
            )
        } else if is_rust_ident(&name) && syn::parse_str::<syn::Ident>(&name).is_err() {
            Some(format!("`{name}` is a Rust keyword; rename the id"))
        } else {
            None
        };

        match problem {
            Some(label) => Err(self.report(
                section.reserved_id_code(),
                id.span.clone(),
                format!("reserved {} id", section.noun()),
                format!("<{} id=\"{}\">: {label}", node.tag_name, id.raw),
            )),
            None => Ok(id),
        }
    }

    /// Finish lowering. Every collected problem is returned together; a board
    /// is only built when there are none.
    pub fn into_board(self, meta: Meta) -> Result<Board, Vec<Diagnostic>> {
        if self.errors.is_empty() {
            Ok(Board {
                meta,
                resources: self.resources.seal(),
                devices: self.devices,
                services: self.services,
                warnings: self.warnings,
            })
        } else {
            Err(self.errors)
        }
    }
}

/// The locals a `Lowered::emit` startup statement may name in `main()`.
///
/// Typed rather than a documented sentence: a typo in a startup statement is
/// then a compile error in the emitter, not a test failure someone may forget
/// to run.
#[derive(Debug, Clone)]
pub struct MainCtx {
    pub registry: proc_macro2::Ident,
    pub spawner: proc_macro2::Ident,
    pub services: proc_macro2::Ident,
}

impl MainCtx {
    /// The `main()` locals this context names, as written.
    ///
    /// The emitter's drift test checks each one against [`RESERVED_IDENTS`]: a
    /// new local that is not reserved there lets a user id collide with it.
    pub fn names(&self) -> [String; 3] {
        [
            self.registry.to_string(),
            self.spawner.to_string(),
            self.services.to_string(),
        ]
    }
}

impl Default for MainCtx {
    fn default() -> Self {
        Self {
            registry: format_ident!("registry"),
            spawner: format_ident!("spawner"),
            services: format_ident!("services"),
        }
    }
}

/// The emission context: a plain collector with no knowledge of any element.
#[derive(Debug, Default)]
pub struct EmitCx {
    module_items: Vec<TokenStream>,
    once: std::collections::BTreeSet<String>,
    device_fields: Vec<TokenStream>,
    device_inits: Vec<TokenStream>,
    service_fields: Vec<TokenStream>,
    service_inits: Vec<TokenStream>,
    /// Statements `Services::new` runs before `Self { .. }`, in board order:
    /// a provider's local (`let home = …;`) so its clients can call
    /// `home.stack()` without an intermediate provider field.
    service_prelude: Vec<TokenStream>,
    startup: Vec<TokenStream>,
    /// Statements `Devices::new` runs before `Self { .. }`.
    device_prelude: Vec<TokenStream>,
    main: MainCtx,
}

/// What an [`EmitCx`] collected, in board order.
pub struct EmitParts {
    /// Module-scope items the elements own (the generated `static` storage), in
    /// board order. They outlive `Devices::new`, so they cannot live in it.
    pub module_items: Vec<TokenStream>,
    /// One field of the generated `Devices` struct.
    pub device_fields: Vec<TokenStream>,
    /// One initialiser inside `Devices::new`.
    pub device_inits: Vec<TokenStream>,
    /// One field of the generated `Services` struct.
    pub service_fields: Vec<TokenStream>,
    /// One initialiser inside `Services::new`.
    pub service_inits: Vec<TokenStream>,
    /// Statements `Services::new` runs before `Self { .. }`, in board order.
    pub service_prelude: Vec<TokenStream>,
    /// Statements for `main()` after the scheduler starts, in first-use order.
    pub startup: Vec<TokenStream>,
    /// Statements `Devices::new` runs before `Self { .. }`, in board order:
    /// a device's bus is built here, inside `Devices::new`.
    pub device_prelude: Vec<TokenStream>,
}

impl EmitCx {
    pub fn new() -> Self {
        Self::default()
    }

    /// The same, with the `main()` locals handed in.
    pub fn with_main(main: MainCtx) -> Self {
        Self {
            main,
            ..Self::default()
        }
    }

    /// One statement `Devices::new` runs before `Self { .. }`.
    pub fn device_prelude(&mut self, statement: TokenStream) {
        self.device_prelude.push(statement);
    }

    /// A statement `Devices::new` runs before `Self { .. }`, emitted once per
    /// key. Returns whether this call inserted it.
    ///
    /// The bus builder is the caller: two devices on one bus share one `let`,
    /// and a bus with no device is never built.
    pub fn device_once(&mut self, key: &str, statement: TokenStream) -> bool {
        if self.first(key) {
            self.device_prelude.push(statement);
            true
        } else {
            false
        }
    }

    /// The locals a startup statement names.
    pub fn main(&self) -> &MainCtx {
        &self.main
    }

    /// One field of the generated `Devices` struct.
    pub fn device_field(&mut self, field: TokenStream) {
        self.device_fields.push(field);
    }

    /// One initialiser inside `Devices::new`.
    pub fn device_init(&mut self, init: TokenStream) {
        self.device_inits.push(init);
    }

    /// One field of the generated `Services` struct.
    pub fn service_field(&mut self, field: TokenStream) {
        self.service_fields.push(field);
    }

    /// One initialiser inside `Services::new`.
    pub fn service_init(&mut self, init: TokenStream) {
        self.service_inits.push(init);
    }

    /// One statement `Services::new` runs before `Self { .. }`.
    pub fn service_prelude(&mut self, statement: TokenStream) {
        self.service_prelude.push(statement);
    }

    /// A statement for `main()` after the scheduler starts, in first-use order.
    pub fn startup(&mut self, statement: TokenStream) {
        self.startup.push(statement);
    }

    /// One field of the generated `Devices` struct.
    ///
    /// A bus handle, an addressed endpoint and a driver are all constructed
    /// devices.
    pub fn field(&mut self, field: TokenStream) {
        self.device_fields.push(field);
    }

    /// One initialiser inside `Devices::new`.
    pub fn init(&mut self, init: TokenStream) {
        self.device_inits.push(init);
    }

    /// A module-scope item: generated storage statics, which must outlive the
    /// constructor that fills them.
    pub fn module_item(&mut self, item: TokenStream) {
        self.module_items.push(item);
    }

    /// A statement that must appear before `Self { .. }`, emitted at most once
    /// per key, in first-use order. Returns whether this call inserted it.
    ///
    /// The SPI prelude is the caller: two devices on one bus must not build
    /// the bus twice, and a bus with no device must not be built at all.
    pub fn once(&mut self, key: &str, statement: TokenStream) -> bool {
        self.device_once(key, statement)
    }

    /// Mark `key` as seen, and report whether this is the first time. Unlike
    /// [`Self::once`] it prints nothing, so a consumed bus can own the
    /// statement itself and still keep its module items unique.
    pub fn first(&mut self, key: &str) -> bool {
        self.once.insert(key.to_string())
    }

    pub fn into_parts(self) -> EmitParts {
        EmitParts {
            module_items: self.module_items,
            device_fields: self.device_fields,
            device_inits: self.device_inits,
            service_fields: self.service_fields,
            service_inits: self.service_inits,
            service_prelude: self.service_prelude,
            startup: self.startup,
            device_prelude: self.device_prelude,
        }
    }
}

/// A refused claim in the wording the diagnostics corpus pins: the code, the
/// title, the label and which span is underlined.
pub struct ConflictReport {
    pub code: DiagCode,
    pub message: &'static str,
    pub label: String,
    pub span: Range<usize>,
}

/// One element's name in a claim diagnostic: `<uart id="gps">`.
fn element_name(owner: &Owner) -> String {
    format!("<{} id=\"{}\">", owner.kind.tag(), owner.id.raw)
}

/// The legacy wording for a refused claim, kept with the graph so every
/// element reports conflicts the same way.
///
/// The blame order and primary span follow the legacy `conflicts.rs` loops for
/// the cases the corpus pins. The corpus fixes each word: never rewrite one of
/// these labels without re-running it.
pub fn conflict_diagnostic(conflict: &Conflict) -> ConflictReport {
    let pin = conflict.pin.0;
    let existing = &conflict.existing;
    let incoming = &conflict.incoming;

    match conflict.kind {
        ConflictKind::Direction => ConflictReport {
            code: espforge_diagnostics::codes::CONFLICT_DIRECTION,
            message: "pin direction clash",
            label: format!(
                "{} and {}: one pin cannot be both input and output",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            span: incoming.span.clone(),
        },
        ConflictKind::SelfClash => match existing.owner.kind {
            crate::model::OwnerKind::I2c => ConflictReport {
                code: espforge_diagnostics::codes::CONFLICT_I2C_WIRING,
                message: "i2c wiring",
                label: format!(
                    "{} sda and scl cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                span: incoming.span.clone(),
            },
            crate::model::OwnerKind::Spi => ConflictReport {
                code: espforge_diagnostics::codes::CONFLICT_SPI_WIRING,
                message: "spi wiring",
                label: format!(
                    "{} sclk and mosi cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                span: incoming.span.clone(),
            },
            crate::model::OwnerKind::Uart => ConflictReport {
                code: espforge_diagnostics::codes::CONFLICT_UART_WIRING,
                message: "uart wiring",
                label: format!(
                    "{} tx and rx cannot both be GPIO{pin}",
                    display(&existing.owner, false)
                ),
                span: incoming.span.clone(),
            },
            _ => ConflictReport {
                code: espforge_diagnostics::codes::CONFLICT_PIN_USED,
                message: "pin already used",
                label: format!("{} claims GPIO{pin} twice", display(&existing.owner, false)),
                span: incoming.span.clone(),
            },
        },
        ConflictKind::Pin => pin_conflict(conflict),
    }
}

fn pin_conflict(conflict: &Conflict) -> ConflictReport {
    use crate::model::OwnerKind::*;

    let pin = conflict.pin.0;
    let existing = &conflict.existing;
    let incoming = &conflict.incoming;
    let (code, message, label, span) = match (&existing.owner.kind, &incoming.owner.kind) {
        (Pin(a), Pin(b)) if a == b => (
            espforge_diagnostics::codes::CONFLICT_PIN_USED,
            "pin already used",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            incoming.span.clone(),
        ),
        (I2c, Uart) | (Uart, I2c) => {
            let (i2c, uart) = if existing.owner.kind == I2c {
                (existing, incoming)
            } else {
                (incoming, existing)
            };
            (
                espforge_diagnostics::codes::CONFLICT_PIN,
                "pin conflict",
                format!(
                    "{} and {} both use GPIO{pin}",
                    display(&i2c.owner, false),
                    display(&uart.owner, false)
                ),
                i2c.span.clone(),
            )
        }
        (_, Pin(_)) => (
            espforge_diagnostics::codes::CONFLICT_PIN,
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&incoming.owner, true),
                display(&existing.owner, false)
            ),
            incoming.span.clone(),
        ),
        (Pin(_), _) => (
            espforge_diagnostics::codes::CONFLICT_PIN,
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, true),
                display(&incoming.owner, false)
            ),
            existing.span.clone(),
        ),
        _ => (
            espforge_diagnostics::codes::CONFLICT_PIN,
            "pin conflict",
            format!(
                "{} and {} both use GPIO{pin}",
                display(&existing.owner, false),
                display(&incoming.owner, false)
            ),
            existing.span.clone(),
        ),
    };
    ConflictReport {
        code,
        message,
        label,
        span,
    }
}

fn display(owner: &Owner, slug: bool) -> String {
    let id = if slug {
        owner.id.rust_name()
    } else {
        owner.id.raw.clone()
    };
    format!("<{} id=\"{id}\">", owner.kind.tag())
}

/// The Rust-identifier rule every device id must satisfy.
pub fn is_rust_ident(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(|c: char| c.is_ascii_digit())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn once_emits_one_statement_per_key_in_first_use_order() {
        let mut cx = EmitCx::new();
        assert!(cx.once("spi2_bus", quote! { let a = 1; }));
        assert!(!cx.once("spi2_bus", quote! { let a = 1; }));
        assert!(cx.once("spi3_bus", quote! { let b = 2; }));

        let parts = cx.into_parts();
        let statements: Vec<String> = parts
            .device_prelude
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(statements, ["let a = 1 ;", "let b = 2 ;"]);
    }
}
