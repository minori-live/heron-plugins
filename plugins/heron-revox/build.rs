use std::{env, fs, io, path::Path};

use sha2::{Digest, Sha256};

const PREPARE: &str = "prepare the pinned models from the repository root:\n\
    node scripts/fetch-revox-model.ts\n\
    uv venv --python 3.12 target/revox/venv\n\
    uv pip install --python target/revox/venv/bin/python --require-hashes -r scripts/revox/requirements.lock\n\
    target/revox/venv/bin/python scripts/revox/export_fcpe.py\n\
    (on Windows use target/revox/venv/Scripts/python.exe for the Python path)";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = env::var("CARGO_MANIFEST_DIR")?;
    let models = Path::new(&manifest).join("../../target/revox");
    embed(
        &models.join("fcpe.onnx"),
        "3fe393cb1942b779a656168043e5d1a0e9ef6ecda05b225caced05b51063faa4",
        "HERON_REVOX_FCPE_MODEL",
    )?;
    embed(
        &models.join("models/revox.onnx"),
        "e7a94cf11388ad12ba9ba1316029fdcbf15f77e342559bd1e9d6971a3f8ee020",
        "HERON_REVOX_VOCODER_MODEL",
    )?;
    Ok(())
}

fn embed(path: &Path, expected: &str, variable: &str) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", path.display());
    let bytes = fs::read(path).map_err(|source| {
        io::Error::new(
            source.kind(),
            format!("could not read {}: {source}; {PREPARE}", path.display()),
        )
    })?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "model checksum mismatch for {}: expected {expected}, got {actual}; {PREPARE}",
                path.display()
            ),
        ));
    }
    println!("cargo:rustc-env={variable}={}", path.display());
    Ok(())
}
