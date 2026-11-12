//! Typed Rust fragments for the board IR.
//!
//! A manifest's `type`, `expr` and `endpoint` are Rust syntax. The schema
//! validates them at load (the schema model keeps the text so it stays
//! `Send + Sync`: `syn` nodes are neither, and the schema is a process-wide
//! `static`); lowering parses each one exactly once into these wrappers, fills
//! a template's `{slot}` holes with parsed values, and stores the concrete
//! nodes on the board. Emission quotes those nodes and never parses a manifest
//! string again.
//!
//! The wrappers exist because `syn` nodes are not `PartialEq` and the board
//! derives equality. They compare by token text, which is enough for a `Board`
//! equality check and does not need `syn`'s `extra-traits` feature.

use std::collections::BTreeMap;

use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::visit_mut::{self, VisitMut};
use syn::{Expr, ExprCall, Path, Type};

/// The prefix a placeholder identifier carries in a parsed template:
/// `Ssd1306Display<{size}>` becomes `Ssd1306Display<__espforge_size>`.
const SENTINEL: &str = "__espforge_";

macro_rules! token_equality {
    ($($wrapper:ident),+ $(,)?) => {
        $(
            impl PartialEq for $wrapper {
                fn eq(&self, other: &Self) -> bool {
                    self.text() == other.text()
                }
            }

            impl Eq for $wrapper {}

            // `syn` nodes implement `Debug` only with `extra-traits`; the token
            // text is the readable form anyway.
            impl std::fmt::Debug for $wrapper {
                fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    formatter.write_str(&self.text())
                }
            }
        )+
    };
}

/// A Rust type expression that has already been parsed.
#[derive(Clone)]
pub struct RustType(Box<Type>);

/// A Rust expression that has already been parsed.
#[derive(Clone)]
pub struct RustExpr(Box<Expr>);

/// A Rust path that has already been parsed.
#[derive(Clone)]
pub struct RustPath(Box<Path>);

token_equality!(RustType, RustExpr, RustPath);

impl RustType {
    /// Parse `text` as a Rust type.
    pub fn parse(text: &str) -> syn::Result<Self> {
        syn::parse_str(text).map(|node| Self(Box::new(node)))
    }

    /// The parsed node.
    pub fn as_syn(&self) -> &Type {
        self.0.as_ref()
    }

    pub(crate) fn from_syn(node: Type) -> Self {
        Self(Box::new(node))
    }

    /// The token text, with `::` kept tight so a diagnostic reads naturally.
    pub fn text(&self) -> String {
        compact(&self.0.to_token_stream())
    }
}

impl RustExpr {
    /// Parse `text` as a Rust expression.
    pub fn parse(text: &str) -> syn::Result<Self> {
        syn::parse_str(text).map(|node| Self(Box::new(node)))
    }

    /// The parsed node.
    pub fn as_syn(&self) -> &Expr {
        self.0.as_ref()
    }

    pub(crate) fn from_syn(node: Expr) -> Self {
        Self(Box::new(node))
    }

    /// The token text, with `::` kept tight so a diagnostic reads naturally.
    pub fn text(&self) -> String {
        compact(&self.0.to_token_stream())
    }

    /// `Some(<inner>)`, built as an AST rather than spliced as text.
    pub fn some(inner: &RustExpr) -> Self {
        Self(Box::new(Expr::Call(ExprCall {
            attrs: Vec::new(),
            func: Box::new(syn::parse_quote!(Some)),
            paren_token: Default::default(),
            args: std::iter::once(inner.as_syn().clone()).collect(),
        })))
    }

    /// `None`, the value an absent optional pin carries.
    pub fn none() -> Self {
        Self(Box::new(syn::parse_quote!(None)))
    }

    /// An integer literal with no suffix (`60`).
    pub fn number(value: u64) -> Self {
        Self(Box::new(Expr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn::Lit::Int(syn::LitInt::new(&value.to_string(), Span::call_site())),
        })))
    }

    /// A string literal, with the escaping a literal needs.
    pub fn string(value: &str) -> Self {
        Self(Box::new(Expr::Lit(syn::ExprLit {
            attrs: Vec::new(),
            lit: syn::Lit::Str(syn::LitStr::new(value, Span::call_site())),
        })))
    }
}

impl RustPath {
    /// Parse `text` as a Rust path.
    pub fn parse(text: &str) -> syn::Result<Self> {
        syn::parse_str(text).map(|node| Self(Box::new(node)))
    }

    /// The parsed node.
    pub fn as_syn(&self) -> &Path {
        self.0.as_ref()
    }

    /// The token text, with `::` kept tight: `espforge_drivers::Ssd1306`.
    pub fn text(&self) -> String {
        compact(&self.0.to_token_stream())
    }
}

impl ToTokens for RustType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.as_ref().to_tokens(tokens);
    }
}

impl ToTokens for RustExpr {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.as_ref().to_tokens(tokens);
    }
}

impl ToTokens for RustPath {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.as_ref().to_tokens(tokens);
    }
}

/// Token text with `::` kept tight.
fn compact(tokens: &TokenStream) -> String {
    tokens.to_string().replace(" :: ", "::")
}

/// `Ssd1306Display<{size}>` -> a `syn::Type` whose generic argument is the
/// sentinel path `__espforge_size`, plus the slots it used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeTemplate {
    /// The parsed template.
    pub syntax: RustType,
    /// The placeholder names it used, in order.
    pub slots: Vec<String>,
}

