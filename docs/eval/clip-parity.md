# clip-japanese 埋め込みパリティ確認

対象: T2-2。モデルは `line-corporation/clip-japanese-base` の固定リビジョン
`77a62f8af977acd73ec2f927fb73fdaeb13af7d1` の ONNX 画像・テキストエンコーダと
`spiece.model`。ファイルの SHA-256 はモデルレジストリに固定している。

## 手順

1. モデルレジストリで clip-japanese を導入する。
2. Linux で ONNX Runtime を動的ロードする場合は、`PICKER_ORT_DYLIB` に
   `libonnxruntime.so` を指定する。macOS は埋め込みバイナリを使う。
3. 参照実装の出力を作り直す場合は、Python の `onnxruntime`、`transformers`、
   `sentencepiece`、`Pillow` を使い、次の条件で計算する。
   - 画像: 長辺を 224 に BICUBIC で縮小し、足りない部分を黒で中央パディング。
     ImageNet の平均・標準偏差で正規化し、CHW に並べる。
   - テキスト: rinna/japanese-roberta-base の T5 SentencePiece を使い、小文字化、
     `[CLS]` (id 4) を先頭に付け、id 3 で 77 トークンまでパディング。
     position ids は 0..76、attention mask はパディング以外 1。
   - 出力を L2 正規化する。
4. 次のコマンドで Rust 実装と固定データを比較する。

```sh
PICKER_CLIP_JAPANESE_DIR=/path/to/clip-japanese \
PICKER_ORT_DYLIB=/path/to/libonnxruntime.so \
cargo test -p picker-ml --test clip_japanese -- --ignored --nocapture
```

固定画像は `crates/ml/tests/fixtures/`、参照ベクトルは
`crates/ml/tests/golden/clip-japanese.json` に置く。画像は合成画像のみで、
外部ライセンスの画像を含まない。

## 結果（2026-10-27、Linux aarch64、CPU）

| 入力 | cosine |
|---|---:|
| solid.png (64x64) | 0.99999946 |
| gradient.png (320x240) | 0.99986637 |
| gradient224.png (224x224) | 1.0000001 |
| circle224.png (224x224) | 0.99999976 |
| checker.png (200x300) | 0.99951106 |
| 犬 | 0.9999997 |
| a photo of a cat | 0.9999997 |
| 海辺の夕日 | 0.9999997 |

しきい値は cosine 0.999。同一画像を2回埋め込んだ結果は 1e-6 以内で一致する。
Core ML 実行プロバイダは macOS でしか確認できないため、この表は CPU の結果である。
