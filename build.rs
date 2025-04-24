use prost_build;
use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(&["src/wire/wire.proto"], &["src/wire/"])?;
    Ok(())
}
