extern crate intrinsic_gen;

use intrinsic_gen::*;
use std::path::{Path,PathBuf};

fn main() {
    std::env::set_var("RUST_BACKTRACE", "full");
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let mut input = PathBuf::from(&dir);
    input.push("tests/input");

    let mut output = PathBuf::from(&dir);
    //output.push("tests/output");

    let files = input.read_dir().expect("read_dir failed");
    for entry in files {
        if let Ok(entry) = entry {
            let platform = parse(&entry.path());
            generate(platform, &output);
        }
    }
}
