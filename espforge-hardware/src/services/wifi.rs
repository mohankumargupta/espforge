//! `<wifi>` service lowering (step 1d, first bus).
//!
//! Moved verbatim from `espforge/src/commands/pipeline.rs::wifi_spec` plus
//! the `# services` loop. One network only: `network1.ssid` (required) and
//! `network1.password` (optional; empty or absent means an open network).

use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_model::WifiService;

/// The `<wifi>` service. Both strings end up inside Rust string literals in
/// `generated.rs`.
pub fn wifi_spec(n: &ParsedNode) -> Result<WifiService, Vec<EspfError>> {
    let id = n.id;
    let obj = n.to_object().map_err(|errs| errs)?;

    if let Some(key) = obj
        .fields
        .keys()
        .find(|k| !matches!(k.as_str(), "id" | "network1"))
    {
        return Err(vec![EspfError::at_node(
            n,
            "unsupported wifi field",
            format!(
                "<wifi id=\"{id}\"> has `{key}`, which is not supported yet (only network1.ssid and network1.password: one network)"
            ),
        )]);
    }

    let net = obj
        .get_object("network1")
        .map_err(|e| vec![e])?
        .ok_or_else(|| {
            vec![EspfError::at_node(
                n,
                "missing wifi ssid",
                format!("<wifi id=\"{id}\"> needs network1.ssid=\"...\""),
            )]
        })?;
    if let Some(key) = net
        .fields
        .keys()
        .find(|k| !matches!(k.as_str(), "ssid" | "password"))
    {
        return Err(vec![EspfError::at_node(
            n,
            "unsupported wifi field",
            format!(
                "<wifi id=\"{id}\"> has `network1.{key}`, which is not supported yet (only ssid and password)"
            ),
        )]);
    }

    let ssid = net
        .get_str("ssid")
        .map_err(|e| vec![e])?
        .ok_or_else(|| {
            vec![EspfError::at_node(
                n,
                "missing wifi ssid",
                format!("<wifi id=\"{id}\"> needs network1.ssid=\"...\""),
            )]
        })?
        .value
        .clone();
    let password = net
        .get_str("password")
        .map_err(|e| vec![e])?
        .map(|p| p.value.clone())
        .unwrap_or_default();

    if ssid.is_empty() || ssid.len() > 32 {
        return Err(vec![EspfError::at_node(
            n,
            "invalid wifi ssid",
            format!(
                "<wifi id=\"{id}\"> network1.ssid must be 1 to 32 bytes, got {}",
                ssid.len()
            ),
        )]);
    }
    if !(password.is_empty() || (8..=63).contains(&password.len())) {
        return Err(vec![EspfError::at_node(
            n,
            "invalid wifi password",
            format!(
                "<wifi id=\"{id}\"> network1.password must be empty (open network) or 8 to 63 characters, got {}",
                password.len()
            ),
        )]);
    }
    for (what, value) in [("ssid", &ssid), ("password", &password)] {
        if value.contains('\\') {
            return Err(vec![EspfError::at_node(
                n,
                "invalid wifi value",
                format!("<wifi id=\"{id}\"> network1.{what} cannot contain a backslash"),
            )]);
        }
    }

    Ok(WifiService { ssid, password })
}

/// The `# services` loop: only `<wifi>` is buildable, at most one.
pub fn lower_services(services: &[ParsedNode]) -> Result<Option<WifiService>, Vec<EspfError>> {
    let mut wifi: Option<WifiService> = None;
    for n in services {
        if n.tag_name != "wifi" {
            return Err(vec![EspfError::at_node(
                n,
                "unsupported service",
                format!(
                    "<{}> in `# services` is not supported by `build` yet (only <wifi>)",
                    n.tag_name
                ),
            )]);
        }
        if wifi.is_some() {
            return Err(vec![EspfError::at_node(
                n,
                "duplicate wifi",
                "only one <wifi> is supported in `# services`".to_string(),
            )]);
        }
        wifi = Some(wifi_spec(n).map_err(|errs| errs)?);
    }
    Ok(wifi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_model::{Board, Console, Meta, Runtime, Service};

    const WITH_WIFI: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"embassy\" />\n# peripherals\n# components\n# devices\n# services\n  <wifi id=\"wifi0\" network1.ssid=\"Wokwi-GUEST\" />\n";

    fn meta() -> Meta {
        Meta {
            name: "wifi".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
    }

    fn sections(
        src: &str,
    ) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode<'_>>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<Board, String> {
        crate::board::build(&sections(src), &meta()).map_err(|errs| {
            errs.into_iter()
                .map(|e| format!("{}: {}", e.title, e.label))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    fn wifi_of(board: &Board) -> &WifiService {
        match &board.services[0] {
            Service::Wifi(wifi) => wifi,
        }
    }

    #[test]
    fn a_wifi_service_is_buildable_and_becomes_a_service() {
        let board = lower_src(WITH_WIFI).unwrap();
        assert_eq!(
            wifi_of(&board),
            &WifiService {
                ssid: "Wokwi-GUEST".into(),
                password: String::new(),
            }
        );
        assert!(board.components.is_empty());
    }

    #[test]
    fn wifi_password_rules() {
        let with = |attrs: &str| {
            WITH_WIFI.replace(
                "network1.ssid=\"Wokwi-GUEST\"",
                &format!("network1.ssid=\"home\" {attrs}"),
            )
        };

        let src = with("network1.password=\"short\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("8 to 63"), "{err}");

        let src = with("network1.password=\"longenough\"");
        assert_eq!(wifi_of(&lower_src(&src).unwrap()).password, "longenough");

        let src = with("network1.password=\"back\\\\slash1\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("backslash"), "{err}");
    }

    #[test]
    fn only_one_network_is_supported() {
        let src = WITH_WIFI.replace(
            "network1.ssid=\"Wokwi-GUEST\"",
            "network1.ssid=\"a\" network2.ssid=\"b\"",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("network2"), "{err}");
    }
    #[test]
    fn wifi_without_an_ssid_is_refused() {
        let src = WITH_WIFI.replace(" network1.ssid=\"Wokwi-GUEST\"", "");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("network1.ssid"), "{err}");
    }

    #[test]
    fn other_services_are_not_buildable_yet() {
        let src = WITH_WIFI.replace(
            "<wifi id=\"wifi0\" network1.ssid=\"Wokwi-GUEST\" />",
            "<ntp id=\"n\" servers+=\"pool.ntp.org\" />",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("<ntp>"), "{err}");
    }
}
