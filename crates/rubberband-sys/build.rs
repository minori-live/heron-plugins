//! Compile the vendored Rubber Band single-file amalgamation.

use std::{env, path::PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../../third_party/rubberband");
    let single = root.join("single/RubberBandSingle.cpp");
    assert!(
        single.is_file(),
        "missing vendored Rubber Band single-file unit at {}",
        single.display()
    );

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .file(&single)
        .include(&root)
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-deprecated-declarations");

    // Prefer the system g++ on Linux: some environments put a clang++ named
    // `c++` on PATH without a matching C++ standard library install.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && env::var_os("CXX").is_none()
        && std::path::Path::new("/usr/bin/g++").is_file()
    {
        build.compiler("g++");
    }

    // RubberBandSingle.cpp already selects USE_BQRESAMPLER, NO_THREADING, and
    // either HAVE_VDSP (Apple) or USE_BUILTIN_FFT.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build.define("NOMINMAX", None);
        build.define("_USE_MATH_DEFINES", None);
        build.define("RUBBERBAND_STATIC", None);
    }

    println!("cargo:rerun-if-changed={}", single.display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("SOURCE.json").display()
    );

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        || env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios")
    {
        build.cpp_link_stdlib(Some("c++"));
        println!("cargo:rustc-link-lib=framework=Accelerate");
    } else if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        build.cpp_link_stdlib(Some("stdc++"));
        // rust-lld does not always search the distro multilib path for libstdc++.
        for candidate in [
            "/usr/lib/gcc/x86_64-linux-gnu/13",
            "/usr/lib/gcc/x86_64-linux-gnu/14",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib64",
            "/usr/lib",
        ] {
            let path = std::path::Path::new(candidate);
            if path.join("libstdc++.so").is_file() || path.join("libstdc++.a").is_file() {
                println!("cargo:rustc-link-search=native={candidate}");
                break;
            }
        }
    }

    build.compile("rubberband");
}
