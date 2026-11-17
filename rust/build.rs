use std::env;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../app.manifest");
    println!("cargo:rerun-if-changed=../app.rc");
    let out_dir = env::var("OUT_DIR").unwrap();
    let res_path = format!("{}/app.res", out_dir);
    // Try x86_64-w64-mingw32-windres or windres
    let status = Command::new("x86_64-w64-mingw32-windres")
        .args(&["--no-preprocess", "../app.rc", "-O", "coff", "-o", &res_path])
        .status()
        .or_else(|_| {
            Command::new("windres")
                .args(&["--no-preprocess", "../app.rc", "-O", "coff", "-o", &res_path])
                .status()
        });
    if let Ok(s) = status {
        if s.success() {
            println!("cargo:rustc-link-arg={}", res_path);
        }
    }
}
