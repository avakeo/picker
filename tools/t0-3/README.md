# T0-3 埋め込みモデル検証

T0-3は、評価入力を固定する前に検索・タグ・類似検索の順位を生成しません。ライセンス、ONNX変換、埋め込み一致、Tokenizer一致、順位を保存しない性能計測は先に実施できます。

## 準備

1. `docs/eval/templates/` をローカルの評価セットディレクトリへコピーする
2. `docs/eval/README.md` に従って画像、検索クエリ、タグ、類似検索クエリ、カスタムタグ3〜5個のラベルを記入する
3. `manifest.json` のパスを決める
4. `candidates.template.csv` をコピーし、候補ごとにウェイト・コード・Tokenizer・変換ツールのライセンスを確認する

## 評価セットの検証

```sh
python3 tools/t0-3/validate_eval_set.py \
  --manifest /absolute/path/to/evaluation-set/manifest.json
```

この検証は、最低件数、画像の存在とSHA-256、CSVの必須列、ID重複、カスタムタグのtrain/test分離を確認し、固定入力のハッシュを出力します。エラーがある場合は品質ランキングを生成しません。

## ブラインド採点

候補名との対応は `candidate-map.csv` だけに置きます。`pool-rankings.csv` を作った後、次を実行します。

```sh
python3 tools/t0-3/build_blind_scoring.py --eval-root /Users/runner/work/picker/eval-data
```

生成されたHTMLは外部通信をせず、0/1/2キー、途中保存、再開、CSV書き出しに対応します。採点後は次で指標、自己一致率、クエリ単位ブートストラップ95%信頼区間を算出します。

```sh
python3 tools/t0-3/analyze_blind_scores.py \
  --eval-root /Users/runner/work/picker/eval-data
```

採点HTML、対応表、採点CSV、分析結果はいずれもgit管理外の `eval-data/` に置きます。

## 計測開始条件

- T0-3 ADRの評価基準コミットが存在する
- クエリ20件以上、タグ20〜30語、類似クエリ画像10件以上、カスタムタグ3〜5個をユーザーが固定し、評価セット検証が成功している
- 3候補以上のライセンス確認が完了している、または除外理由が記録されている
- 実機Mac、macOS、CPUアーキテクチャ、Core ML EPの利用可否を記録している
- Python参照実装とONNX/Rust実装の比較対象・Tokenizerを固定している

足切り・性能計測後もADRの状態を「提案」のままにし、採用・却下の判断は行いません。未取得の数値を推測で補完しません。

再現可能な変換、埋め込み一致、Rust Tokenizer一致、順位を保存しない性能計測の詳細は `docs/eval/measurement-procedure.md` に記録しています。

実測の集計値は `raw-results.csv` にあり、品質指標の空欄はユーザーが入力を固定してブラインド採点を完了するまで埋めません。Core ML行はCPU fallbackを含みます。
