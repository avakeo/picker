# T2-6 ゼロショット自動タグの評価

- 日付: 2026-10-27
- モデル: clip-japanese（`77a62f8`、5 プロンプト平均ではなく 3 テンプレート平均）
- 語彙: `crates/ml/data/zero_shot_tags.json` の 214 タグ（実写向け、カテゴリ・日英表示名・閾値付き）
- コマンド:
  `PICKER_ORT_DYLIB=... cargo run --release -p picker-ml --example eval_zero_shot -- <ラベル付きディレクトリ> <モデルディレクトリ>`
- 入力: `<タグID>/<画像>` の構成。タグ ID は語彙の ID（例: `color.red`）。

## 合成画像での動作確認

実写のラベル付き評価セットはまだ用意していない。スクリプトの動作確認として、
5 色 × 4 枚の合成画像（ノイズ付き単色）で実行した。精度の判断には使わない。

| tag | tp | predicted | fn | precision | recall |
|---|---:|---:|---:|---:|---:|
| color.blue | 4 | 4 | 0 | 1.000 | 1.000 |
| color.green | 4 | 8 | 0 | 0.500 | 1.000 |
| color.purple | 4 | 4 | 0 | 1.000 | 1.000 |
| color.red | 4 | 4 | 0 | 1.000 | 1.000 |
| color.yellow | 4 | 12 | 0 | 0.333 | 1.000 |

micro precision 0.625、micro recall 1.000（20 枚）。

## 残課題

- 実写セットでのタグ別閾値調整。
- 現状は 1 画像あたり全語彙と比較するため、語彙を増やす場合は
  テキスト埋め込みの事前計算・キャッシュと、必要なら近似最近傍を検討する。
- `rejected` は `replace_suggestions` の `ON CONFLICT DO NOTHING` と
  DB の不変条件で再提案されないことをテスト済み
  （`zero_shot_suggestions_never_replace_rejected_tags`）。
