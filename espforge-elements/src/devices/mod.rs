//! Elements that live in `# devices`: drivers for a specific part.
//!
//! A device binds *down* to the `i2c`/`spi` peripheral it drives
//! (`bus="$i2c0"`) and, for its auxiliary control lines, to `<gpio>`
//! peripherals. It carries its own `address` or `cs`; it never names a bus pin
//! directly. A UART device has no emitter, so a recipe that declares one is
//! refused at lowering instead of reaching emission. The shared wording of its
//! diagnostics lives here, so a driver's lowering differs only in the data its
//! recipe carries.
//!
//! The drivers are blocking-only: a display or sensor declared under the
//! embassy runtime is refused with [`requires_blocking`] rather than producing
//! code that cannot compile.

pub mod pin;
pub mod recipe;

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use espforge_schema::{AttrKind, Element as ElementDef};
use std::ops::Range;

use crate::attrs::Issue;
use crate::element::{LowerCx, Reported};
use crate::model::{Direction, Ident};

/// The value span of one attribute on `node`, or the node itself.
pub(crate) fn span_of(node: &ParsedNode<'_>, attr: &str) -> Range<usize> {
    node.get_attr_spanned(attr)
        .map(|a| a.val_offset..a.val_offset + a.val.len())
        .unwrap_or_else(|| node_span(node))
}

/// A device id that cannot become a Rust identifier.
pub(crate) fn invalid_id(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
) -> Reported {
    cx.report(
        codes::DEVICE_INVALID_ID,
        node_span(node),
        "invalid device id",
        format!(
            "<{tag} id=\"{}\">: the id must be a valid Rust identifier \
             (letters, digits, `_`; not starting with a digit)",
            id.raw
        ),
    )
}

/// A schema-level problem (missing or invalid attribute).
///
/// The wording is the one the hand-written device elements used: the message
/// names the attribute as written, and quotes the values the schema allows.
pub(crate) fn invalid_element(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    element: &ElementDef,
    issue: Issue<'_>,
) -> Reported {
    let id = node.id;
    let message = match issue {
        Issue::Missing { attr } => {
            format!("<{tag} id=\"{id}\"> is missing `{attr}`")
        }
        Issue::Invalid { attr, value } => {
            format!(
                "<{tag} id=\"{id}\"> {attr}=\"{value}\" must be {}",
                expected(element, attr)
            )
        }
    };
    cx.report(
        codes::DEVICE_INVALID,
        node_span(node),
        format!("invalid {tag}"),
        message,
    )
}

/// The values an attribute accepts, in the wording the schema's own listing
/// uses: `true or false`, `128x64 or 128x32`, `one of 0, 90, 180, 270`.
fn expected(element: &ElementDef, attr: &str) -> String {
    match element
        .attrs
        .iter()
        .find(|declared| declared.name == attr)
        .map(|declared| &declared.kind)
    {
        Some(AttrKind::Bool) => "true or false".to_string(),
        Some(AttrKind::Choice(choices)) if choices.len() == 2 => {
            format!("{} or {}", choices[0], choices[1])
        }
        Some(AttrKind::Choice(choices)) => format!("one of {}", choices.join(", ")),
        _ => "a valid value".to_string(),
    }
}

/// An auxiliary pin reference to a `<gpio>` that does not exist.
pub(crate) fn unknown_gpio(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    attr: &str,
    name: &str,
) -> Reported {
    cx.report(
        codes::DEVICE_UNKNOWN_GPIO,
        span_of(node, attr),
        "unknown gpio",
        format!(
            "<{tag} id=\"{}\"> refers to `{name}`, which is not a <gpio> in `# peripherals`",
            id.raw
        ),
    )
}

/// A device auxiliary pin on a GPIO declared as the other direction.
pub(crate) fn wrong_direction(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    attr: &str,
    name: &str,
    wanted: Direction,
    found: Direction,
) -> Reported {
    let found = match found {
        Direction::Input => "input",
        Direction::Output => "output",
        Direction::Bidirectional => "bidirectional",
    };
    let wanted = match wanted {
        Direction::Input => "input",
        Direction::Output => "output",
        Direction::Bidirectional => "bidirectional",
    };
    cx.report(
        codes::DEVICE_WRONG_DIRECTION,
        span_of(node, attr),
        "wrong gpio direction",
        format!(
            "<{tag} id=\"{}\"> needs an {wanted} pin, but `{name}` has direction=\"{found}\"",
            id.raw
        ),
    )
}
