fn main() {
    println!("cargo:rerun-if-env-changed=SQLX_OFFLINE");
    let offline = std::env::var("SQLX_OFFLINE").unwrap_or_else(|_| "true".into());
    println!("cargo:rustc-env=SQLX_OFFLINE={offline}");
}
