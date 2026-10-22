fn main() {
    // include_dir! does not register its files with cargo.
    println!("cargo:rerun-if-changed=src/examples");
}
