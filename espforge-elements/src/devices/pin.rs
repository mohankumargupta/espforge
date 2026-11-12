//! The pin-device tier: one interpreter for every pin recipe.
//!
//! A `<led>` and a `<button>` differ on exactly four facts: the attribute that
//! names the `<gpio>`, the direction that gpio must declare, the Rust type
//! constructed, and (optionally) the code for "cannot be built". Everything
//! else is shared: the `$` reference, the unknown-gpio and wrong-direction
//! refusals, the identifier rule, the pin claim, and the emitted field.
//!
//! A schema element that carries a `recipe` child gives those four facts, so
//! this crate needs no Rust file for it: [`PinRecipeElement`] reads the recipe
//! and follows the same eight steps a hand-written element did. It knows no
//! tag.
//!
//! The runtime type itself is not generated. `Led` and `Button` have materially
//! different bodies, so the recipe binds an existing type by path; an element
//! that needs new behaviour adds a `pub struct` to `espforge-runtime`, which is
//! a runtime site, not an element site.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::{DiagCode, codes};
use espforge_schema::{AttrDef, Direction as RecipeDirection, Recipe, RustPath};
use quote::{format_ident, quote};

use crate::attrs::{self, Issue};
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, Section, is_rust_ident};
use crate::model::{
    Board, ClaimedPin, Device, Direction, Ident, Owner, OwnerKind, Pin, PinDevice, PinDeviceKind,
    PinUse, Runtime,
};
use crate::schema::schema;

/// The interpreter for one schema element that declares a `recipe`.
///
/// It stores the tag, not the element: the element belongs to the session that
/// composed the schema and is looked up through the lowering context, so two
/// sessions can own two different definitions of the same tag.
pub struct PinRecipeElement {
    tag: String,
}

impl PinRecipeElement {
    /// An interpreter over the element `tag`, which must declare a recipe.
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Element for PinRecipeElement {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn lower<'s>(&self, node: &ParsedNode<'_>, cx: &mut LowerCx<'s>) -> Result<(), Reported> {
        let tag = self.tag.as_str();
        let element = cx
            .schema()
            .element(tag)
            .unwrap_or_else(|| panic!("`<{tag}>` is not in the schema"));
        let recipe = element
            .recipe
            .as_ref()
            .expect("a PinRecipeElement is only built for an element with a recipe");
        let code = recipe.invalid;
        let pin_attr = recipe.pin.as_str();

        // 1. the id, with the span of its own attribute.
        let id = cx.checked_ident(node, Section::Devices)?;

        // 2. the pin: a `pin="18"` number, or the legacy `$<gpio>` reference.
        // A number stands alone; a reference is also checked against the
        // `<gpio>`'s declared direction.
        let wanted = match recipe.direction {
            RecipeDirection::Input => Direction::Input,
            RecipeDirection::Output => Direction::Output,
        };
        let direct = node.get_attr("pin").is_some();
        let pin_number = if direct {
            attrs::pin(node, "pin")
                .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?
                .value
        } else {
            // 3. the referenced `<gpio>`.
            let reference = attrs::reference(node, pin_attr)
                .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?;
            let Some(gpio) = cx.gpio(reference.name).cloned() else {
                return Err(cx.report(
                    codes::COMPONENT_UNKNOWN_GPIO,
                    node_span(node),
                    "unknown gpio",
                    format!(
                        "<{tag} id=\"{}\"> refers to `{}`, which is not a <gpio> in `# peripherals`",
                        id.raw, reference.name
                    ),
                ));
            };

            // 4. a declared direction that is not the recipe's.
            if let Some(direction) = gpio.direction.filter(|direction| *direction != wanted) {
                return Err(cx.report(
                    codes::COMPONENT_WRONG_DIRECTION,
                    node_span(node),
                    "wrong gpio direction",
                    format!(
                        "<{tag} id=\"{}\"> needs an {} pin, but `{}` has direction=\"{}\"",
                        id.raw,
                        word(wanted),
                        gpio.id.raw,
                        word(direction),
                    ),
                ));
            }
            gpio.pin
        };

        // 5. the id must become a Rust identifier.
        if !is_rust_ident(&id.rust_name()) {
            return Err(cx.report(
                code,
                node_span(node),
                format!("invalid {tag}"),
                format!(
                    "<{tag} id=\"{}\">: the id must become a Rust identifier \
                     (letters, digits, `_`; not starting with a digit)",
                    id.raw
                ),
            ));
        }

        // 6. claim the pin.
        let pin_use = match recipe.direction {
            RecipeDirection::Input => PinUse::Input,
            RecipeDirection::Output => PinUse::Output,
        };
        let pin = cx
            .claim_pin(
                Pin(pin_number),
                Owner::new(OwnerKind::Pin(tag.to_string()), id.clone()),
                node_span(node),
                pin_use,
            )
            .map_err(|refusal| refusal.report(cx))?;

        // 7. the direction-specific boolean, read by name with its schema
        // default. A new pin recipe extends `PinDeviceKind`; do not go back to
        // an untyped vector of booleans.
        let kind = match recipe.direction {
            RecipeDirection::Output => PinDeviceKind::Output {
                active_low: bool_of(node, element, "active_low")
                    .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?,
            },
            RecipeDirection::Input => PinDeviceKind::Input {
                pull_up: bool_of(node, element, "pull_up")
                    .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?,
            },
        };

        // 8. the pin device.
        cx.push_device(Device::Pin(PinDevice {
            id,
            tag: tag.to_string(),
            pin,
            runtime: runtime_path(&recipe.runtime),
            kind,
        }));
        Ok(())
    }
}

