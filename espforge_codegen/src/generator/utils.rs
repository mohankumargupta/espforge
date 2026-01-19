use anyhow::{Result, anyhow};
use proc_macro2::Ident;
use quote::format_ident;

pub fn resolve_resource_ident(reference: &str) -> Result<Ident> {
    let name = reference
        .strip_prefix('$')
        .ok_or_else(|| anyhow!("Invalid resource reference {}", reference))?;
    Ok(format_ident!("{}", name))
}
