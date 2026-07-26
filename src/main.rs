mod collectors;
mod config;
mod enrich;
mod ir;
mod scanlog;

fn main() {
    // Phase 1 の配線はこれから。現時点ではパーサ・IR型・スキャンログのみ実装済み。
    println!("beltmap {}", env!("CARGO_PKG_VERSION"));
}
