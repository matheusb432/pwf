use std::fs;

use anyhow::{Context as _, Result, ensure};

use crate::paths;

pub(crate) fn run(check: bool) -> Result<()> {
    let root = paths::repo_root().join("crates/pwf-wire");
    let generated = root.join("src/generated");
    let temporary = tempfile::tempdir()?;
    tonic_prost_build::configure()
        .build_transport(false)
        .out_dir(temporary.path())
        .file_descriptor_set_path(temporary.path().join("pwf_descriptor.bin"))
        .compile_protos(
            &[root.join("proto/pwf/v1/pwf.proto")],
            &[root.join("proto")],
        )?;
    for name in ["pwf.v1.rs", "pwf_descriptor.bin"] {
        let fresh = fs::read(temporary.path().join(name))?;
        let destination = generated.join(name);
        if check {
            let committed = fs::read(&destination)
                .with_context(|| format!("read {} - run just proto", destination.display()))?;
            ensure!(committed == fresh, "{name} is stale - run just proto");
        } else {
            fs::create_dir_all(&generated)?;
            fs::write(destination, fresh)?;
        }
    }
    Ok(())
}
