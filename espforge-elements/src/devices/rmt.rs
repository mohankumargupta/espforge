//! Raw RMT pulse channels: `<rmt_tx>` and `<rmt_rx>`.
//!
//! A channel is not a driver: it binds one `<rmt>` block, claims one GPIO, and
//! allocates one channel number from the block's direction-specific pool. It
//! emits an endpoint from `espforge-runtime`, never a `Driver` record.
//!
//! The two elements differ by direction, the three TX-only attributes, and the
//! pin's claimed use. Everything else is shared.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use espforge_schema::RustType;
use linkme::distributed_slice;
use quote::{format_ident, quote};

use crate::chips::rmt_caps;
use crate::devices::{invalid_element, invalid_id, span_of};
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, Section, is_rust_ident};
use crate::generated::{rmt_rx, rmt_tx};
use crate::model::{
    Board, Bus, Carrier, Device, Ident, Owner, OwnerKind, Pin, PinUse, RmtChannelDevice,
    RmtDirection, RmtLevel, RmtTransport, Runtime,
};
use crate::registry::ELEMENTS;
use crate::rmt::{allocate_rmt_channel, carrier_ticks};

/// The `<rmt_tx>` element.
pub struct RmtTx;

#[distributed_slice(ELEMENTS)]
static RMT_TX: &dyn Element = &RmtTx;

impl Element for RmtTx {
    fn tag(&self) -> &str {
        "rmt_tx"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        let tag = self.tag();
        let element = cx
            .schema()
            .element(tag)
            .expect("`<rmt_tx>` is in the bundled schema");
        refuse_deferred(cx, node, tag)?;
        let attrs = rmt_tx::Attrs::from_node(node)
            .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
        let idle_level = match attrs.idle_level.value {
            rmt_tx::IdleLevel::Low => RmtLevel::Low,
            rmt_tx::IdleLevel::High => RmtLevel::High,
        };
        lower_channel(
            node,
            cx,
            tag,
            RmtDirection::Tx,
            ChannelAttrs {
                bus: attrs.bus.name.to_string(),
                pin: attrs.pin.value,
                channel: attrs.channel.map(|channel| channel.value),
                clk_divider: attrs.clk_divider.value,
                idle_level,
                carrier_hz: attrs.carrier_hz.map(|carrier| carrier.value),
                duty: attrs.duty.value,
            },
        )
    }
}

/// The `<rmt_rx>` element.
pub struct RmtRx;

#[distributed_slice(ELEMENTS)]
static RMT_RX: &dyn Element = &RmtRx;

impl Element for RmtRx {
    fn tag(&self) -> &str {
        "rmt_rx"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'_>) -> Result<(), Reported> {
        let tag = self.tag();
        let element = cx
            .schema()
            .element(tag)
            .expect("`<rmt_rx>` is in the bundled schema");
        refuse_deferred(cx, node, tag)?;
        let attrs = rmt_rx::Attrs::from_node(node)
            .map_err(|issue| invalid_element(cx, node, tag, element, issue))?;
        lower_channel(
            node,
            cx,
            tag,
            RmtDirection::Rx,
            ChannelAttrs {
                bus: attrs.bus.name.to_string(),
                pin: attrs.pin.value,
                channel: attrs.channel.map(|channel| channel.value),
                clk_divider: attrs.clk_divider.value,
                idle_level: RmtLevel::Low,
                carrier_hz: None,
                duty: 50,
            },
        )
    }
}

/// The values the two generated attribute readers have in common.
struct ChannelAttrs {
    bus: String,
    pin: u8,
    channel: Option<u32>,
    clk_divider: u32,
    idle_level: RmtLevel,
    carrier_hz: Option<u32>,
    duty: u32,
}

