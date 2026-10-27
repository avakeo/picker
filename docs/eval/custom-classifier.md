# T3-3 カスタムタグ分類器の計測

- 日付: 2026-10-27
- 環境: Linux aarch64、release ビルド、CPU
- 実装: L2 正則化ロジスティック回帰（自前実装、full-batch 勾配降下）
- データ: 50,000 件 × 512 次元、`x[0] > 0` の合成境界、20 epoch
- コマンド:
  `cargo test -p picker-ml --release --lib fifty_thousand_samples -- --ignored --nocapture`
- 結果: 学習と全件スコアリングで 0.353 秒（設計目標 1 秒以内、DB 入出力を除く）

境界の学習は `learns_a_known_linear_boundary`、段階 A から段階 B への切り替えは
`custom_classifier_switches_on_after_ten_examples` で確認している。
