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
use espforge_schema::{AttrKind, RustExpr, RustPath, RustType, ServiceExprTemplate, ServiceField};

use crate::attrs;
use crate::element::{Element, LowerCx, Reported, Section, is_rust_ident};
use crate::generated;
use crate::model::{Ident, ProtocolService, Runtime, Service};
use crate::services::{buffer_too_small, invalid_element, invalid_id, span_of, unknown_network};

/// The interpreter for one schema element that declares a `service_recipe`.
///
/// It stores the tag, not the element: the element belongs to the session that
/// composed the schema and is looked up through the lowering context, so two
/// sessions can own two different definitions of the same tag.
pub struct ServiceRecipeElement {
    tag: String,
}

impl ServiceRecipeElement {
    /// An interpreter over the element `tag`, which must declare a
    /// `service_recipe`.
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Element for ServiceRecipeElement {
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
            .service_recipe
            .as_ref()
            .expect("a ServiceRecipeElement is only built for an element with a service_recipe");

        // 1. the schema's own readers: presence, kind and allowed values.
        match generated::validate(tag, node) {
            Some(Err(issue)) => return Err(invalid_element(cx, node, tag, issue)),
            Some(Ok(())) => {}
            None => panic!("`<{tag}>` has no generated `Attrs`"),
        }

        // 2. the id must become a Rust identifier.
        let id = cx.checked_ident(node, Section::Services)?;
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

        // 4. the network service, read as a `$` reference.
        let network_attr = recipe.network.as_str();
        let reference = attrs::reference(node, network_attr)
            .map_err(|issue| invalid_element(cx, node, tag, issue))?;
        let network = Ident::new(reference.name, reference.span.start..reference.span.end);
        let interfaces = interfaces_of(element, network_attr);
        if !interfaces
            .iter()
            .any(|interface| cx.has_provider(interface, network.as_str()))
        {
            return Err(unknown_network(cx, node, tag, &id, &network));
        }

        // 5. the config values, with the floors and windows the recipe states.
        let mut config_fields: Vec<(String, RustExpr)> = Vec::new();
        let mut const_field: Option<(String, String)> = None;
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

            // A `const` field is not a config member: it parameterizes the
            // recipe's Rust types, which step 7 does once all values are known.
            if field.is_const {
                const_field = Some((field.name.clone(), value.clone()));
                continue;
            }

            config_fields.push((
                field.name.clone(),
                expression(cx, node, tag, &id, field, &value, kind)?,
            ));
        }

        // The const argument is left out when it holds the attribute's declared
        // default, so a project that never set it generates the bare types it
        // always did (the Rust type's own default is the same value).
        let const_arg = const_field
            .filter(|(name, value)| *value != default_of(element, name))
            .map(|(_, value)| value);

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

        let ty = rust_type(
            cx,
            node,
            tag,
            &id,
            "the field type",
            &generic(&recipe.ty, const_arg.as_deref()),
        )?;
        let new = rust_path(
            cx,
            node,
            tag,
            &id,
            "the constructor",
            &generic_path(&recipe.new, const_arg.as_deref()),
        )?;
        let storage_text = recipe
            .storage
            .as_deref()
            .map(|text| generic(text, const_arg.as_deref()));
        let storage = optional_type(
            cx,
            node,
            tag,
            &id,
            "the storage type",
            storage_text.as_deref(),
        )?;
        let config = optional_type(
            cx,
            node,
            tag,
            &id,
            "the config type",
            recipe.config.as_deref(),
        )?;
        cx.push_service(Service::Protocol(ProtocolService {
            id,
            tag: tag.to_string(),
            network,
            feature: recipe.feature.clone(),
            ty,
            new,
            storage,
            config,
            heap_bytes: recipe.heap_bytes,
            config_fields,
            extra_args,
        }));
        Ok(())
    }
}

/// The interfaces the `network` attribute's `to=` names.
///
/// A service element's reference attribute says which interface it rides on
/// (`to="network"`), not which tag provides it: the lowering context answers by
/// the provider's own `provides` declaration, so a second provider needs no
/// change here.
fn interfaces_of<'a>(element: &'a espforge_schema::Element, attr: &str) -> &'a [String] {
    element
        .attrs
        .iter()
        .find(|declared| declared.name == attr)
        .and_then(|declared| match &declared.kind {
            AttrKind::Ref { to } => Some(to.as_slice()),
            _ => None,
        })
        .unwrap_or(&[])
}

/// The default the schema gives `name`.
///
/// A `const` field is compared against it: the value is emitted only when the
/// project sets something else.
fn default_of(element: &espforge_schema::Element, name: &str) -> String {
    element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .and_then(|attr| attr.default.clone())
        .unwrap_or_default()
}

