fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protos = ["proto/pwf/v1/pwf.proto"];
    tonic_prost_build::configure()
        .build_transport(false)
        .boxed(".pwf.v1.TaskRead.value.data")
        .file_descriptor_set_path(std::env::var("OUT_DIR")? + "/pwf_descriptor.bin")
        .compile_protos(&protos, &["proto"])?;

    for proto in protos {
        println!("cargo:rerun-if-changed={proto}");
    }
    Ok(())
}
