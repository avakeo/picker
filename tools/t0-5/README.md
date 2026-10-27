# T0-5 ベクトル検索ベンチマーク

`vector_benchmark.rs` は、外部クレートなしで self full scan の基準値を計測します。`run_benchmarks.py` は同じ固定seedのデータでsqlite-vec 0.1.9も計測します。ベクトルは単位ベクトルに正規化し、f16のTop-10は同じ入力のf32結果に対する再現率を記録します。

## コンパイル

```sh
rustc --edition=2021 -O tools/t0-5/vector_benchmark.rs -o /tmp/picker-vector-benchmark
```

## 計測

```sh
/tmp/picker-vector-benchmark \
  --output /tmp/picker-vector-benchmark.csv \
  --dimensions 512 --count 10000 --query-count 20 \
  --input /tmp/vectors.bin --queries /tmp/queries.bin \
  --precision f32 --warmups 3 \
  --top-k 10
```

全ケースの比較は次で実行します。

```sh
python3 tools/t0-5/run_benchmarks.py \
  --binary /tmp/picker-vector-benchmark \
  --output /tmp/picker-t0-5-results.csv
```

出力はリポジトリ外のCSVに保存します。計測環境とコマンドをADRへ転記してください。

sqlite-vecは、利用するリリースまたはビルド済み拡張のパスとライセンスを確認してから別方式として計測します。拡張が見つからない場合、sqlite-vecの値をself full scanの値で代用してはいけません。
