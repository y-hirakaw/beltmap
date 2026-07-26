mod collectors;
mod config;
mod enrich;
mod ir;

fn main() {
    // Phase 1 の配線はこれから。現時点ではパーサとIR型のみ実装済み。
    println!("beltmap {}", env!("CARGO_PKG_VERSION"));
}
