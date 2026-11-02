//! Generic service lowering: one interpreter for every `service_recipe`.
//!
//! A `<http_client>` and a `<ntp>` differ only in data the schema already
//! carries: the network reference, the runtime feature, the constructor and the
//! values its config takes. [`ServiceRecipeElement`] reads that data and
//! follows the same steps the hand-written service elements did. It knows no
//! tag.
//!
//! The typed, schema-generated `Attrs` readers still own presence and kind:
//! [`crate::generated::validate`] runs first, so a missing or misspelled value
//! is refused with the same wording as before, and an unmapped kind cannot
//! reach emission.

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use espforge_schema::{AttrKind, ServiceField, ServiceRecipe};

use crate::attrs;
use crate::element::{Element, LowerCx, Reported, is_rust_ident};
use crate::generated;
use crate::model::{Ident, Runtime, Service};
use crate::services::{buffer_too_small, invalid_element, invalid_id, span_of, unknown_network};

/// The interpreter for one schema element that declares a `service_recipe`.
pub struct ServiceRecipeElement {
    element: &'static espforge_schema::Element,
}

impl ServiceRecipeElement {
    /// An interpreter over `element`, which must declare a `service_recipe`.
    pub fn new(element: &'static espforge_schema::Element) -> Self {
        Self { element }
    }

    /// The schema element this recipe describes.
    fn element(&self) -> &'static espforge_schema::Element {
        self.element
    }

    /// The element's recipe. A `ServiceRecipeElement` is only ever built for an
    /// element that has one.
    fn recipe(&self) -> &'static ServiceRecipe {
        self.element
            .service_recipe
            .as_ref()
            .expect("a ServiceRecipeElement is only built for an element with a service_recipe")
    }
}

impl Element for ServiceRecipeElement {
    fn tag(&self) -> &'static str {
        self.element().tag.as_str()
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let tag = self.tag();
        let element = self.element();
        let recipe = self.recipe();

        // 1. the schema's own readers: presence, kind and allowed values.
        match generated::validate(tag, node) {
            Some(Err(issue)) => return Err(invalid_element(cx, node, tag, issue)),
            Some(Ok(())) => {}
            None => panic!("`<{tag}>` has no generated `Attrs`"),
        }

        // 2. the id must become a Rust identifier.
        let id = cx.ident(node);
        if !is_rust_ident(&id.rust_name()) {
            return Err(invalid_id(cx, node, tag, &id));
        }

        // 3. a service rides on the async stack, so the embassy runtime.
        if recipe.requires_embassy && cx.runtime() != Runtime::Embassy {
            return Err(cx.report(
                codes::SERVICE_REQUIRES_EMBASSY,
                node_span(node),
                format!("{tag} requires embassy"),
                format!(
                    "<{tag} id=\"{}\"> needs the embassy runtime; change \
                     <runtime type=\"..\" /> to embassy",
                    id.raw
                ),
            ));
        }

        // 4. the network component, read as a `$` reference.
        let network_attr = recipe.network.as_str();
        let reference = attrs::reference(node, network_attr)
            .map_err(|issue| invalid_element(cx, node, tag, issue))?;
        let network = Ident::new(reference.name, reference.span.start..reference.span.end);
        if !cx.has_network_provider(network.as_str()) {
            return Err(unknown_network(cx, node, tag, &id, &network));
        }

        // 5. the config values, with the floors and windows the recipe states.
        let mut config_fields: Vec<(String, String)> = Vec::new();
        for field in &recipe.fields {
            let kind = kind_of(element, &field.name);
            let value = value_of(node, element, &field.name);

            if field.non_empty && value.is_empty() {
                return Err(cx.report(
                    codes::SERVICE_INVALID,
                    span_of(node, &field.name),
                    format!("invalid {tag} {}", field.name),
                    format!(
                        "<{tag} id=\"{}\"> {}=\"\" must not be empty",
                        id.raw, field.name
                    ),
                ));
            }
            if let Some(minimum) = field.min {
                if let Some(number) = number_of(&value) {
                    if number < minimum {
                        return Err(buffer_too_small(
                            cx,
                            node,
                            tag,
                            &id,
                            &field.name,
                            number,
                            minimum,
                        ));
                    }
                }
            }
            if let Some((low, high)) = field.range {
                if let Some(number) = number_of(&value) {
                    if !(low..=high).contains(&number) {
                        return Err(cx.report(
                            codes::SERVICE_INVALID,
                            span_of(node, &field.name),
                            format!("invalid {tag} {}", field.name),
                            format!(
                                "<{tag} id=\"{}\"> {}=\"{number}\" must be in {low}..={high}",
                                id.raw, field.name
                            ),
                        ));
                    }
                }
            }

            config_fields.push((field.name.clone(), expression(field, &value, kind)));
        }

        // 6. `insecure` turns certificate verification off: a warning, not an
        // error, because a development board is a legitimate use.
        if element
            .attrs
            .iter()
            .any(|attr| attr.name == "insecure" && matches!(attr.kind, AttrKind::Bool))
            && value_of(node, element, "insecure") == "true"
        {
            cx.warn(
                codes::SERVICE_INSECURE_WARNING,
                span_of(node, "insecure"),
                "insecure TLS verification enabled",
                "server certificates will not be validated",
            );
        }

        // 7. the arguments after the config, in recipe order.
        let extra_args = recipe
            .args
            .iter()
            .map(|name| literal(&value_of(node, element, name), kind_of(element, name)))
            .collect();

        cx.push_service(Service {
            id,
            tag,
            network,
            feature: recipe.feature.clone(),
            ty: recipe.ty.clone(),
            new: recipe.new.clone(),
            config: recipe.config.clone(),
            heap_bytes: recipe.heap_bytes,
            config_fields,
            extra_args,
        });
        Ok(())
    }
}

/// The value written for `name`, or the default the schema gives it.
fn value_of(
    node: &ParsedNode<'_>,
    element: &'static espforge_schema::Element,
    name: &str,
) -> String {
    node.get_attr(name)
        .map(str::to_string)
        .or_else(|| {
            element
                .attrs
                .iter()
                .find(|attr| attr.name == name)
                .and_then(|attr| attr.default.clone())
        })
        .unwrap_or_default()
}

/// The kind the schema declares for `name`.
fn kind_of(element: &'static espforge_schema::Element, name: &str) -> &'static AttrKind {
    &element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .unwrap_or_else(|| panic!("`<{}>` has no attribute `{name}`", element.tag))
        .kind
}

/// The Rust expression an attribute's value is written as: a string is quoted,
/// a `u32` keeps its type (so a cast reads like the hand-written emitters did),
/// and a flag or a `u8` is written as it is.
fn literal(value: &str, kind: &AttrKind) -> String {
    match kind {
        AttrKind::Str => format!("{value:?}"),
        AttrKind::U32 => format!("{value}u32"),
        _ => value.to_string(),
    }
}

/// The config expression a field emits.
///
/// The field's `expr` is a Rust expression template in which the attribute's
/// own name stands for its literal: `"timeout_ms as u64"` becomes
/// `15000u32 as u64`. Without a template, the literal is the whole expression.
fn expression(field: &ServiceField, value: &str, kind: &AttrKind) -> String {
    let literal = literal(value, kind);
    match &field.expr {
        Some(expr) => expr.replace(&field.name, &literal),
        None => literal,
    }
}

/// A numeric attribute value, if it is one.
fn number_of(value: &str) -> Option<u32> {
    value.parse().ok()
}
