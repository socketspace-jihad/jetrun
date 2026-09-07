fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only compile protos if protoc is available
    if std::process::Command::new("protoc")
        .arg("--version")
        .output()
        .is_ok()
    {
        tonic_build::compile_protos("../../proto/jetrun.proto")?;
    } else {
        eprintln!("cargo:warning=protoc not found, skipping proto generation. Install protobuf to enable gRPC.");
        // Generate an empty module file so the build succeeds
        let out_dir = std::env::var("OUT_DIR")?;
        std::fs::write(
            std::path::Path::new(&out_dir).join("jetrun.rs"),
            "// Proto stubs — install protoc to generate real gRPC code\n",
        )?;
    }
    Ok(())
}
