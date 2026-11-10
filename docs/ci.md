# CI と main ブランチの保護

## 実行される検査

`.github/workflows/ci.yml` は `main` または積み上げ元の `codex/T0-2` 向けの Pull Request と、`main` への push で、macOS runner 上に次の検査を実行する。

- Rust: `fmt`、UI 契約の生成忘れ検査、`check`、`clippy`、`test`
- Rust 依存: `cargo-deny` による advisory、license、ban、source 検査、`scripts/generate-sbom.py --check` による SBOM とライセンス一覧の古さの検査
- フロントエンド: `npm ci`、型チェック、lint、build

最後の `ci` ジョブは、Rust・依存検査・フロントエンドのいずれかが成功以外の場合に失敗する。GitHub の branch protection では、この `ci` ジョブを必須 status check に指定する。

## GitHub リポジトリ設定

リポジトリ管理者は `main` に次の ruleset または branch protection を設定する。

1. Pull Request を必須にする。
2. 必須 status check に `ci` を追加する（表示名は `CI / ci`）。
3. 必須チェックが成功しない場合のマージを禁止する。
4. force push とブランチ削除を禁止する。
5. stale approval の破棄を有効にする。

Workflow ファイルだけでは GitHub の保護設定は変更されないため、この設定はリポジトリ管理者が一度行う必要がある。