impl TypeTemplate {
    /// Parse a template whose only holes are `{slot}` names.
    pub fn parse(text: &str) -> Result<Self, String> {
        let (substituted, slots) = substitute(text).map_err(|(message, _)| message)?;
        let syntax = RustType::parse(&substituted).map_err(|error| error.to_string())?;
        Ok(Self { syntax, slots })
    }

    /// The concrete type, with every placeholder replaced by its value.
    ///
    /// The schema already checked the placeholder list, so a missing value is
    /// an internal error rather than a diagnostic.
    pub fn fill(&self, types: &BTreeMap<String, RustType>) -> RustType {
        struct Filler<'a> {
            types: &'a BTreeMap<String, RustType>,
        }

        impl VisitMut for Filler<'_> {
            fn visit_type_mut(&mut self, node: &mut Type) {
                if let Type::Path(path) = node
                    && path.qself.is_none()
                    && let Some(slot) = sentinel_slot(&path.path)
                    && let Some(replacement) = self.types.get(&slot)
                {
                    *node = replacement.as_syn().clone();
                    return;
                }
                visit_mut::visit_type_mut(self, node);
            }
        }

        let mut filled = self.syntax.as_syn().clone();
        Filler { types }.visit_type_mut(&mut filled);
        RustType::from_syn(filled)
    }

    /// Whether the template reads `slot`.
    pub fn uses(&self, slot: &str) -> bool {
        self.slots.iter().any(|known| known == slot)
    }
}

/// One mapped attribute value: a parsed type, a parsed expression, or both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappedValue {
    /// Present when the value is used in a type slot.
    pub ty: Option<RustType>,
    /// Present when the value is used in a constructor slot.
    pub expr: Option<RustExpr>,
}

impl MappedValue {
    /// Parse one map entry for the roles its recipe uses.
    ///
    /// A value used in a type template must parse as a type; a value used as a
    /// constructor argument must parse as an expression.
    pub fn parse(text: &str, needs_type: bool, needs_expr: bool) -> Result<Self, String> {
        let ty = if needs_type {
            Some(
                RustType::parse(text)
                    .map_err(|error| format!("`{text}` is not a Rust type: {error}"))?,
            )
        } else {
            None
        };
        let expr = if needs_expr {
            Some(
                RustExpr::parse(text)
                    .map_err(|error| format!("`{text}` is not a Rust expression: {error}"))?,
            )
        } else {
            None
        };
        Ok(Self { ty, expr })
    }
}

/// A service config field's expression template.
///
/// The attribute's own name stands for its value: `timeout_ms as u64` becomes
/// `15000u32 as u64`. The template is a parsed expression, so substitution
/// replaces one path node rather than scanning text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceExprTemplate {
    /// The parsed template, with the attribute name as its only slot.
    pub syntax: RustExpr,
    /// The attribute name the template reads.
    pub slot: String,
}

impl ServiceExprTemplate {
    /// Parse `text` as a template whose only slot is `slot`.
    pub fn parse(text: &str, slot: &str) -> syn::Result<Self> {
        Ok(Self {
            syntax: RustExpr::parse(text)?,
            slot: slot.to_string(),
        })
    }

    /// The expression with the attribute's value in place of its name.
    pub fn fill(&self, value: &RustExpr) -> RustExpr {
        struct Filler<'a> {
            slot: &'a str,
            value: &'a Expr,
        }

        impl VisitMut for Filler<'_> {
            fn visit_expr_mut(&mut self, node: &mut Expr) {
                if let Expr::Path(path) = node
                    && path.qself.is_none()
                    && path.path.segments.len() == 1
                    && path.path.segments[0].ident == self.slot
                {
                    *node = self.value.clone();
                    return;
                }
                visit_mut::visit_expr_mut(self, node);
            }
        }

        let mut filled = self.syntax.as_syn().clone();
        Filler {
            slot: &self.slot,
            value: value.as_syn(),
        }
        .visit_expr_mut(&mut filled);
        RustExpr::from_syn(filled)
    }
}

/// The slot name a single-ident sentinel path carries.
fn sentinel_slot(path: &Path) -> Option<String> {
    if path.segments.len() != 1 {
        return None;
    }
    let ident = path.segments.first()?.ident.to_string();
    ident.strip_prefix(SENTINEL).map(str::to_string)
}

/// Replace every `{name}` hole with a sentinel identifier, collecting the
/// hole names in order.
///
/// A `{` inside a string literal is text, not a hole: the emitted Rust may
/// legitimately carry braces there.
pub(crate) fn substitute(text: &str) -> Result<(String, Vec<String>), (String, String)> {
    let mut out = String::new();
    let mut slots = Vec::new();
    let mut rest = text;
    let mut in_string = false;
    let mut escaped = false;
    while let Some(character) = rest.chars().next() {
        rest = &rest[character.len_utf8()..];
        if in_string {
            out.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => {
                in_string = true;
                out.push(character);
            }
            '{' => {
                let Some((name, after)) = rest.split_once('}') else {
                    return Err((
                        "unclosed `{` in a codegen template".into(),
                        "write `{name}`".into(),
                    ));
                };
                let name = name.trim();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    return Err((
                        format!("`{{{name}}}` is not a placeholder"),
                        "placeholder names are identifiers".into(),
                    ));
                }
                out.push_str(SENTINEL);
                out.push_str(name);
                slots.push(name.to_string());
                rest = after;
            }
            '}' => {
                return Err((
                    "unmatched `}` in a codegen template".into(),
                    "write `{name}`".into(),
                ));
            }
            _ => out.push(character),
        }
    }
    Ok((out, slots))
}
