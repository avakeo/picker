# T0-3 足切り・性能計測手順

品質ランキングは、ユーザーが `queries.csv`、`tags.csv`、`similarity-queries.csv` を固定するまで生成しない。この手順で先に行うのは固定サンプルの実装一致と、順位を保存しない画像単体推論だけである。

## 環境

- Apple M5 / 16 GB / macOS 26.6.2 / arm64
- Python 3.12.13
- Pythonパッケージは `tools/t0-3/requirements-benchmark.txt` に固定
- ONNX Runtime 1.30.0。CPU EPとCore ML EPを別々に実行
- Core ML EPは `ModelFormat=NeuralNetwork`、`MLComputeUnits=ALL`。MLProgramは画像側Convの`pad`属性を解釈できなかったため使用しない
- Core ML計測時もCPU EPをfallbackとして登録する。ONNX Runtimeが一部ノードだけをCore MLへ分割するため、結果を「Core MLのみ」の性能とは表現しない

仮想環境、モデル、ONNX、raw-resultsはすべてgit管理外の `eval-data/` に置く。

## ONNXと足切り

Optimumで画像・テキストを含むONNXを出力し、`split_onnx.py` で画像・テキスト経路を分離する。f16は同じツールで変換し、画像・テキストを合算した実ファイルサイズを記録する。

```sh
python tools/t0-3/split_onnx.py \
  --input eval-data/models/CANDIDATE/onnx-fp32/model.onnx \
  --output eval-data/models/CANDIDATE/split-fp16 \
  --precision fp16
```

`check_embedding_parity.py` は固定日本語3件と固定画像2件だけを使い、Python参照とONNXの行ごとのcosineを記録する。最低値が0.999未満なら足切り不合格。CPUとCore ML EPで別々に実行する。

Rustのtoken IDは `tokenizer-parity` を使い、Python参照の全IDと完全一致した場合だけ合格とする。

## 性能

`benchmark_onnx.py` は `images.csv` の50枚を読み、前処理を計時外で済ませる。batch 1、5回ウォームアップ後に全50枚を推論し、これを独立プロセスで5回繰り返す。

- ロード: 5回の生値、p50、p95
- 推論: 250件の生値、p50、p95
- メモリ: 各プロセスの最大RSSの最大値
- サイズ: 配布対象の画像ONNX、テキストONNX、Tokenizer設定の実ファイル合計

出力JSONには `rankings_generated: false` を記録する。画像埋め込みは直ちに破棄し、類似度、順位、検索結果を生成・保存しない。

`trust_remote_code` を使う前処理のモジュールキャッシュは、既定で `eval-data/hf-modules-cache/` に置く。ワークスペース外のユーザーキャッシュへ書き込まない。

集計前のロード・推論タイミング列は `eval-data/raw-results/` に保持し、重み・上限の再計算に必要なp50/p95、サイズ、最大RSS、cosine、Tokenizer一致は `tools/t0-3/raw-results.csv` に記録する。モデル、画像、公開データセットはGitへ追加しない。
