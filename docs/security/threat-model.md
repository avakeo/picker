# 脅威モデル（草案）

対象はローカル画像管理アプリ Picker の MVP。設計書第12章の脅威と対策を、実装・テスト・確認手順に対応付ける。本番 UI の点検（T4-1 の開始条件）は未完了のため、状態は草案とする。

| 脅威 | 対策 | 検証 |
|---|---|---|
| 悪意のある画像・解凍爆弾 | デコーダはサイズ/画素数の上限を検査し、失敗を型付きエラーとして返す。OS 標準デコーダを使う | `crates/imaging` の異常系テスト、`cargo test --workspace` |
| モデル改ざん・すり替え | 固定 URL・固定 SHA-256・HTTPS のみ。不一致は読み込まない | `registry` のハッシュ不一致テスト |
| Webview 経由の IPC 悪用 | Tauri capability を最小化し、外部由来文字列は Svelte のテキスト補間で描画する。リモートコンテンツなし | `src-tauri/capabilities/default.json`、`grep` による `{@html}` 不使用確認 |
| カスタムプロトコルのパストラバーサル | アセット ID とサイズのみ受け付け、キャッシュ配下に限定 | `thumbnail_protocol` の不正入力テスト |
| SQL インジェクション | rusqlite のパラメータ化クエリのみ | `cargo clippy`、コードレビュー |
| XMP/EXIF の外部実体・DTD | DOCTYPE/ENTITY を拒否。XML は自前の限定的マージで処理 | `picker-sidecar` の `refuses_dtd_and_entities` |
| サプライチェーン | lockfile、`cargo deny`、`npm audit`、SBOM、ライセンス一覧 | `cargo deny check advisories licenses bans sources`、`docs/sbom.cdx.json` |
| プライバシー漏洩 | 通信はモデル取得のみ。テレメトリなし。GPS はローカル表示のみ | コードレビュー、ネットワーク依存の限定 |
| 顔特徴量（将来） | 既定無効。エクスポート対象外 | 未実装のため対象外 |

## 残作業

- 本番 UI 完成後に CSP、Tauri 権限、IPC 引数の上限を再点検する。
- macOS の署名・公証とリリース成果物のハッシュ公開は T4-3 で行う。
