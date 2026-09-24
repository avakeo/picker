# T0-3 埋め込みモデル検証

T0-3は、評価セットとライセンス確認が揃うまで正式なモデル計測を開始しません。ライセンス確認前にモデルをダウンロードしない候補もあります。

## 準備

1. `docs/eval/templates/` をローカルの評価セットディレクトリへコピーする
2. `docs/eval/README.md` に従って画像、検索クエリ、関連度、タグ、カスタムタグのラベルを記入する
3. `manifest.json` のパスを決める
4. `candidates.template.csv` をコピーし、候補ごとにウェイト・コード・Tokenizer・変換ツールのライセンスを確認する

## 評価セットの検証

```sh
python3 tools/t0-3/validate_eval_set.py \
  --manifest /absolute/path/to/evaluation-set/manifest.json
```

この検証は、画像の存在、CSVの必須列、ID重複、全画像確認フラグ、カスタムタグのtrain/test重複を確認します。エラーがある場合はモデル計測を開始しません。

## 計測開始条件

- T0-3 ADRの評価基準コミットが存在する
- 評価セット検証が成功している
- 3候補以上のライセンス確認が完了している、または除外理由が記録されている
- 実機Mac、macOS、CPUアーキテクチャ、Core ML EPの利用可否を記録している
- Python参照実装とONNX/Rust実装の比較対象・Tokenizerを固定している

計測後はADRの状態を「提案」のままにし、採用・却下の判断は行いません。