impl PinDevice {
    /// Build a pin device for `tag` from the schema's own recipe.
    ///
    /// There is one constructor, so a hand-built fixture and the lowering
    /// cannot disagree about the runtime type. The boolean starts at the
    /// schema's default (`false`).
    pub fn new(tag: &str, id: Ident, pin: ClaimedPin) -> Self {
        let recipe = recipe_of(tag);
        let kind = match recipe.direction {
            RecipeDirection::Output => PinDeviceKind::Output { active_low: false },
            RecipeDirection::Input => PinDeviceKind::Input { pull_up: false },
        };
        Self {
            id,
            tag: tag.to_string(),
            pin,
            runtime: runtime_path(&recipe.runtime),
            kind,
        }
    }
}

impl Lowered for PinDevice {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, board: &Board, cx: &mut EmitCx) {
        let name = format_ident!("{}", self.id.rust_name());
        let gpio = format_ident!("GPIO{}", self.pin.pin().0);
        let runtime = &self.runtime;
        let flag = match self.kind {
            PinDeviceKind::Output { active_low } => active_low,
            PinDeviceKind::Input { pull_up } => pull_up,
        };
        let init = quote! {
            #runtime::new(
                unsafe { registry.peripherals.#gpio.clone_unchecked() },
                #flag,
            )
        };

        // One accessor for every device: on an embassy board the container is
        // an `AsyncMutex`, so `device!` can always call `.lock().await`.
        if board.meta.runtime == Runtime::Embassy {
            cx.field(quote! { pub #name: espforge_runtime::AsyncMutex<#runtime>, });
            cx.init(quote! {
                #name: espforge_runtime::AsyncMutex::new(
                    #init,
                ),
            });
        } else {
            cx.field(quote! { pub #name: #runtime, });
            cx.init(quote! { #name: #init, });
        }
    }

    fn overlay_attr(&self, attr: &str) -> Option<String> {
        match attr {
            "pin" => Some(self.pin.pin().0.to_string()),
            _ => None,
        }
    }
}

/// The runtime path one pin recipe names.
///
/// The schema validated it as a plain path at load; parsing it here is the one
/// parse a pin device does, and it fails only if the schema is broken.
fn runtime_path(path: &str) -> RustPath {
    RustPath::parse(path).expect("a recipe runtime path is a valid Rust path")
}

/// The recipe `tag` declares in `espf.schema.kdl`.
fn recipe_of(tag: &str) -> &'static Recipe {
    let element = schema()
        .element(tag)
        .unwrap_or_else(|| panic!("`<{tag}>` is not in espf.schema.kdl"));
    element
        .recipe
        .as_ref()
        .unwrap_or_else(|| panic!("`<{tag}>` has no recipe in espf.schema.kdl"))
}

/// One named `bool` attribute, read with its schema default.
fn bool_of<'a>(
    node: &'a ParsedNode<'a>,
    element: &'a espforge_schema::Element,
    name: &str,
) -> Result<bool, Issue<'a>> {
    let attr = element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .unwrap_or_else(|| panic!("`<{}>` has no attribute `{name}`", element.tag));
    read_bool(node, attr)
}

/// One `bool` attribute, read with its schema default.
fn read_bool<'a>(node: &'a ParsedNode<'a>, attr: &'a AttrDef) -> Result<bool, Issue<'a>> {
    let name = attr.name.as_str();
    let value = if attr.required {
        attrs::flag(node, name)
    } else {
        let default = attr.default.as_deref() == Some("true");
        attrs::flag_or(node, name, default)
    };
    value.map(|spanned| spanned.value)
}

/// `output` / `input` / `bidirectional`, the wording both sides use.
fn word(direction: Direction) -> &'static str {
    match direction {
        Direction::Output => "output",
        Direction::Input => "input",
        Direction::Bidirectional => "bidirectional",
    }
}

/// The generic wording for a schema-level problem.
///
/// Only the number is shared: the message stays tag-derived, so a new element
/// still reads specifically. `led` and `button` keep their own legacy codes by
/// writing `invalid=`.
fn invalid(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    pin_attr: &str,
    code: DiagCode,
    issue: Issue<'_>,
) -> Reported {
    let id = node.id;
    let label = match issue {
        Issue::Missing { attr } if attr == pin_attr => {
            format!("<{tag} id=\"{id}\"> needs pin=\"<number>\" or {pin_attr}=\"$<gpio id>\"")
        }
        Issue::Missing { attr } => format!("Missing required attribute '{attr}'"),
        Issue::Invalid { attr, value } => {
            format!("<{tag} id=\"{id}\"> {attr}=\"{value}\" must be \"true\" or \"false\"")
        }
    };
    cx.report(code, node_span(node), format!("invalid {tag}"), label)
}
