# Picker

ローカル画像を元の場所に置いたまま索引化する、オンデバイス画像管理アプリです。画像は参照型で扱い、埋め込み・自動タグ・類似検索・自然言語検索・承認/却下からのカスタムタグ学習をローカルで行います。

> 現状は MVP の開発版です。配布用の署名・公証とスクリーンショットは未整備です。

## 主な機能

- フォルダの再帰スキャン、仮想グリッド、EXIF 表示、サムネイル
- BLAKE3 による同一性判定と dHash による類似画像
- CLIP 埋め込みによる意味検索・類似検索（全件走査）
- 214 語彙のゼロショット自動タグと承認/却下
- 確定タグを正例にしたカスタムタグ候補、ロジスティック回帰、レビュー画面
- タグ・別名・階層・カスタム分類器・確定/却下の JSON エクスポート/インポート
- 任意の XMP サイドカー書き出し（既定無効、既存項目を保持）

## ビルド

```sh
npm ci
cargo build --workspace
npm run tauri dev
```

Rust とフロントエンドの検査:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
npm run check && npm run lint && npm run build && npm run test:ui
```

モデルは `line-corporation/clip-japanese-base` の固定リビジョンを使い、SHA-256 検証後に読み込みます。詳細は `crates/ml/src/registry.rs` と `docs/adr/ADR-001-embedding-model.md` を参照してください。

## プライバシー

ネットワーク通信はモデルのダウンロードのみで、接続先は固定です。テレメトリはありません。索引とタグはローカルの SQLite に保存されます。DB は暗号化しないため、OS のディスク暗号化に依存します。

## 既知の制限

- macOS 実機での ImageIO/HEIC、Core ML、FSEvents、Finder 連携は未検証です。
- T2-8 色検索、T3-9 自動アルバムは仕様未決のため未実装です。
- XMP は確定タグの追記のみで、削除の同期は行いません。
- v0.1.0 の GitHub Releases 公開、Developer ID 署名・公証は未実施です。

## ドキュメント

- [設計](docs/design.md)
- [実装計画](docs/implementation-plan.md)
- [UI 契約](docs/ui-contract.md)
- [ADR](docs/adr/)
- [脅威モデル](docs/security/threat-model.md)
- [ライセンス一覧](docs/licenses.md) / [SBOM](docs/sbom.cdx.json)
