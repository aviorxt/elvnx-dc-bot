fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let build = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("System clock must be after 1970")
                .as_secs()
        });
    println!("cargo:rustc-env=ELVNX_BUILD_NUMBER={build}");
}
