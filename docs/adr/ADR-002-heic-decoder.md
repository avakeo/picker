# ADR-002: HEICデコード方式

- 状態: 提案
- タスク: T0-4
- 作成日: 2026-09-25
- 計測環境: Apple M5 / 16 GB / macOS 26.6.2 / arm64（計測前提）

## 文脈

PickerはHEIC、JPEG、PNG、WebPを読み込み、向き補正とsRGB化を行ったサムネイルを生成する。macOSではImageIOを利用し、OS依存部分をRustのデコーダ境界の背後に隔離する。

ImageIOについて、次の方式を同一入力で比較する。

1. 画像全体をデコードしてから縮小する
2. `CGImageSourceCreateThumbnailAtIndex` でサムネイルを生成する（必要時に元画像から生成）
3. 埋め込みサムネイルがある場合はそれを優先し、なければImageIOに生成させる

## 計測前に固定した評価基準

以下の基準をこのADRの計測前コミットで固定する。計測結果を見て重みや足切り条件を変更しない。変更が必要な場合は、理由を別コミットに記録し、計測をやり直す。

### 重み

| 評価項目 | 重み |
|---|---:|
| 正確性（形式、向き、sRGB、壊れた入力の安全な失敗） | 40 |
| サムネイル生成時間（p95） | 25 |
| ピークメモリ | 20 |
| Rust境界への統合・保守性 | 10 |
| fallbackの明確さ | 5 |

### 足切り条件

- 対応形式のいずれかでプロセスがクラッシュする方式は不採用
- EXIFの向きが正しく反映されない方式は不採用
- sRGBへ正規化できない方式は不採用
- 壊れた画像・上限超過画像をエラーとして返せない方式は不採用
- macOS以外でのビルド境界またはfallback方針を説明できない方式は不採用

### 計測条件

- 対応4形式、向き情報付き画像、カラープロファイル違い、壊れた入力を含める
- ウォームアップ後に複数回測定し、中央値とp95を記録する
- 画像ごとの入力サイズ、出力サイズ、成否を生データへ保存する
- 同一プロセス、同一サムネイル最大辺、同一入力順で比較する

## 計画

`tools/t0-4/imageio_benchmark.swift` で3方式を計測し、実機の機種・OS・サンプル数・生データを記録する。ImageIOとAppleのシステムフレームワーク自体に追加の再配布ライセンスはない。Rust側の呼び出し方式を追加する場合は、そのクレートとライセンスを別途記録する。

## 結果

Apple M5 / 16 GB / macOS 26.6.2 / arm64で、`eval-data/images` の50枚、各5回、ウォームアップ2回、最大辺512pxで計測した。成功した画像ごとのp95の中央値、成功したプロセスの最大RSSを記録する。raw JSONはリポジトリ外の `/private/tmp/picker-t0-4-results/all.json` に保存した。

| 方式 | 成功率 | p50 | p95 | ピークメモリ | 正確性 | スコア |
|---|---:|---:|---:|---:|---|---:|
| 全体デコード後に縮小 | 21/50 | 6.64 ms | 28.83 ms | 59.5 MB | JPEG/PNGの成功分は向き・sRGB確認 | 足切り不合格（HEIC等で失敗） |
| ImageIOサムネイル生成 | 50/50 | 5.14 ms | 41.38 ms | 62.1 MB | 全件で向き・sRGB確認 | 通過 |
| 埋め込みサムネイル優先 | 50/50 | 1.80 ms | 18.11 ms | 62.1 MB | 全件で向き・sRGB確認 | 通過 |

壊れた入力と空ファイルは各方式でエラーとして返り、プロセスは継続した。ImageIOのHEIC destination encoderが実機で利用できなかったため、テストでは外部のgit管理外HEIC fixtureを指定できるようにし、ベンチマーク本体は既存の実写HEIC 20枚で検証した。WebP destinationはImageIOが提供しないため、決定的な小さなfixtureをバイト列で生成した。

## 提案

**仮決定（DeepSeek、2026-10-27）**: 提案どおり embedded-preferred を第一候補とし、なければ ImageIO のサムネイル生成を使う。Linux コンテナでは Rust のフォールバック実装で JPEG・PNG・WebP を扱い、HEIC は非対応エラーとする。ADR の状態は提案のままとする。

提案: macOSでは `embedded-preferred` を第一候補とし、埋め込みサムネイルがない場合は `imageio-thumbnail` へfallbackする。全体デコード後の縮小は、ImageIOサムネイル生成が失敗した場合の最後の診断用fallbackに限定する。Rust側はこの方式をデコーダトレイトの背後に置き、非macOSでは別実装へ差し替える。採用の決定は人間が行う。

## ライセンス・参照情報

- Apple ImageIO / Core Graphics: macOSシステムフレームワーク
- 追加クレート: `objc2-image-io`、`objc2-core-graphics`、`objc2-core-foundation`（いずれも Zlib OR Apache-2.0 OR MIT）。macOS だけで使い、`crates/imaging/src/imageio.rs` から ImageIO と Core Graphics を呼ぶ
- 実装（2026-10-29）: HEIC は `imageio-thumbnail` 方式で読み込む。`kCGImageSourceCreateThumbnailWithTransform` で向きを補正し、最大辺 2048px（詳細サムネイルの大きさ）で読み込む。埋め込みサムネイル優先の方式は、埋め込みサムネイルが詳細表示の 2048px に足りるかを確かめていないため、まだ使っていない。以前の `sips` で PNG に変換する方式と比べて、`eval-data/images` の HEIC 30枚で 1枚あたり 1,516ms から 93ms になった（どちらも依存クレートを最適化した開発用ビルド）
