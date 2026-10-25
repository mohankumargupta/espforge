//! The typed IR: one `Board`, built once, immutable afterwards.
//!
//! Phase 2 of the extreme refactor replaces the `RuntimeContext` `$` graph,
//! the `*Draft` structs and the seven parallel `Vec`s with this model. The
//! important property is the one in extreme.md §3.3: **validation and
//! construction are the same act**. A [`ClaimedPin`] can only be made by
//! [`ResourceGraph::claim`], so a `Board` that holds one cannot have a pin
//! conflict; there is no later `conflicts.rs` pass to forget.
//!
//! Element wording for a [`Conflict`] stays in the lowering code that knows
//! which element is being built; the graph records both owners and both spans
//! so that wording can be exactly what it is today. The emitter reads this
//! model directly (Phase 3 deleted the `Hardware` projection it used to
//! read).
//!
//! ```text
//! ResourceGraph::claim(Pin(18), Owner::new(OwnerKind::Led, id), node_span, PinUse::Output)
//!     -> Ok(ClaimedPin)          // free pin, now owned
//!     -> Err(Conflict { kind, pin, existing, incoming })
//! ```

use std::collections::BTreeMap;

use crate::Meta;

/// A byte range into the original `.espf` source.
pub type Span = std::ops::Range<usize>;

/// An element id and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    /// The id exactly as written (`red-led`).
    pub raw: String,
    /// Span of the id attribute's value.
    pub span: Span,
}

impl Ident {
    pub fn new(raw: impl Into<String>, span: Span) -> Self {
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

/// Where a bus lives in [`ResourceGraph::buses`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BusId(pub usize);

/// What a pin is asked to do. Two claims that demand opposite directions are a
/// wiring error; anything else on an occupied pin is a plain conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinUse {
    Input,
    Output,
    /// A bus pin that both sides drive (I2C SDA/SCL) or that the peripheral
    /// samples at its own rate (SPI MISO).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    Gpio,
    Led,
    Button,
    I2c,
    Spi,
    SpiComponent,
    Uart,
}

impl OwnerKind {
    /// The schema tag spelling (`spi_component`).
    pub fn tag(self) -> &'static str {
        match self {
            OwnerKind::Gpio => "gpio",
            OwnerKind::Led => "led",
            OwnerKind::Button => "button",
            OwnerKind::I2c => "i2c",
            OwnerKind::Spi => "spi",
            OwnerKind::SpiComponent => "spi_component",
            OwnerKind::Uart => "uart",
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
    pub span: Span,
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

/// Two elements claiming the same hardware peripheral (`UART1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralClash {
    pub peripheral: String,
    pub existing: Claim,
    pub incoming: Claim,
}

/// Pins, peripherals and buses: the resources a board owns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceGraph {
    pins: BTreeMap<Pin, Claim>,
    peripherals: BTreeMap<String, Claim>,
    buses: Vec<Bus>,
}

impl ResourceGraph {
    pub fn new() -> Self {
        Self::default()
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
        span: Span,
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
        Ok(ClaimedPin {
            pin,
            claim: incoming,
        })
    }

    /// Claim a named hardware peripheral (`UART1`, `I2C0`) for `owner`.
    ///
    /// The current model drives at most one component per peripheral, so a
    /// second claim is the "peripheral clash" case.
    pub fn claim_peripheral(
        &mut self,
        peripheral: impl Into<String>,
        owner: Owner,
        span: Span,
    ) -> Result<(), PeripheralClash> {
        let peripheral = peripheral.into();
        let incoming = Claim {
            owner,
            span,
            need: PinUse::Unspecified,
        };

        if let Some(existing) = self.peripherals.get(&peripheral) {
            return Err(PeripheralClash {
                peripheral,
                existing: existing.clone(),
                incoming,
            });
        }

        self.peripherals.insert(peripheral, incoming);
        Ok(())
    }

    /// Append a bus and return its id. Bus ids are positional and assigned in
    /// construction order (which is source order).
    pub fn add_bus(&mut self, bus: Bus) -> BusId {
        let id = BusId(self.buses.len());
        self.buses.push(bus);
        id
    }

    pub fn bus(&self, id: BusId) -> Option<&Bus> {
        self.buses.get(id.0)
    }

    /// The bus, for lowering to attach its component while the folder is
    /// still being assembled.
    pub fn bus_mut(&mut self, id: BusId) -> Option<&mut Bus> {
        self.buses.get_mut(id.0)
    }

