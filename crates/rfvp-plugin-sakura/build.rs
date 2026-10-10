use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    // Length-delimited source inputs make this independent of checkout paths and
    // timestamps. Cargo.lock binds the portable decoder's dependency versions.
    let mut hash = Sha256::new();
    hash.update(b"RFVP-SAKURA-SOURCE-1\0");
    for path in [
        "build.rs",
        "Cargo.toml",
        "src/lib.rs",
        "../rfvp-plugin-api/Cargo.toml",
        "../rfvp-plugin-api/src/lib.rs",
        "../../Cargo.lock",
    ] {
        println!("cargo:rerun-if-changed={path}");
        let bytes = fs::read(path).expect("read Sakura implementation source");
        hash.update((path.len() as u32).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let digest: [u8; 32] = hash.finalize().into();
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("identity.rs"),
        format!("pub const IMPLEMENTATION_CONTENT_SHA256: [u8; 32] = {digest:?};\n"),
    )
    .unwrap();
}
