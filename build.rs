use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(
        &["src/wire/types.proto", "src/wire/p2p.proto"],
        &["src/wire/"],
    )?;
    Ok(())
}