    pub fn buses(&self) -> &[Bus] {
        &self.buses
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
    pub fn peripheral_claims(&self) -> impl Iterator<Item = (&str, &Claim)> {
        self.peripherals
            .iter()
            .map(|(name, claim)| (name.as_str(), claim))
    }
}

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
        /// The component riding on this bus, once one is attached. The model
        /// allows one component per bus.
        used_by: Option<Ident>,
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
    },
    Uart {
        id: Ident,
        /// Upper-cased esp-hal peripheral name (`UART0`, `UART1`), which may
        /// differ from the id when `uart="UART1"` is written explicitly.
        peripheral: String,
        tx: ClaimedPin,
        rx: ClaimedPin,
        used_by: Option<Ident>,
    },
}

impl Bus {
    pub fn id(&self) -> &Ident {
        match self {
            Bus::I2c { id, .. } | Bus::Spi { id, .. } | Bus::Uart { id, .. } => id,
        }
    }
}

/// UART parity, in the IR's own vocabulary. The lowering maps the generated
/// schema choice onto this; the backend maps it onto `esp_hal`.
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

/// One `<led>` over a claimed pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Led {
    pub id: Ident,
    pub pin: ClaimedPin,
    pub active_low: bool,
}

/// One `<button>` over a claimed pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: Ident,
    pub pin: ClaimedPin,
    pub pull_up: bool,
}

/// One `<i2c_component>` on a bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cComponent {
    pub id: Ident,
    pub bus: BusId,
    /// The address the component declares, when it has one. A bus-level
    /// component such as an I2C scanner deliberately has none.
    pub address: Option<u8>,
}

/// One `<spi_component>` on a bus, with its own chip-select claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiComponent {
    pub id: Ident,
    pub bus: BusId,
    pub cs: ClaimedPin,
    pub mode: u8,
}

/// One `<uart_component>` on a UART peripheral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UartComponent {
    pub id: Ident,
    pub port: BusId,
    pub baud: u32,
    pub framing: Framing,
}

/// A component in the board. Adding `<pwm>` is one variant plus lowering plus
/// emission, never a new string tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    Led(Led),
    Button(Button),
    I2c(I2cComponent),
    Spi(SpiComponent),
    Uart(UartComponent),
}

impl Component {
    pub fn id(&self) -> &Ident {
        match self {
            Component::Led(c) => &c.id,
            Component::Button(c) => &c.id,
            Component::I2c(c) => &c.id,
            Component::Spi(c) => &c.id,
            Component::Uart(c) => &c.id,
        }
    }
}

/// The `<wifi>` service. Both strings are written into Rust string literals,
/// so the caller must have rejected `\\`, `"` and line breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiService {
    pub ssid: String,
    /// Empty means an open network.
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Service {
    Wifi(WifiService),
}

/// One board: the project settings, the resources it owns, the components on
/// them and the services it asks for.
///
/// `Board::build` (the only constructor) lands in increment 3; until then the
/// fields are assembled by lowering and by tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub meta: Meta,
    pub resources: ResourceGraph,
    pub components: Vec<Component>,
    pub services: Vec<Service>,
}

