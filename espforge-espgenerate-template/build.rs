fn main() {
    // include_dir! does not register its files with cargo, so tell cargo
    // to rebuild whenever the template or the KDL/text files change.
    println!("cargo:rerun-if-changed=src/template");
    println!("cargo:rerun-if-changed=src/options.kdl");
    println!("cargo:rerun-if-changed=src/elements.kdl");
    println!("cargo:rerun-if-changed=src/overview.txt");
}
