fn main() {
    println!("cargo:rustc-link-search=/usr/lib");
    println!("cargo:rustc-link-lib=sqlite3");
    println!("cargo:rerun-if-changed=wrapper.h");
}