impl Board {
    /// The bus a component rides on.
    pub fn bus(&self, id: BusId) -> Option<&Bus> {
        self.resources.bus(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Console, Meta, Runtime};

    fn id(raw: &str, at: usize) -> Ident {
        Ident::new(raw, at..at + raw.len())
    }

    fn owner(kind: OwnerKind, raw: &str, at: usize) -> Owner {
        Owner::new(kind, id(raw, at))
    }

    fn meta() -> Meta {
        Meta {
            name: "board".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
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
            owner(OwnerKind::Led, name, at + 1),
            at..at + 4 + name.len(),
            PinUse::Output,
        )
    }

    fn button_claim(
        g: &mut ResourceGraph,
        pin: u8,
        name: &str,
        at: usize,
    ) -> Result<ClaimedPin, Conflict> {
        g.claim(
            Pin(pin),
            owner(OwnerKind::Button, name, at + 1),
            at..at + 7 + name.len(),
            PinUse::Input,
        )
    }

    #[test]
    fn claiming_a_free_pin_records_its_owner_and_span() {
        let mut g = ResourceGraph::new();
        let p = led_claim(&mut g, 18, "red-led", 40).unwrap();

        assert_eq!(p.pin(), Pin(18));
        assert_eq!(p.owner().kind, OwnerKind::Led);
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
    fn opposite_directions_are_a_direction_clash() {
        let mut g = ResourceGraph::new();
        led_claim(&mut g, 18, "red_led", 10).unwrap();
        let conflict = button_claim(&mut g, 18, "btn", 40).unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::Direction);
        assert_eq!(conflict.existing.owner.kind, OwnerKind::Led);
        assert_eq!(conflict.incoming.owner.kind, OwnerKind::Button);
    }

    #[test]
    fn a_bus_pin_does_not_clash_with_a_led_by_direction() {
        // I2C SDA is bidirectional: a led on the same pin is a plain conflict,
        // not an input/output clash.
        let mut g = ResourceGraph::new();
        g.claim(
            Pin(4),
            owner(OwnerKind::I2c, "i2c0", 10),
            10..40,
            PinUse::Bidirectional,
        )
        .unwrap();
        let conflict = led_claim(&mut g, 4, "red_led", 50).unwrap_err();
        assert_eq!(conflict.kind, ConflictKind::Pin);
    }

    #[test]
    fn one_element_claiming_the_same_pin_twice_is_a_self_clash() {
        let mut g = ResourceGraph::new();
        let uart = owner(OwnerKind::Uart, "uart1", 10);
        g.claim(Pin(5), uart.clone(), 10..35, PinUse::Unspecified)
            .unwrap();
        let conflict = g
            .claim(Pin(5), uart, 10..35, PinUse::Unspecified)
            .unwrap_err();

        assert_eq!(conflict.kind, ConflictKind::SelfClash);
        assert_eq!(conflict.existing.owner.id.raw, "uart1");
        assert_eq!(conflict.incoming.owner.id.raw, "uart1");
    }

    #[test]
    fn a_peripheral_can_only_be_claimed_once() {
        let mut g = ResourceGraph::new();
        g.claim_peripheral("UART1", owner(OwnerKind::Uart, "uart1", 10), 10..35)
            .unwrap();
        let clash = g
            .claim_peripheral("UART1", owner(OwnerKind::Uart, "other", 40), 40..65)
            .unwrap_err();

        assert_eq!(clash.peripheral, "UART1");
        assert_eq!(clash.existing.owner.id.raw, "uart1");
        assert_eq!(clash.incoming.owner.id.raw, "other");
    }

    #[test]
    fn buses_are_indexed_in_creation_order() {
        let mut g = ResourceGraph::new();
        let tx = g
            .claim(
                Pin(20),
                owner(OwnerKind::Uart, "uart1", 0),
                0..10,
                PinUse::Unspecified,
            )
            .unwrap();
        let rx = g
            .claim(
                Pin(21),
                owner(OwnerKind::Uart, "uart1", 0),
                0..10,
                PinUse::Unspecified,
            )
            .unwrap();
        let uart = g.add_bus(Bus::Uart {
            id: id("uart1", 2),
            peripheral: "UART1".into(),
            tx,
            rx,
            used_by: None,
        });
        let sda = g
            .claim(
                Pin(4),
                owner(OwnerKind::I2c, "i2c0", 50),
                50..80,
                PinUse::Bidirectional,
            )
            .unwrap();
        let scl = g
            .claim(
                Pin(5),
                owner(OwnerKind::I2c, "i2c0", 50),
                50..80,
                PinUse::Bidirectional,
            )
            .unwrap();
        let i2c = g.add_bus(Bus::I2c {
            id: id("i2c0", 50),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 100,
            used_by: None,
        });

        assert_eq!(uart, BusId(0));
        assert_eq!(i2c, BusId(1));
        assert_eq!(g.bus(uart).unwrap().id().raw, "uart1");
        assert_eq!(g.bus(i2c).unwrap().id().raw, "i2c0");
        assert!(g.bus(BusId(2)).is_none());
    }

    #[test]
    fn a_board_holds_components_buses_and_services() {
        let mut resources = ResourceGraph::new();
        let pin = led_claim(&mut resources, 18, "red-led", 10).unwrap();
        let board = Board {
            meta: meta(),
            resources,
            components: vec![Component::Led(Led {
                id: id("red-led", 11),
                pin,
                active_low: false,
            })],
            services: vec![Service::Wifi(WifiService {
                ssid: "net".into(),
                password: "pw".into(),
            })],
        };

        assert_eq!(board.components[0].id().rust_name(), "red_led");
        assert_eq!(board.resources.claim_of(Pin(18)).unwrap().span, 10..21);
        assert!(matches!(board.services[0], Service::Wifi(_)));
    }

    #[test]
    fn a_full_board_carries_every_component_its_bus_and_its_service() {
        let mut g = ResourceGraph::new();
        let red_led = g
            .claim(
                Pin(18),
                owner(OwnerKind::Led, "red-led", 10),
                10..21,
                PinUse::Output,
            )
            .unwrap();
        let btn = g
            .claim(
                Pin(9),
                owner(OwnerKind::Button, "btn", 30),
                30..41,
                PinUse::Input,
            )
            .unwrap();

        let sda = g
            .claim(
                Pin(4),
                owner(OwnerKind::I2c, "i2c0", 50),
                50..80,
                PinUse::Bidirectional,
            )
            .unwrap();
        let scl = g
            .claim(
                Pin(5),
                owner(OwnerKind::I2c, "i2c0", 50),
                50..80,
                PinUse::Bidirectional,
            )
            .unwrap();
        let i2c = g.add_bus(Bus::I2c {
            id: id("i2c0", 51),
            peripheral: "I2C0".into(),
            sda,
            scl,
            khz: 400,
            used_by: Some(id("oled", 100)),
        });

        let sclk = g
            .claim(
                Pin(8),
                owner(OwnerKind::Spi, "spi2", 120),
                120..150,
                PinUse::Bidirectional,
            )
            .unwrap();
        let mosi = g
            .claim(
                Pin(7),
                owner(OwnerKind::Spi, "spi2", 120),
                120..150,
                PinUse::Bidirectional,
            )
            .unwrap();
        let spi = g.add_bus(Bus::Spi {
            id: id("spi2", 121),
            peripheral: "SPI2".into(),
            sclk,
            mosi,
            miso: None,
            khz: 1000,
            mode: 3,
        });
        let cs = g
            .claim(
                Pin(10),
                owner(OwnerKind::SpiComponent, "display", 160),
                160..190,
                PinUse::Unspecified,
            )
            .unwrap();

        let tx = g
            .claim(
                Pin(20),
                owner(OwnerKind::Uart, "uart1", 200),
                200..230,
                PinUse::Unspecified,
            )
            .unwrap();
        let rx = g
            .claim(
                Pin(21),
                owner(OwnerKind::Uart, "uart1", 200),
                200..230,
                PinUse::Unspecified,
            )
            .unwrap();
        let uart = g.add_bus(Bus::Uart {
            id: id("uart1", 201),
            peripheral: "UART1".into(),
            tx,
            rx,
            used_by: Some(id("serial", 240)),
        });

        let board = Board {
            meta: meta(),
            resources: g,
            components: vec![
                Component::Led(Led {
                    id: id("red-led", 11),
                    pin: red_led,
                    active_low: true,
                }),
                Component::Button(Button {
                    id: id("btn", 31),
                    pin: btn,
                    pull_up: true,
                }),
                Component::I2c(I2cComponent {
                    id: id("oled", 100),
                    bus: i2c,
                    address: Some(0x3c),
                }),
                Component::Spi(SpiComponent {
                    id: id("display", 160),
                    bus: spi,
                    cs,
                    mode: 0,
                }),
                Component::Uart(UartComponent {
                    id: id("serial", 240),
                    port: uart,
                    baud: 115200,
                    framing: Framing {
                        parity: Parity::Even,
                        stop_bits: StopBits::Two,
                    },
                }),
            ],
            services: vec![Service::Wifi(WifiService {
                ssid: "net".into(),
                password: "password".into(),
            })],
        };

        assert_eq!(
            board
                .components
                .iter()
                .map(|c| c.id().rust_name())
                .collect::<Vec<_>>(),
            vec!["red_led", "btn", "oled", "display", "serial"]
        );
        assert!(matches!(
            &board.components[0],
            Component::Led(Led { pin, active_low: true, .. }) if pin.pin() == Pin(18)
        ));
        assert!(matches!(
            &board.components[1],
            Component::Button(Button { pull_up: true, .. })
        ));
        assert!(matches!(
            board.bus(i2c),
            Some(Bus::I2c { peripheral, sda, scl, khz: 400, .. })
                if peripheral == "I2C0" && sda.pin() == Pin(4) && scl.pin() == Pin(5)
        ));
        // a bus with no MISO is write-only, and its component keeps its own CS
        assert!(matches!(
            board.bus(spi),
            Some(Bus::Spi { peripheral, sclk, mosi, miso: None, mode: 3, .. })
                if peripheral == "SPI2" && sclk.pin() == Pin(8) && mosi.pin() == Pin(7)
        ));
        assert!(matches!(
            &board.components[3],
            Component::Spi(SpiComponent { cs, mode: 0, .. }) if cs.pin() == Pin(10)
        ));
        assert!(matches!(
            board.bus(uart),
            Some(Bus::Uart { peripheral, tx, rx, .. })
                if peripheral == "UART1" && tx.pin() == Pin(20) && rx.pin() == Pin(21)
        ));
        assert!(matches!(
            &board.components[4],
            Component::Uart(UartComponent { baud: 115200, framing, .. })
                if framing.parity == Parity::Even && framing.stop_bits == StopBits::Two
        ));
        assert_eq!(
            board.services,
            vec![Service::Wifi(WifiService {
                ssid: "net".into(),
                password: "password".into(),
            })]
        );
    }

    /// The six ownership cases the diagnostics corpus pins today, expressed as
    /// claims. The graph is responsible for detecting them and keeping both
    /// sides; the element wording ("pin conflict" vs "pin already used" vs
    /// "pin direction clash") stays with the lowering that knows the element,
    /// exactly as Phase 1 left attribute wording with the wrappers.
    #[test]
    fn the_diagnostics_corpus_conflicts_are_expressed_as_claims() {
        #[derive(Debug)]
        struct Case {
            name: &'static str,
            kind: ConflictKind,
            pin: u8,
            existing: (OwnerKind, &'static str),
            incoming: (OwnerKind, &'static str),
        }

        let cases = [
            Case {
                name: "i2c-vs-gpio",
                kind: ConflictKind::Pin,
                pin: 4,
                existing: (OwnerKind::Gpio, "g"),
                incoming: (OwnerKind::I2c, "i2c0"),
            },
            Case {
                name: "spi-vs-gpio",
                kind: ConflictKind::Pin,
                pin: 2,
                existing: (OwnerKind::Gpio, "g"),
                incoming: (OwnerKind::Spi, "spi2"),
            },
            Case {
                name: "uart-vs-i2c",
                kind: ConflictKind::Pin,
                pin: 20,
                existing: (OwnerKind::Uart, "uart1"),
                incoming: (OwnerKind::I2c, "i2c0"),
            },
            Case {
                name: "led-vs-button",
                kind: ConflictKind::Direction,
                pin: 18,
                existing: (OwnerKind::Led, "red_led"),
                incoming: (OwnerKind::Button, "btn"),
            },
            Case {
                name: "led-vs-led",
                kind: ConflictKind::Pin,
                pin: 18,
                existing: (OwnerKind::Led, "red_led"),
                incoming: (OwnerKind::Led, "other"),
            },
            Case {
                name: "spi-cs-vs-bus-pin",
                kind: ConflictKind::Pin,
                pin: 6,
                existing: (OwnerKind::Spi, "spi2"),
                incoming: (OwnerKind::SpiComponent, "dev"),
            },
        ];

        for case in cases {
            let mut g = ResourceGraph::new();
            let (kind, name) = case.existing;
            let need = match kind {
                OwnerKind::Led => PinUse::Output,
                OwnerKind::Button => PinUse::Input,
                OwnerKind::Gpio | OwnerKind::Uart => PinUse::Unspecified,
                _ => PinUse::Bidirectional,
            };
            g.claim(Pin(case.pin), owner(kind, name, 10), 10..40, need)
                .unwrap();

            let (kind, name) = case.incoming;
            let need = match kind {
                OwnerKind::Button => PinUse::Input,
                OwnerKind::SpiComponent => PinUse::Unspecified,
                OwnerKind::I2c | OwnerKind::Spi => PinUse::Bidirectional,
                _ => PinUse::Unspecified,
            };
            let conflict = g
                .claim(Pin(case.pin), owner(kind, name, 50), 50..80, need)
                .unwrap_err();

            assert_eq!(conflict.kind, case.kind, "{}", case.name);
            assert_eq!(conflict.pin.0, case.pin, "{}", case.name);
            assert_eq!(
                (
                    conflict.existing.owner.kind,
                    conflict.existing.owner.id.raw.as_str()
                ),
                case.existing,
                "{}",
                case.name
            );
            assert_eq!(
                (
                    conflict.incoming.owner.kind,
                    conflict.incoming.owner.id.raw.as_str()
                ),
                case.incoming,
                "{}",
                case.name
            );
        }
    }
}
