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

現時点で実装済みなのはコレクターのパーサとスキャンログのみで、`cargo run` はバージョンを表示するだけ。

## スキャン結果を持ち帰る

スキャンのたびに `.beltmap/scan-log.jsonl` へ1行ずつ追記される。各行に、コレクターごとの取得元・件数・所要時間・失敗理由と、決定論で埋まらなかった穴が入る。

```sh
cat .beltmap/scan-log.jsonl | tail -1 | jq .
```

このファイルを開発マシンへ持ち帰って改善の材料にする。gitignore されているのでコミットはされない。

**中身にはローカルのパス・リポジトリ名・ホスト名が入る。**共有する前に確認すること。
