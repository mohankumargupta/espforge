//! The recipe tier: one interpreter for every pin component.
//!
//! A `<led>` and a `<button>` differ on exactly four facts: the attribute that
//! names the `<gpio>`, the direction that gpio must declare, the Rust type
//! constructed, and (optionally) the code for "cannot be built". Everything
//! else is shared: the `$` reference, the unknown-gpio and wrong-direction
//! refusals, the identifier rule, the pin claim, and the emitted field.
//!
//! A schema element that carries a `recipe` child gives those four facts, so
//! this crate needs no Rust file for it: [`RecipeElement`] reads the recipe and
//! follows the same eight steps a hand-written element did. It knows no tag.
//!
//! The runtime type itself is not generated. `Led` and `Button` have materially
//! different bodies, so the recipe binds an existing type by path; an element
//! that needs new behaviour adds a `pub struct` to `espforge-runtime`, which is
//! a runtime site, not an element site.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::{DiagCode, codes};
use espforge_schema::{AttrDef, AttrKind, Direction as RecipeDirection, Recipe};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::attrs::{self, Issue};
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, is_rust_ident};
use crate::schema::schema;
use crate::model::{Board, ClaimedPin, Component, Direction, Ident, Owner, OwnerKind, Pin, PinUse};

/// The interpreter for one schema element that declares a `recipe`.
pub struct RecipeElement {
    element: &'static espforge_schema::Element,
}

impl RecipeElement {
    /// An interpreter over `element`, which must declare a recipe.
    pub fn new(element: &'static espforge_schema::Element) -> Self {
        Self { element }
    }

    /// The schema element this recipe describes.
    fn element(&self) -> &'static espforge_schema::Element {
        self.element
    }

    /// The element's recipe. A `RecipeElement` is only ever built for an
    /// element that has one.
    fn recipe(&self) -> &'static Recipe {
        self.element
            .recipe
            .as_ref()
            .expect("a RecipeElement is only built for an element with a recipe")
    }
}

impl Element for RecipeElement {
    fn tag(&self) -> &'static str {
        self.element().tag.as_str()
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let recipe = self.recipe();
        let tag = self.tag();
        let code = recipe.invalid;
        let pin_attr = recipe.pin.as_str();

        // 1. the id, with the span of its own attribute.
        let id = cx.ident(node);

        // 2. the pin attribute, read as a `$` reference.
        let reference = attrs::reference(node, pin_attr)
            .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?;

        // 3. the referenced `<gpio>`.
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
        let wanted = match recipe.direction {
            RecipeDirection::Input => Direction::Input,
            RecipeDirection::Output => Direction::Output,
        };
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
                Pin(gpio.pin),
                Owner::new(OwnerKind::Recipe(tag), id.clone()),
                node_span(node),
                pin_use,
            )
            .map_err(|conflict| cx.report_conflict(&conflict))?;

        // 7. every remaining `bool` attribute, in schema order.
        let mut flags = Vec::new();
        for attr in &self.element.attrs {
            if attr.name.as_str() == pin_attr {
                continue;
            }
            if matches!(attr.kind, AttrKind::Bool) {
                let value = read_bool(node, attr)
                    .map_err(|issue| invalid(cx, node, tag, pin_attr, code, issue))?;
                flags.push(value);
            }
        }

        // 8. the component.
        cx.push(Component::Recipe(RecipeComponent::new(tag, id, pin, flags)));
        Ok(())
    }
}

/// One lowered recipe component over a claimed pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeComponent {
    pub id: Ident,
    /// The schema tag, which names the recipe that built this.
    pub tag: &'static str,
    pub pin: ClaimedPin,
    /// The Rust type, resolved from the schema.
    pub runtime: &'static str,
    /// The `bool` attributes, in schema order.
    pub flags: Vec<bool>,
}

impl RecipeComponent {
    /// Build a component for `tag` from the schema's own recipe.
    ///
    /// There is one constructor, so a hand-built fixture and the lowering
    /// cannot disagree about the runtime type.
    pub fn new(tag: &'static str, id: Ident, pin: ClaimedPin, flags: Vec<bool>) -> Self {
        let runtime = recipe_of(tag).runtime.as_str();
        Self {
            id,
            tag,
            pin,
            runtime,
            flags,
        }
    }
}

impl Lowered for RecipeComponent {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, _board: &Board, cx: &mut EmitCx) {
        let name = format_ident!("{}", self.id.rust_name());
        let gpio = format_ident!("GPIO{}", self.pin.pin().0);
        let runtime: TokenStream = self
            .runtime
            .parse()
            .expect("a recipe runtime path is a Rust path");
        let flags = &self.flags;
        cx.field(quote! { pub #name: #runtime, });
        cx.init(quote! {
            #name: #runtime::new(
                unsafe { registry.peripherals.#gpio.clone_unchecked() },
                #(#flags),*,
            ),
        });
        cx.safety_comment(
            format!("{name}: {}::new(", self.runtime),
            format!(
                "SAFETY: espforge checked that no other component uses GPIO{}.",
                self.pin.pin().0
            ),
        );
    }

    fn overlay_attr(&self, attr: &str) -> Option<String> {
        match attr {
            "pin" => Some(self.pin.pin().0.to_string()),
            _ => None,
        }
    }
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

/// One `bool` attribute, read with its schema default.
fn read_bool<'a>(node: &'a ParsedNode<'a>, attr: &'static AttrDef) -> Result<bool, Issue<'a>> {
    let name = attr.name.as_str();
    let value = if attr.required {
        attrs::flag(node, name)
    } else {
        let default = attr.default.as_deref() == Some("true");
        attrs::flag_or(node, name, default)
    };
    value.map(|spanned| spanned.value)
}

/// `output` / `input`, the wording both directions use.
fn word(direction: Direction) -> &'static str {
    match direction {
        Direction::Output => "output",
        Direction::Input => "input",
    }
}

/// The generic wording for a schema-level problem.
///
/// Only the number is shared: the message stays tag-derived, so a new element
/// still reads specifically. `led` and `button` keep their own legacy codes by
/// writing `invalid=`.
fn invalid(
    cx: &mut LowerCx,
    node: &ParsedNode<'_>,
    tag: &str,
    pin_attr: &str,
    code: DiagCode,
    issue: Issue<'_>,
) -> Reported {
    let id = node.id;
    let label = match issue {
        Issue::Missing { attr } if attr == pin_attr => {
            format!("<{tag} id=\"{id}\"> needs {pin_attr}=\"$<gpio id>\"")
        }
        Issue::Missing { attr } => format!("Missing required attribute '{attr}'"),
        Issue::Invalid { attr, value } => {
            format!("<{tag} id=\"{id}\"> {attr}=\"{value}\" must be \"true\" or \"false\"")
        }
    };
    cx.report(code, node_span(node), format!("invalid {tag}"), label)
}
