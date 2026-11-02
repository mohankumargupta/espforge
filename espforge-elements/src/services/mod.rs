//! Elements that live in `# services`: software over an interface.
//!
//! A service claims no pin. It refers *down* to a component (or device) that
//! provides the interface it rides on — today a `<wifi_station>` and its
//! embassy-net stack.
//!
//! A service is data as much as a component or a device is: its type, its
//! constructor, its config fields and its feature all come from a
//! `service_recipe` in `services.schema.kdl`, so adding one edits the runtime
//! and the schema, never this crate. [`recipe`] is the interpreter; the wording of
//! the diagnostics it can report lives here, so every service says "unknown
//! network" the same way.

pub mod recipe;

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::attrs::Issue;
use crate::element::{LowerCx, Reported};
use crate::model::Ident;

/// `components.<network>.stack()` — the embassy-net stack a service's
/// `network=` reference names. The component that carries the stack exposes it
/// with a `stack()` method; `<wifi_station>` is the one today.
pub(crate) fn network_stack(network: &Ident) -> TokenStream {
    let name = format_ident!("{}", network.rust_name());
    quote! { components.#name.stack() }
}

/// The value span of one attribute on `node`, or the node itself.
pub(crate) fn span_of(node: &ParsedNode<'_>, attr: &str) -> std::ops::Range<usize> {
    node.get_attr_spanned(attr)
        .map(|a| a.val_offset..a.val_offset + a.val.len())
        .unwrap_or_else(|| node_span(node))
}

/// An id that cannot become a Rust identifier.
pub(crate) fn invalid_id(
    cx: &mut LowerCx,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
) -> Reported {
    cx.report(
        codes::SERVICE_INVALID,
        node_span(node),
        format!("invalid {tag}"),
        format!(
            "<{tag} id=\"{}\">: the id must become a Rust identifier \
             (letters, digits, `_`; not starting with a digit)",
            id.raw
        ),
    )
}

/// A schema-level problem (missing or invalid attribute).
pub(crate) fn invalid_element(
    cx: &mut LowerCx,
    node: &ParsedNode<'_>,
    tag: &str,
    issue: Issue<'_>,
) -> Reported {
    let id = node.id;
    let message = match issue {
        Issue::Missing { attr } => {
            format!("<{tag} id=\"{id}\"> needs {attr}=\"$<network component id>\"")
        }
        Issue::Invalid { attr, value } => {
            format!("<{tag} id=\"{id}\"> has an invalid {attr}=\"{value}\"")
        }
    };
    cx.report(
        codes::SERVICE_INVALID,
        node_span(node),
        format!("invalid {tag}"),
        message,
    )
}

/// A `network=` reference that names no provider component.
pub(crate) fn unknown_network(
    cx: &mut LowerCx,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    network: &Ident,
) -> Reported {
    cx.report(
        codes::SERVICE_UNKNOWN_NETWORK,
        node_span(node),
        "unknown network interface",
        format!(
            "<{tag} id=\"{}\"> refers to `{}`, which is not a network component in `# components`",
            id.raw, network.raw
        ),
    )
}

/// A buffer below the minimum the protocol needs.
pub(crate) fn buffer_too_small(
    cx: &mut LowerCx,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    attr: &str,
    value: u32,
    minimum: u32,
) -> Reported {
    cx.report(
        codes::SERVICE_BUFFER_TOO_SMALL,
        node_span(node),
        "buffer too small",
        format!(
            "<{tag} id=\"{}\"> {attr}=\"{value}\" is below the minimum of {minimum} bytes",
            id.raw
        ),
    )
}
