# 評価セットの準備

T0-3 の埋め込みモデル比較で使う、ローカル評価セットの書式を定義します。
評価用の写真とラベルはリポジトリへ追加せず、ユーザーのMac上に置きます。Codexは、ユーザーから伝えられた `manifest.json` のパスを読み込んで計測します。

## ディレクトリ構成

以下のテンプレートをコピーして、同じディレクトリに置きます。

```text
evaluation-set/
├── manifest.json
├── images.csv
├── queries.csv
├── relevance.csv
├── tags.csv
└── custom-labels.csv
```

`templates/` に空のテンプレートがあります。

## manifest.json

`manifest.json` は評価セットの入口です。`image_root` は画像が入っているローカルディレクトリの絶対パス、その他のファイルは `manifest.json` からの相対パスにします。

```json
{
  "schema_version": "0.1",
  "image_root": "/Users/example/Pictures/picker-eval",
  "images": "images.csv",
  "queries": "queries.csv",
  "relevance": "relevance.csv",
  "tags": "tags.csv",
  "custom_labels": "custom-labels.csv",
  "unlisted_label": 0,
  "random_seed": 42
}
```

`image_root` の配下にある画像だけを使用します。評価セット外のファイルを参照するパスは書きません。

## CSV共通仕様

- UTF-8、1行目はヘッダー、区切り文字はカンマ
- 文字列にカンマ・改行・ダブルクォートが含まれる場合はRFC 4180形式で引用する
- `image_id`、`query_id`、`tag_id` は評価セット内で一意にする
- `images.csv` の `path` は `image_root` からの相対パスにする
- 画像ファイル名やラベルに個人情報が含まれる場合は、必要に応じて `notes` にも記録しない
- 空欄は「未設定」、`0` は「該当しない」として扱う

## images.csv

画像一覧です。50枚程度を目安に、実写画像を選びます。形式はHEIC、JPEG、PNG、WebPを混在させても構いません。

| 列 | 必須 | 内容 |
|---|---:|---|
| `image_id` | ○ | 例：`img-0001` |
| `path` | ○ | `image_root` からの相対パス |
| `notes` |  | 撮影条件など、評価に必要な補足 |

## queries.csv

自然言語検索用の日本語クエリを10個程度用意します。モデル比較の前に文言を変更しないでください。

| 列 | 必須 | 内容 |
|---|---:|---|
| `query_id` | ○ | 例：`q-0001` |
| `query_ja` | ○ | 実際に入力する日本語クエリ |
| `query_en` |  | 英語フォールバック比較用。不要なら空欄 |
| `reviewed_all_images` | ○ | 全画像を確認済みなら `true` |
| `notes` |  | クエリの意図 |

`reviewed_all_images` が `true` でないクエリは、検索品質の正式な比較対象にしません。

## relevance.csv

クエリと画像の関連度です。全組み合わせを記録する必要はなく、記載のない組み合わせは `manifest.json` の `unlisted_label`（通常は0）として扱います。ただし、各クエリについて全画像を確認し、該当する画像を漏れなく記録してください。

| 列 | 必須 | 内容 |
|---|---:|---|
| `query_id` | ○ | `queries.csv` のID |
| `image_id` | ○ | `images.csv` のID |
| `relevance` | ○ | `0` 非該当、`1` 該当、`2` 特に強く該当 |
| `notes` |  | 判定理由 |

同じ `query_id` と `image_id` の組み合わせは1行だけにします。各クエリには、可能なら関連度2を1枚以上、関連度1を2枚以上含めます。

## tags.csv

ゼロショット自動タグ用の小さなタグ語彙です。まずは検索クエリと重複しない実写向けのタグを10〜20個程度用意します。

| 列 | 必須 | 内容 |
|---|---:|---|
| `tag_id` | ○ | 例：`tag-0001` |
| `tag_ja` | ○ | 日本語タグ |
| `tag_en` |  | 英語タグまたは英語プロンプト用語 |
| `category` |  | 例：`scene`、`object`、`activity` |
| `reviewed_all_images` | ○ | 全画像を確認済みなら `true` |
| `notes` |  | タグの境界や注意点 |

## custom-labels.csv

カスタムタグの簡易分類器を比較するための正例・負例です。タグごとに `train` と `test` の画像を分離します。テスト画像は学習側に含めません。

| 列 | 必須 | 内容 |
|---|---:|---|
| `tag_id` | ○ | `tags.csv` のID |
| `image_id` | ○ | `images.csv` のID |
| `label` | ○ | `1` 正例、`0` 負例 |
| `split` | ○ | `train` または `test` |
| `notes` |  | 判定理由 |

各カスタムタグについて、少なくとも学習側に正例3枚以上、テスト側に正例と負例を用意します。`train` と `test` の画像IDは重複させません。

## 準備後の確認

ユーザーが記入を終えたら、以下をCodexに伝えます。

1. `manifest.json` の絶対パス
2. 評価セットを全画像について確認したこと
3. 個人写真をリポジトリへ追加していないこと

Codexは、読み込み検証で不足列、重複ID、存在しない画像、未確認クエリ、学習・テスト間の重複を検出します。検証に失敗した場合、T0-3の正式な計測は開始しません。
