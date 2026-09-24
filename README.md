# Picker

ローカルの画像フォルダを参照したまま索引化し、オンデバイス AI によるタグ付け・検索を行うデスクトップアプリです。

現在は T0-1 の初期構成です。Tauri v2 の空ウィンドウと、Rust の Cargo workspace、Svelte 5 + TypeScript の仮 UI を含みます。

## 開発環境

- macOS（MVP の対象は Apple Silicon）
- Rust stable（`rustup` 経由）
- Node.js 22 以上と npm

## 起動

```sh
npm install
npm run tauri dev
```

フロントエンド単体で確認する場合は `npm run dev` を使います。

## 検査

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --all-features
cargo test --workspace
npm run check
npm run lint
npm run build
```

## 構成

- `src-tauri/`: Tauri アプリ層（起動処理、将来のコマンド・イベント）
- `crates/core/`: ドメイン・ユースケース層
- `crates/store/`: SQLite 永続化境界
- `crates/indexer/`: 再開可能な索引処理
- `crates/fswatch/`: ファイル監視境界
- `crates/imaging/`: 画像デコード・サムネイル境界
- `crates/ml/`: 推論境界・モデルレジストリ
- `crates/learn/`: カスタムタグ学習
- `crates/sidecar/`: JSON / XMP の入出力
- `src/`: Svelte 5 + TypeScript の仮 UI
- `docs/`: 設計書、実装計画、UI 契約

設計判断は `docs/adr/`、脅威モデルなどのセキュリティ文書は `docs/security/` に記録します。
