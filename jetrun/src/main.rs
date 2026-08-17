//! Entry point. Dispatches to server, worker, or CLI based on argv.

// The workload is allocation-heavy (hashing buffers, tree manifests, trace
// records), which is exactly where the system allocator's central locks hurt
// most under a thread-per-core layout.
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    println!("jetrun {}", env!("CARGO_PKG_VERSION"));
}
