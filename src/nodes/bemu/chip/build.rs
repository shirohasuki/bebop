#[path = "../../../../../../bebop/src/nodes/bemu/build_support/mod.rs"]
mod build_support;

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let dispatch = manifest_dir.join("dispatch.rs");
    let pb = manifest_dir
        .parent()
        .expect("bemu manifest must live under examples/chips/<chip>/configs/generated/bemu")
        .join("chip.pb");
    let proto = manifest_dir.join("../../../../../../bbdev/api/steps/config/scripts/proto/chip.proto");
    if !dispatch.is_file() {
        panic!("missing {}; run bbdev config --install", dispatch.display());
    }
    if !pb.is_file() {
        panic!("missing {}; run bbdev config --install", pb.display());
    }
    if !proto.is_file() {
        panic!("missing {}", proto.display());
    }
    fs::copy(&dispatch, out_dir.join("chip_balls.rs")).expect("copy dispatch.rs");
    let proto_dir = proto.parent().expect("chip.proto parent").to_path_buf();
    prost_build::compile_protos(&[&proto], &[&proto_dir]).unwrap_or_else(|e| panic!("prost: {e}"));
    println!("cargo:rerun-if-changed={}", dispatch.display());
    println!("cargo:rerun-if-changed={}", pb.display());
    println!("cargo:rerun-if-changed={}", proto.display());

    let engine = manifest_dir.join("../../../../../../bebop/src/nodes/bemu");
    let native_dir = build_support::spike::native_dir(&engine);
    let spike_dir = native_dir.join("spike");
    let spike_install_dir = out_dir.join("spike_install");
    let spike_build_dir = out_dir.join("spike_build");
    build_support::spike::build_and_link(&native_dir, &spike_dir, &spike_build_dir, &spike_install_dir);
    build_support::rerun::emit_engine(&engine, &native_dir);
}
