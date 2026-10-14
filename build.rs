fn main() {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let epoch = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value
            .parse::<i64>()
            .expect("SOURCE_DATE_EPOCH must be a Unix timestamp"),
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System clock must be after 1970")
            .as_secs() as i64,
    };
    let time = chrono::DateTime::<chrono::Utc>::from_timestamp(epoch, 0)
        .expect("Build timestamp must be within the supported calendar range");
    assert!(
        (1970..=9999).contains(&chrono::Datelike::year(&time)),
        "Build year must be 1970–9999"
    );
    let build = time.format("%Y%m%d.%H%M%S");
    println!("cargo:rustc-env=ELVNX_BUILD_NUMBER={build}");
    println!("cargo:rustc-env=ELVNX_BUILD_EPOCH={epoch}");
    println!(
        "cargo:rustc-env=ELVNX_RELEASE={}+build.{build}",
        std::env::var("CARGO_PKG_VERSION").expect("Cargo package version")
    );
}
