//! The migration oracle: every bundled v2 `driver` must compile to the same
//! recipe as the frozen v1 `element` it replaced.
//!
//! The v1 file is kept for one release under `testing/fixtures/schema-v1`; it
//! is not read by the shipped pipeline. Comparing the typed model, not the
//! text, is what proves the split into per-tag files did not change what is
//! lowered or emitted.

use std::collections::BTreeMap;

use espforge_schema::{AddressNeed, AttrKind, DeviceRecipe, Element, Schema};

fn load_v1() -> Schema {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the workspace")
        .join("testing/fixtures/schema-v1/drivers.schema.kdl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read '{}': {error}", path.display()));
    espforge_schema::load_all(&[
        espforge_schema::SCHEMA_KDL,
        espforge_schema::SERVICES_SCHEMA_KDL,
        &text,
    ])
    .expect("the frozen v1 schema loads")
}

fn load_v2() -> Schema {
    espforge_elements::schema::compose(&[]).expect("the bundled v2 manifests load")
}

fn attr_map(element: &Element) -> BTreeMap<&str, (&AttrKind, bool, Option<&str>)> {
    element
        .attrs
        .iter()
        .map(|attr| {
            (
                attr.name.as_str(),
                (&attr.kind, attr.required, attr.default.as_deref()),
            )
        })
        .collect()
}

fn assert_recipe_eq(tag: &str, v1: &DeviceRecipe, v2: &DeviceRecipe) {
    assert_eq!(v1.bus, v2.bus, "{tag}: bus");
    assert_eq!(v1.feature, v2.feature, "{tag}: feature");
    assert_eq!(v1.kind, v2.kind, "{tag}: kind");
    assert_eq!(v1.ty, v2.ty, "{tag}: type");
    assert_eq!(v1.new, v2.new, "{tag}: constructor");
    assert_eq!(v1.address, v2.address, "{tag}: address need");
    assert_eq!(v1.runtimes, v2.runtimes, "{tag}: runtimes");
    assert_eq!(v1.async_mode, v2.async_mode, "{tag}: async mode");
    assert_eq!(v1.endpoint, v2.endpoint, "{tag}: endpoint");
    assert_eq!(v1.spi_config, v2.spi_config, "{tag}: spi config");
    assert_eq!(v1.cs_polarity, v2.cs_polarity, "{tag}: cs polarity");
    assert_eq!(
        v1.address_default, v2.address_default,
        "{tag}: address default"
    );
    assert_eq!(
        v1.args.iter().map(|arg| &arg.name).collect::<Vec<_>>(),
        v2.args.iter().map(|arg| &arg.name).collect::<Vec<_>>(),
        "{tag}: argument names"
    );
    for (left, right) in v1.args.iter().zip(&v2.args) {
        assert_eq!(left.map, right.map, "{tag}: map for `{}`", left.name);
        assert_eq!(left.pin, right.pin, "{tag}: pin for `{}`", left.name);
        assert_eq!(
            left.direction, right.direction,
            "{tag}: direction for `{}`",
            left.name
        );
    }
    assert!(
        v1.deps.iter().all(|dep| v2.deps.contains(dep)),
        "{tag}: dependencies {:?} vs {:?}",
        v1.deps,
        v2.deps
    );
}

#[test]
fn every_bundled_tag_migrated_to_the_same_recipe() {
    let v1 = load_v1();
    let v2 = load_v2();
    for tag in espforge_drivers::TAGS {
        let old = v1
            .element(tag)
            .unwrap_or_else(|| panic!("`<{tag}>` is in the v1 fixture"));
        let new = v2
            .element(tag)
            .unwrap_or_else(|| panic!("`<{tag}>` is in the v2 manifests"));
        assert_recipe_eq(
            tag,
            old.device_recipe.as_ref().unwrap(),
            new.device_recipe.as_ref().unwrap(),
        );
        assert_eq!(
            attr_map(old),
            attr_map(new),
            "`<{tag}>` attributes changed during migration"
        );
    }
}

#[test]
fn every_bundled_tag_has_exactly_one_file_named_after_it() {
    let v2 = load_v2();
    let tags: Vec<&str> = espforge_drivers::MANIFESTS
        .iter()
        .map(|(path, _)| {
            std::path::Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .expect("a manifest path has a file stem")
        })
        .collect();
    for tag in espforge_drivers::TAGS {
        assert_eq!(
            tags.iter().filter(|stem| **stem == *tag).count(),
            1,
            "`<{tag}>` has one manifest file"
        );
        let element = v2.element(tag).expect("the element loads");
        assert_eq!(
            element.source.as_deref(),
            Some(format!("bundled espforge-drivers/manifests/{tag}.kdl").as_str()),
            "`<{tag}>` records the file it came from"
        );
    }
    assert_eq!(tags.len(), espforge_drivers::TAGS.len());
}

#[test]
fn a_fixed_address_survives_the_migration() {
    let v2 = load_v2();
    let recipe = v2.element("scd4x").unwrap().device_recipe.as_ref().unwrap();
    assert_eq!(recipe.address, AddressNeed::Fixed(0x62));
}
