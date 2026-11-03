//! Prints arena state and kill-cam sizes.
use engine::arena::*;
fn main() {
    println!("STATE_LEN {STATE_LEN} bytes, {STATE_CHUNKS} chunks");
    println!("Arena in memory {} bytes", core::mem::size_of::<Arena>());
}
