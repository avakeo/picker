# T0-3 評価データとブラインド採点

T0-3は、候補モデルの検索結果をプーリングし、候補名を隠してユーザーが0/1/2で採点する。写真と採点結果は `/Users/runner/work/picker/eval-data/` に置き、Gitには追加しない。

事前に全画像へ検索・タグの正解ラベルを付けない。事前ラベルを残すのはカスタムタグ3〜5個のtrain/testだけとする。

## ディレクトリ

```text
eval-data/
├── manifest.json
├── images.csv
├── images/
├── queries.csv
├── tags.csv
├── similarity-queries.csv
├── custom-labels.csv
├── pool-rankings.csv        # 候補実行後。run_idは匿名ID
├── candidate-map.csv        # run_idと候補名の対応。採点HTMLには埋め込まない
├── blind-scoring.html       # 生成物
├── blind-map.csv            # 採点IDと元項目の対応
└── blind-scores.csv         # ブラウザから書き出す採点結果
```

テンプレートは `docs/eval/templates/` にある。`eval-data/` は `.gitignore` で除外する。

## manifest.json

```json
{
  "schema_version": "0.2",
  "image_root": "/Users/runner/work/picker/eval-data/images",
  "images": "images.csv",
  "queries": "queries.csv",
  "tags": "tags.csv",
  "similarity_queries": "similarity-queries.csv",
  "custom_labels": "custom-labels.csv",
  "random_seed": 42
}
```

## 固定する入力

### images.csv

`image_id,path,notes`。写真は当初50枚、最終的に300〜500枚を想定する。`path` は `image_root` からの相対パスにする。

### queries.csv

`query_id,query_ja,query_en,notes`。日本語クエリを20件以上用意する。`query_en` は参考値の英語fallbackにだけ使い、主評価へ加点しない。

### tags.csv

`tag_id,tag_ja,tag_en,category,notes`。設計書8.3の語彙ファイルを想定し、20〜30語を計測前に固定する。ゼロショット評価では候補ごとの上位20件をpoolし、採点結果からタグ別APとmean APを算出する。

### similarity-queries.csv

`query_image_id,image_sha256,notes`。類似検索のクエリ画像を15枚程度、最低10枚固定する。`image_sha256` は画像バイト列のSHA-256で、検証スクリプトが実ファイルと照合する。

### custom-labels.csv

`tag_id,image_id,label,split,notes`。カスタムタグは3〜5個だけを対象にし、タグごとにtrain正例3枚以上、test正例・負例を用意する。train/testで同じ画像を使わない。

## 計測前の固定とハッシュ

`queries.csv`、`tags.csv`、`similarity-queries.csv` は、ユーザー確定後に個人情報やローカルパスを含まないことを確認してコミットする。類似クエリの画像本体はコミットせず、画像IDとSHA-256だけを固定する。

検証コマンドは各CSVと結合定義のSHA-256を出力する。ADR-001には結合定義ハッシュを記録する。

```sh
python3 tools/t0-3/validate_eval_set.py \
  --manifest /Users/runner/work/picker/eval-data/manifest.json
```

最低件数は、クエリ20、タグ20、類似クエリ画像10、カスタムタグ3。件数、画像ハッシュ、参照ID、train/test分離のどれかが不正なら計測を開始しない。

## プーリング

候補ごとの順位は `pool-rankings.csv` に次の形式で保存する。

```csv
task,prompt_id,run_id,rank,image_id
natural_language,q-0001,R001,1,img-0001
zero_shot,tag-0001,R001,1,img-0002
similarity,img-0003,R001,1,img-0004
```

- 自然言語検索: 各候補の上位20件を合併
- ゼロショットタグ: タグごとに各候補の上位20件を合併
- 類似画像検索: クエリ画像ごとに各候補の上位10件を合併
- `run_id` は `R001` のような匿名IDにし、モデル名を含めない
- モデル名との対応は `candidate-map.csv` にだけ保存する

ユーザーがクエリ・タグ・類似クエリ画像を固定する前に、写真に対する順位を生成・閲覧・保存しない。

## ブラインド採点HTML

```sh
python3 tools/t0-3/build_blind_scoring.py \
  --eval-root /Users/runner/work/picker/eval-data
```

生成物は `eval-data/blind-scoring.html` と `eval-data/blind-map.csv` のみ。HTMLは外部通信・CDNを使わず、候補名とrun IDを埋め込まない。画面とHTMLソース、書き出しファイル名から候補を推測できないようにする。

ブラウザでHTMLを開き、キーボードの0/1/2で採点する。採点はlocalStorageへ自動保存され、再度開くと続きから再開できる。「CSVを書き出す」で `blind-scores.csv` を保存する。

採点項目の約10%は、同一項目を別の位置へ重複して出題する。重複で同じ点を付けた割合を自己一致率として記録する。

## 指標

- 自然言語検索: 未採点を0としてnDCG@10
- ゼロショットタグ: 1または2を関連ありとしてAP、タグ単位の平均をmean AP
- 類似画像検索: pool内で1または2の画像を関連ありとしてRecall@10、0/1/2を使ってnDCG@10
- カスタムタグ: 固定train/testのmacro F1
- 候補差: クエリ・タグ・類似クエリ画像単位のブートストラップ95%信頼区間。0をまたぐ場合は有意差なしと記録

点数化前の候補別・クエリ別指標、信頼区間、自己一致率を生データとして保存する。重みや仮上限は後から再計算できるよう、生データへ埋め込まない。
