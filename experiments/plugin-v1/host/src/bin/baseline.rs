//! Fixture parsing baseline; intentionally does not link the probe library/runtime.
fn main() -> anyhow::Result<()> {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("fixture path"));
    let context: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("context.json"))?)?;
    let mut total = 0;
    for source in context["sources"].as_array().expect("sources") {
        total += std::fs::read(root.join(source["path"].as_str().expect("path")))?.len();
    }
    println!("{}", std::hint::black_box(total));
    Ok(())
}