/// `espforge_runtime::HttpClientStorage` plus the const argument, as the type it
/// becomes: `espforge_runtime::HttpClientStorage::<2>`.
///
/// The turbofish spelling parses as a type and still reads as an expression, so
/// emission's `#ty::new()` is valid for the generic and the bare form alike.
fn generic(path: &str, argument: Option<&str>) -> String {
    match argument {
        Some(value) => format!("{path}::<{value}>"),
        None => path.to_string(),
    }
}

/// A constructor path with the const argument before its last segment:
/// `espforge_runtime::HttpClient::<2>::new_in`.
fn generic_path(path: &str, argument: Option<&str>) -> String {
    let Some(value) = argument else {
        return path.to_string();
    };
    match path.rsplit_once("::") {
        Some((head, last)) => format!("{head}::<{value}>::{last}"),
        None => format!("{path}::<{value}>"),
    }
}

/// The value written for `name`, or the default the schema gives it.
fn value_of(node: &ParsedNode<'_>, element: &espforge_schema::Element, name: &str) -> String {
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
fn kind_of<'a>(element: &'a espforge_schema::Element, name: &str) -> &'a AttrKind {
    &element
        .attrs
        .iter()
        .find(|attr| attr.name == name)
        .unwrap_or_else(|| panic!("`<{}>` has no attribute `{name}`", element.tag))
        .kind
}

/// The Rust expression an attribute's value is written as: a string is a
/// quoted literal, a `u32` keeps its type (so a cast reads like the
/// hand-written emitters did), and a flag or a `u8` is a literal.
fn literal(value: &str, kind: &AttrKind) -> RustExpr {
    match kind {
        AttrKind::Str => RustExpr::string(value),
        AttrKind::U32 => {
            RustExpr::parse(&format!("{value}u32")).expect("a u32 attribute is a Rust literal")
        }
        _ => RustExpr::parse(value).expect("the schema validated the attribute value"),
    }
}

/// The config expression a field emits.
///
/// The field's `as=` is a Rust expression template in which the attribute's
/// own name stands for its literal: `"timeout_ms as u64"` becomes
/// `15000u32 as u64`. The template is parsed and the one path node replaced,
/// so a name that prefixes another (`port` in `timeout_port`) is not touched.
fn expression(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    field: &ServiceField,
    value: &str,
    kind: &AttrKind,
) -> Result<RustExpr, Reported> {
    let literal = literal(value, kind);
    match &field.expr {
        Some(text) => {
            let template = ServiceExprTemplate::parse(text, &field.name).map_err(|error| {
                cx.report(
                    codes::SERVICE_INVALID,
                    span_of(node, &field.name),
                    format!("invalid {tag} {}", field.name),
                    format!(
                        "<{tag} id=\"{}\"> as=\"{text}\" is not a Rust expression: {error}",
                        id.raw
                    ),
                )
            })?;
            Ok(template.fill(&literal))
        }
        None => Ok(literal),
    }
}

/// A Rust type the schema supplied (a field, storage or config type).
fn rust_type(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: &str,
) -> Result<RustType, Reported> {
    RustType::parse(text)
        .map_err(|error| invalid_syntax(cx, node, tag, id, what, &error.to_string()))
}

/// An optional Rust type the schema supplied.
fn optional_type(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: Option<&str>,
) -> Result<Option<RustType>, Reported> {
    match text {
        Some(text) => rust_type(cx, node, tag, id, what, text).map(Some),
        None => Ok(None),
    }
}

/// A Rust path the schema supplied (a constructor).
fn rust_path(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    text: &str,
) -> Result<RustPath, Reported> {
    RustPath::parse(text)
        .map_err(|error| invalid_syntax(cx, node, tag, id, what, &error.to_string()))
}

/// The located diagnostic for a service recipe the schema accepted but could
/// not be expressed as Rust. The schema validates these, so this is a safety
/// net, not a user-facing path.
fn invalid_syntax(
    cx: &mut LowerCx<'_>,
    node: &ParsedNode<'_>,
    tag: &str,
    id: &Ident,
    what: &str,
    message: &str,
) -> Reported {
    cx.report(
        codes::SERVICE_INVALID,
        node_span(node),
        format!("invalid {tag} recipe"),
        format!(
            "<{tag} id=\"{}\">: {what} is not Rust syntax: {message}",
            id.raw
        ),
    )
}

/// A numeric attribute value, if it is one.
fn number_of(value: &str) -> Option<u32> {
    value.parse().ok()
}
