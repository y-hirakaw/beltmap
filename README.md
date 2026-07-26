# beltmap

AIループ(定期実行されるAIエージェント)をつないだ「工場」の構成をスキャンし、TUIで地図として可視化するツール。

**開発中(v0.1)。まだ地図は出ない。**

設計と調査結果は [初版計画.md](初版計画.md) を参照。

## 工場マシンで動かす

前提:

- macOS
- [`gh`](https://cli.github.com/) が認証済み (`gh auth status` で確認)
- Rust stable — 未導入なら `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

手順:

```sh
git clone https://github.com/y-hirakaw/beltmap.git
cd beltmap
cargo test   # 現状ここまでが動く範囲
cargo run
```

現時点で実装済みなのはコレクターのパーサのみで、`cargo run` はバージョンを表示するだけ。
