//===- build.rs - Build BEMU for buckyball simulation ---------------------===//
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
//===---------------------------------------------------------------------------===//
//
// 1. Why build Spike first?
//  Spike headers/libs are required for bemu's CPU part simulation.
//
// 2. How to build bemu?
//  Link Spike libs with rpath, let bemu can find the libraries at runtime.
//  Spike API calls come from riscv/{processor,extension,rocc}.h in libriscv
//
// 3. How to register chip balls?
//  The chip crate's `src/lib.rs` links the ball emu modules for that chip.
//  Topology comes from chip.pb decoded by prost in the generated crate.
//
//===---------------------------------------------------------------------------===//

mod build_support;

use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let native_dir = build_support::spike::native_dir(&manifest_dir);
    let spike_dir = native_dir.join("spike");
    let spike_install_dir = out_dir.join("spike_install");
    let spike_build_dir = out_dir.join("spike_build");
    build_support::spike::build_and_link(&native_dir, &spike_dir, &spike_build_dir, &spike_install_dir);
    build_support::rerun::emit_engine(&manifest_dir, &native_dir);
    panic!(
        "build bebop-bemu from examples/chips/<chip>/configs/generated/bemu/Cargo.toml, not {}",
        manifest_dir.display()
    );
}