fn lower_channel(
    node: &ParsedNode<'_>,
    cx: &mut LowerCx<'_>,
    tag: &str,
    direction: RmtDirection,
    attrs: ChannelAttrs,
) -> Result<(), Reported> {
    let id = cx.checked_ident(node, Section::Devices)?;
    if !is_rust_ident(&id.rust_name()) {
        return Err(invalid_id(cx, node, tag, &id));
    }
    let span = node_span(node);

    let bus = match cx.bus_by_name(&attrs.bus) {
        Some(bus) if matches!(cx.bus(bus), Some(Bus::Rmt { .. })) => bus,
        _ => {
            return Err(cx.report(
                codes::DEVICE_INVALID,
                span_of(node, "bus"),
                "unknown rmt bus",
                format!(
                    "<{tag} id=\"{}\"> refers to `{}`, which is not an <rmt> in `# peripherals`",
                    id.raw, attrs.bus
                ),
            ));
        }
    };

    let Some(caps) = rmt_caps(cx.chip()) else {
        let why = if crate::chips::has_rmt(cx.chip()) {
            format!(
                "espforge has no verified RMT channel table for {} yet",
                cx.chip()
            )
        } else {
            format!("{} has no RMT peripheral", cx.chip())
        };
        return Err(cx.report(
            codes::RMT_NOT_SUPPORTED,
            span,
            "RMT is not available on this chip",
            format!("<{tag} id=\"{}\">: {why}", id.raw),
        ));
    };
    let khz = match cx.bus(bus) {
        Some(Bus::Rmt { khz, .. }) => *khz,
        _ => unreachable!("the bus was checked to be an RMT block"),
    };

    let requested = match attrs.channel {
        Some(channel) if channel <= u32::from(u8::MAX) => Some(channel as u8),
        Some(channel) => {
            return Err(cx.report(
                codes::RMT_CHANNEL_CONFLICT,
                span_of(node, "channel"),
                "RMT channel unavailable",
                format!(
                    "<{tag} id=\"{}\"> channel {channel} does not exist on {}",
                    id.raw,
                    cx.chip()
                ),
            ));
        }
        None => None,
    };

    let owner = Owner::new(OwnerKind::Device(tag.to_string()), id.clone());
    let pin_use = match direction {
        RmtDirection::Tx => PinUse::Output,
        RmtDirection::Rx => PinUse::Input,
    };
    let pin = cx
        .claim_pin(Pin(attrs.pin), owner.clone(), span.clone(), pin_use)
        .map_err(|refusal| refusal.report(cx))?;
    let channel = allocate_rmt_channel(cx, &caps, direction, requested, owner, span)?;

    // The channel and pin are claimed before the remaining values are checked,
    // so a rejected element exercises the checkpoint rollback: the next
    // element may allocate the same channel.
    if !(1..=255).contains(&attrs.clk_divider) {
        return Err(cx.report(
            codes::DEVICE_INVALID,
            span_of(node, "clk_divider"),
            format!("invalid {tag}"),
            format!(
                "<{tag} id=\"{}\"> clk_divider=\"{}\" must be 1..=255",
                id.raw, attrs.clk_divider
            ),
        ));
    }
    let clk_divider = attrs.clk_divider as u8;

    let carrier = match attrs.carrier_hz {
        Some(hz) if direction == RmtDirection::Tx => {
            if hz == 0 {
                return Err(invalid_value(
                    cx,
                    node,
                    tag,
                    &id,
                    "carrier_hz",
                    "must be greater than 0",
                ));
            }
            if !(1..=100).contains(&attrs.duty) {
                return Err(invalid_value(cx, node, tag, &id, "duty", "must be 1..=100"));
            }
            let duty = attrs.duty as u8;
            if carrier_ticks(khz, clk_divider, hz, duty).is_none() {
                return Err(invalid_value(
                    cx,
                    node,
                    tag,
                    &id,
                    "carrier_hz",
                    "and duty do not fit the channel clock",
                ));
            }
            Some(Carrier { hz, duty })
        }
        _ => None,
    };

    let (endpoint, async_endpoint) = match direction {
        RmtDirection::Tx => (
            "espforge_runtime::BlockingRmtTxEndpoint",
            "espforge_runtime::AsyncRmtTxEndpoint",
        ),
        RmtDirection::Rx => (
            "espforge_runtime::BlockingRmtRxEndpoint",
            "espforge_runtime::AsyncRmtRxEndpoint",
        ),
    };
    let parse = |text: &str| {
        RustType::parse(text).unwrap_or_else(|error| {
            panic!("the runtime endpoint `{text}` is not a Rust type: {error}")
        })
    };

    cx.push_device(Device::Rmt(RmtChannelDevice {
        id,
        tag: tag.to_string(),
        transport: RmtTransport {
            bus,
            channel,
            direction,
            pin,
            clk_divider,
            idle_level: attrs.idle_level,
            carrier,
            endpoint: Some(parse(endpoint)),
            async_endpoint: Some(parse(async_endpoint)),
        },
    }));
    Ok(())
}

/// `init="deferred"` needs a factory that stores a half-consumed `Rmt`; v1
/// builds channels at boot, so the request is refused by name (`E0907`).
fn refuse_deferred(cx: &mut LowerCx<'_>, node: &ParsedNode<'_>, tag: &str) -> Result<(), Reported> {
    let Some(init) = node.get_attr("init") else {
        return Ok(());
    };
    if init.eq_ignore_ascii_case("deferred") {
        return Err(cx.report(
            codes::DEVICE_INVALID,
            span_of(node, "init"),
            format!("invalid {tag}"),
            format!(
                "<{tag} id=\"{}\"> is built at boot; remove `init=\"deferred\"`",
                node.id
            ),
        ));
    }
    Ok(())
}

fn invalid_value(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    attr: &str,
    expected: &str,
) -> Reported {
    cx.report(
        codes::DEVICE_INVALID,
        span_of(node, attr),
        format!("invalid {tag}"),
        format!("<{tag} id=\"{}\"> {attr} {expected}", id.raw),
    )
}

impl Lowered for RmtChannelDevice {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut EmitCx) {
        let name = format_ident!("{}", self.id.rust_name());
        let endpoint = crate::bus_emit::rmt_channel(cx, board, self);
        let asynchronous = board.meta.runtime == Runtime::Embassy;
        let ty = match (self.transport.direction, asynchronous) {
            (RmtDirection::Tx, false) => quote! { espforge_runtime::BlockingRmtTxEndpoint },
            (RmtDirection::Tx, true) => quote! { espforge_runtime::AsyncRmtTxEndpoint },
            (RmtDirection::Rx, false) => quote! { espforge_runtime::BlockingRmtRxEndpoint },
            (RmtDirection::Rx, true) => quote! { espforge_runtime::AsyncRmtRxEndpoint },
        };
        if asynchronous {
            cx.device_field(quote! { pub #name: espforge_runtime::AsyncMutex<#ty>, });
            cx.device_init(quote! { #name: espforge_runtime::AsyncMutex::new(#endpoint), });
        } else {
            cx.device_field(quote! { pub #name: #ty, });
            cx.device_init(quote! { #name: #endpoint, });
        }
    }
}
