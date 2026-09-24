# T0-5 ベクトル検索ベンチマーク

`vector_benchmark.rs` は、外部クレートなしで self full scan の基準値を計測します。ベクトルは固定seedで生成し、単位ベクトルに正規化します。f16のTop-10は同じ入力のf32結果に対する再現率を記録します。

## コンパイル

```sh
rustc --edition=2021 -O tools/t0-5/vector_benchmark.rs -o /tmp/picker-vector-benchmark
```

## 計測

```sh
/tmp/picker-vector-benchmark \
  --output /tmp/picker-vector-benchmark.csv \
  --dimensions 512,768,1152 \
  --sizes 10000,50000,100000 \
  --iterations 3 \
  --warmups 1 \
  --top-k 10
```

出力はリポジトリ外のCSVに保存します。計測環境とコマンドをADRへ転記してください。

sqlite-vecは、利用するリリースまたはビルド済み拡張のパスとライセンスを確認してから別方式として計測します。拡張が見つからない場合、sqlite-vecの値をself full scanの値で代用してはいけません。
