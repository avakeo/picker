# T2-3 ベクトル検索の計測

- 日付: 2026-10-27
- 環境: Linux aarch64、release ビルド、CPU
- 方式: 正規化済み f32 ベクトルの全件走査（ADR-003）
- データ: 固定 seed の 50,000 件 × 512 次元、クエリ 20 件、Top-10
- コマンド: `cargo test -p picker-ml --release --lib fifty_thousand -- --ignored --nocapture`
- 結果: p95 23.15 ms（設計目標 200 ms 以内）

SQLite からの読み込みと UI への変換は含まない。インデックスは起動時に
`embeddings` テーブルからメモリへ構築し、埋め込み追加時に更新する。
