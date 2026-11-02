//! The ESPF parser: sections, self-closing elements, attributes with byte
//! spans, dotted keys, `+=` lists, comment blocks and junk lines.
//!
//! The parser is lossless: it reports raw `(key, op, value)` attributes and
//! [`value::assemble`] turns them into an [`value::Object`]. Every error is an
//! `espforge_diagnostics::Diagnostic` with a typed code, constructed at the
//! point of failure.

pub mod context;
pub mod diag;
pub mod parser;
pub mod value;
