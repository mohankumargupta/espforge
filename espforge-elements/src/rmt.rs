//! Shared RMT channel allocation.
//!
//! `<rmt_tx>`, `<rmt_rx>` and (later) a `bus "rmt"` driver all take a channel
//! from the one block. The allocator lives here so the numbering, the
//! direction rule and the claim name are written once.
//!
//! A channel is claimed by its number, never by `(number, direction)`: on
//! `esp32`/`esp32s2` a channel can do either direction, so a second claim for
//! the other direction must see the first.

use std::ops::Range;

use espforge_diagnostics::codes;

use crate::chips::RmtCaps;
use crate::element::{LowerCx, Reported};
use crate::model::{Owner, RmtDirection};

/// The claim name one channel uses (`RMT.channel0`).
pub(crate) fn channel_peripheral(channel: u8) -> String {
    format!("RMT.channel{channel}")
}

/// Allocate one channel in `direction`, or report `E0912`/`E0913`.
///
/// `requested` is honoured only when the direction can use it. Without one,
/// the lowest unclaimed channel in the direction's list wins. The claim is
/// made through [`LowerCx::claim_peripheral`], so the element's checkpoint
/// rolls it back if a later step fails.
pub(crate) fn allocate_rmt_channel(
    cx: &mut LowerCx<'_>,
    caps: &RmtCaps,
    direction: RmtDirection,
    requested: Option<u8>,
    owner: Owner,
    span: Range<usize>,
) -> Result<u8, Reported> {
    let channels = direction.channels(caps);
    let chip = cx.chip().to_string();
    let what = match direction {
        RmtDirection::Tx => "TX",
        RmtDirection::Rx => "RX",
    };
    let list = || {
        channels
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let owner_name = format!("<{} id=\"{}\">", owner.kind.tag(), owner.id.raw);

    if let Some(channel) = requested {
        if !channels.contains(&channel) {
            return Err(cx.report(
                codes::RMT_CHANNEL_CONFLICT,
                span,
                "RMT channel unavailable",
                format!(
                    "{owner_name}: channel {channel} is not a {what} channel on {chip}; \
                     {what} channels are {}",
                    list()
                ),
            ));
        }
        let peripheral = channel_peripheral(channel);
        if cx.peripheral_claimed(&peripheral) {
            return Err(cx.report(
                codes::RMT_CHANNEL_CONFLICT,
                span,
                "RMT channel unavailable",
                format!("{owner_name}: channel {channel} is already in use"),
            ));
        }
        cx.claim_peripheral(peripheral, owner, span)?;
        return Ok(channel);
    }

    for &channel in channels {
        if !cx.peripheral_claimed(&channel_peripheral(channel)) {
            cx.claim_peripheral(channel_peripheral(channel), owner, span)?;
            return Ok(channel);
        }
    }

    Err(cx.report(
        codes::RMT_CHANNEL_UNAVAILABLE,
        span,
        "no RMT channel left",
        format!(
            "{owner_name}: all {what} channels are in use on {chip} ({})",
            list()
        ),
    ))
}

/// The carrier high/low ticks `esp-hal` wants, or `None` when the request
/// cannot be represented.
///
/// `source_khz` is the block's source clock and `clk_divider` the channel
/// divider; the carrier period is counted in divided-clock ticks. Both phases
/// must be at least one tick and fit the 15-bit `u16` field.
pub(crate) fn carrier_ticks(
    source_khz: u32,
    clk_divider: u8,
    carrier_hz: u32,
    duty: u8,
) -> Option<(u16, u16)> {
    if carrier_hz == 0 || !(1..=100).contains(&duty) || clk_divider == 0 {
        return None;
    }
    let source_hz = source_khz.checked_mul(1_000)? / u32::from(clk_divider);
    let period = source_hz / carrier_hz;
    if period == 0 {
        return None;
    }
    let high = period * u32::from(duty) / 100;
    let low = period - high;
    if high == 0 || low == 0 || high > u32::from(u16::MAX) || low > u32::from(u16::MAX) {
        return None;
    }
    Some((high as u16, low as u16))
}
