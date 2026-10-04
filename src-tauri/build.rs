fn main() {
    println!("cargo:rerun-if-changed=../product-release.json");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    let source = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|source| source.trim().to_owned())
        .filter(|source| source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unavailable".into());
    println!("cargo:rustc-env=LOCALBRIDGE_SOURCE_COMMIT={source}");
    tauri_build::build();
}
