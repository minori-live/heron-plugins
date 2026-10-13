use std::{env, io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return Ok(());
    }

    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=SDKROOT");
    let output = Command::new("xcrun")
        .args(["clang", "--print-resource-dir"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "could not locate Apple compiler runtime: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let resource_dir = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let library_dir = PathBuf::from(resource_dir.trim()).join("lib/darwin");
    let archive = library_dir.join("libclang_rt.osx.a");
    if !archive.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Apple compiler runtime not found at {}", archive.display()),
        ));
    }

    // The bundled CoreML objects call Apple's OS-version availability helpers.
    // Rust links with -nodefaultlibs, so clang does not add this archive for us.
    // Native library metadata carries the dependency to examples and tests too.
    println!("cargo:rustc-link-search=native={}", library_dir.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
    Ok(())
}
