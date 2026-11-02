//! `<wifi_station>`: associate with an access point over a `<wifi>` radio.
//!
//! Not recipe-shaped (strings and a choice, no pin), so it is a hand-written
//! element. It constructs in `Components::new` and *starts* from a `main()`
//! startup statement: the radio needs the scheduler running and a heap, and
//! `Components::new` runs before `esp_rtos::start`.
//!
//! Two rules the schema cannot express are checked here: the SSID's byte length
//! and the password's character length (there is no length `range` kind, and
//! adding one for two attributes is not worth a schema feature).

use espf::context::ParsedNode;
use espf::diag::node_span;
use espforge_diagnostics::codes;
use linkme::distributed_slice;
use quote::{format_ident, quote};

use crate::attrs::Issue;
use crate::element::{Element, EmitCx, LowerCx, Lowered, Reported, is_rust_ident};
use crate::generated::wifi_station::{Attrs, Auth};
use crate::model::{Board, Component, Ident};
use crate::registry::ELEMENTS;

/// The `<wifi_station>` element.
pub struct WifiStationElement;

#[distributed_slice(ELEMENTS)]
static WIFI_STATION: &dyn Element = &WifiStationElement;

impl Element for WifiStationElement {
    fn tag(&self) -> &'static str {
        "wifi_station"
    }

    fn lower(&self, node: &ParsedNode<'_>, cx: &mut LowerCx) -> Result<(), Reported> {
        let attrs = Attrs::from_node(node).map_err(|issue| describe(cx, node, issue))?;
        let id = cx.ident(node);
        let radio = Ident::new(attrs.wifi.name, span_of(node, "wifi"));

        if !is_rust_ident(&id.rust_name()) {
            return Err(cx.report(
                codes::COMPONENT_INVALID_ID,
                node_span(node),
                "invalid wifi_station",
                format!(
                    "<wifi_station id=\"{}\">: the id must become a Rust identifier \
                     (letters, digits, `_`; not starting with a digit)",
                    id.raw
                ),
            ));
        }

        if !cx.has_radio(radio.as_str()) {
            return Err(cx.report(
                codes::COMPONENT_UNKNOWN_WIFI,
                node_span(node),
                "unknown wifi radio",
                format!(
                    "<wifi_station id=\"{}\"> refers to `{}`, which is not a <wifi> in `# peripherals`",
                    id.raw, radio.raw
                ),
            ));
        }
        if let Some(other) = cx.radio_station(radio.as_str()).map(str::to_string) {
            return Err(cx.report(
                codes::COMPONENT_WIFI_RADIO_USED,
                node_span(node),
                "wifi radio already used",
                format!(
                    "<wifi_station id=\"{other}\"> and <wifi_station id=\"{}\"> both use <wifi id=\"{}\">",
                    id.raw, radio.raw
                ),
            ));
        }

        // `ssid`: 1 to 32 bytes, as in the legacy validations.
        let ssid = attrs.ssid.value;
        let ssid_bytes = ssid.len();
        if ssid_bytes == 0 || ssid_bytes > 32 {
            return Err(cx.report(
                codes::COMPONENT_INVALID_SSID,
                span_of(node, "ssid"),
                "invalid ssid",
                format!(
                    "<wifi_station id=\"{}\"> ssid must be 1 to 32 bytes, but \"{ssid}\" is {ssid_bytes}",
                    id.raw
                ),
            ));
        }

        let password = match attrs.auth.value {
            // An open network carries no password at all.
            Auth::Open => match attrs.password.as_ref().map(|p| p.value) {
                Some(written) if !written.is_empty() => {
                    return Err(cx.report(
                        codes::COMPONENT_OPEN_WITH_PASSWORD,
                        span_of(node, "password"),
                        "password given for an open network",
                        format!(
                            "<wifi_station id=\"{}\"> auth=\"open\" cannot carry a password",
                            id.raw
                        ),
                    ));
                }
                _ => Password::Open,
            },
            Auth::Wpa2 => match attrs.password.as_ref().map(|p| p.value) {
                None => {
                    return Err(cx.report(
                        codes::COMPONENT_INVALID_PASSWORD,
                        node_span(node),
                        "missing password",
                        format!(
                            "<wifi_station id=\"{}\"> needs a password unless auth=\"open\"",
                            id.raw
                        ),
                    ));
                }
                Some(raw) => match raw.strip_prefix("env:") {
                    Some(name) => {
                        if !is_env_name(name) {
                            return Err(cx.report(
                                codes::COMPONENT_INVALID_ENV,
                                span_of(node, "password"),
                                "invalid environment variable name",
                                format!(
                                    "<wifi_station id=\"{}\"> env:{name} is not a variable name \
                                     (letters, digits, `_`; not starting with a digit)",
                                    id.raw
                                ),
                            ));
                        }
                        Password::Env(name.to_string())
                    }
                    None => {
                        let chars = raw.chars().count();
                        if !(8..=63).contains(&chars) {
                            return Err(cx.report(
                                codes::COMPONENT_INVALID_PASSWORD,
                                span_of(node, "password"),
                                "invalid password",
                                format!(
                                    "<wifi_station id=\"{}\"> password must be 8 to 63 characters, \
                                     but this one is {chars}",
                                    id.raw
                                ),
                            ));
                        }
                        Password::Literal(raw.to_string())
                    }
                },
            },
        };

        cx.set_radio_station(radio.as_str(), id.raw.clone());
        cx.add_network(id.raw.clone());
        cx.push(Component::WifiStation(WifiStationComponent {
            id,
            radio,
            ssid: ssid.to_string(),
            password,
        }));
        Ok(())
    }
}

