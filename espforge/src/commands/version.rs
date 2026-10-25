/// Print version/build information: the espforge crate version and the
/// version of the project template it generates from.
pub fn run() {
    println!("espforge   {}", env!("CARGO_PKG_VERSION"));
}