/// The password as the generated Rust writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Password {
    /// An open network: the runtime sees an empty password.
    Open,
    /// A literal, compiled into the firmware verbatim.
    Literal(String),
    /// `env:NAME`: a compile-time `env!("NAME")`.
    Env(String),
}

impl Password {
    /// The tokens the constructor argument is written with.
    fn tokens(&self) -> proc_macro2::TokenStream {
        match self {
            Password::Open => quote! { "" },
            Password::Literal(text) => quote! { #text },
            Password::Env(name) => {
                let name = name.as_str();
                quote! { env!(#name) }
            }
        }
    }
}

/// One lowered `<wifi_station>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiStationComponent {
    pub id: Ident,
    /// The `<wifi>` radio it rides on.
    pub radio: Ident,
    pub ssid: String,
    pub password: Password,
}

impl Lowered for WifiStationComponent {
    fn id(&self) -> &Ident {
        &self.id
    }

    fn emit(&self, _board: &Board, cx: &mut EmitCx) {
        let name = format_ident!("{}", self.id.rust_name());
        let ssid = &self.ssid;
        let password = self.password.tokens();

        cx.field(quote! {
            pub #name: espforge_runtime::WifiStation,
        });
        cx.init(quote! {
            #name: espforge_runtime::WifiStation::new(espforge_runtime::WifiConfig {
                ssid: #ssid,
                password: #password,
            }),
        });

        // The radio needs the scheduler and the heap, so it starts from main
        // after `esp_rtos::start`, not from `Components::new`.
        let components = cx.main().components.clone();
        let registry = cx.main().registry.clone();
        let spawner = cx.main().spawner.clone();
        cx.startup(quote! {
            #components
                .#name
                .start(#registry.peripherals.WIFI, &#spawner)
                .expect("wifi start failed");
        });
    }
}

/// `[A-Za-z_][A-Za-z0-9_]*`.
fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The value span of one attribute on `node`, or the node itself.
fn span_of(node: &ParsedNode<'_>, attr: &str) -> std::ops::Range<usize> {
    node.get_attr_spanned(attr)
        .map(|a| a.val_offset..a.val_offset + a.val.len())
        .unwrap_or_else(|| node_span(node))
}

/// This element's wording for a schema-level problem.
fn describe(cx: &mut LowerCx, node: &ParsedNode<'_>, issue: Issue<'_>) -> Reported {
    let id = node.id;
    let message = match issue {
        Issue::Missing { attr } => format!("<wifi_station id=\"{id}\"> is missing `{attr}`"),
        Issue::Invalid { attr, value } => {
            format!("<wifi_station id=\"{id}\"> has an invalid {attr}=\"{value}\"")
        }
    };
    cx.report(
        codes::COMPONENT_INVALID_WIFI,
        node_span(node),
        "invalid wifi_station",
        message,
    )
}
