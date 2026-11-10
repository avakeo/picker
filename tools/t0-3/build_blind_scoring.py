#!/usr/bin/env python3
"""Build a standalone, offline blind-scoring page inside eval-data."""

from __future__ import annotations

import argparse
import csv
import hashlib
import html
import json
import random
from pathlib import Path
from urllib.parse import quote

from validate_eval_set import ValidationError, read_csv, validate


TASK_LIMITS = {"natural_language": 20, "zero_shot": 20, "similarity": 10}


def image_uri(path: Path) -> str:
    return "file://" + quote(str(path.resolve()))


def read_manifest(path: Path) -> dict[str, object]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def build(eval_root: Path) -> tuple[Path, Path, int]:
    eval_root = eval_root.resolve()
    manifest_path = eval_root / "manifest.json"
    validate(manifest_path)
    manifest = read_manifest(manifest_path)
    image_root = Path(str(manifest["image_root"])).expanduser().resolve()

    images = read_csv(eval_root / str(manifest["images"]), {"image_id", "path", "notes"})
    queries = read_csv(eval_root / str(manifest["queries"]), {"query_id", "query_ja", "query_en", "notes"})
    tags = read_csv(eval_root / str(manifest["tags"]), {"tag_id", "tag_ja", "tag_en", "category", "notes"})
    rankings = read_csv(eval_root / "pool-rankings.csv", {"task", "prompt_id", "run_id", "rank", "image_id"})
    candidate_map = read_csv(eval_root / "candidate-map.csv", {"run_id", "candidate_id"})

    image_by_id = {row["image_id"]: row for row in images}
    query_by_id = {row["query_id"]: row for row in queries}
    tag_by_id = {row["tag_id"]: row for row in tags}
    run_ids = {row["run_id"] for row in candidate_map}
    candidate_names = {row["candidate_id"].strip() for row in candidate_map if row["candidate_id"].strip()}
    if len(run_ids) != len(candidate_map):
        raise ValidationError("candidate-map.csv contains duplicate run_id")

    pooled: dict[tuple[str, str, str], None] = {}
    for row in rankings:
        task = row["task"]
        if task not in TASK_LIMITS:
            raise ValidationError(f"pool-rankings.csv: unknown task {task}")
        if row["run_id"] not in run_ids:
            raise ValidationError("pool-rankings.csv references an unknown run_id")
        if row["image_id"] not in image_by_id:
            raise ValidationError("pool-rankings.csv references an unknown image_id")
        try:
            rank = int(row["rank"])
        except ValueError as error:
            raise ValidationError("pool-rankings.csv rank must be an integer") from error
        if not 1 <= rank <= TASK_LIMITS[task]:
            raise ValidationError(f"{task}: rank must be between 1 and {TASK_LIMITS[task]}")
        prompt_id = row["prompt_id"]
        if task == "natural_language" and prompt_id not in query_by_id:
            raise ValidationError("natural-language ranking references an unknown query_id")
        if task == "zero_shot" and prompt_id not in tag_by_id:
            raise ValidationError("zero-shot ranking references an unknown tag_id")
        if task == "similarity" and prompt_id not in image_by_id:
            raise ValidationError("similarity ranking references an unknown query image")
        pooled[(task, prompt_id, row["image_id"])] = None

    def visible_prompt(task: str, prompt_id: str) -> str:
        if task == "natural_language":
            return query_by_id[prompt_id]["query_ja"]
        if task == "zero_shot":
            return tag_by_id[prompt_id]["tag_ja"]
        return "この画像にどの程度似ていますか"

    seed = int(manifest["random_seed"])
    rng = random.Random(seed)
    base = list(pooled)
    duplicate_count = round(len(base) * 0.10)
    duplicates = rng.sample(base, min(duplicate_count, len(base)))
    expanded = [(key, 0) for key in base] + [(key, 1) for key in duplicates]
    rng.shuffle(expanded)

    items: list[dict[str, str]] = []
    map_rows: list[dict[str, str]] = []
    for key, occurrence in expanded:
        task, prompt_id, image_id = key
        canonical = f"{task}|{prompt_id}|{image_id}"
        blind_id = hashlib.sha256(f"{seed}|{canonical}|{occurrence}".encode()).hexdigest()[:20]
        duplicate_group = hashlib.sha256(f"{seed}|{canonical}".encode()).hexdigest()[:20]
        image_path = image_root / image_by_id[image_id]["path"]
        query_image = ""
        if task == "similarity":
            query_image = image_uri(image_root / image_by_id[prompt_id]["path"])
        items.append(
            {
                "blindId": blind_id,
                "task": task,
                "prompt": visible_prompt(task, prompt_id),
                "imageSrc": image_uri(image_path),
                "queryImageSrc": query_image,
            }
        )
        map_rows.append(
            {
                "blind_id": blind_id,
                "task": task,
                "prompt_id": prompt_id,
                "image_id": image_id,
                "duplicate_group": duplicate_group,
            }
        )

    assessment_id = hashlib.sha256(json.dumps(items, sort_keys=True).encode()).hexdigest()[:20]
    payload = json.dumps(items, ensure_ascii=False).replace("</", "<\\/")
    page = render_html(payload, assessment_id)
    lowered = page.lower()
    leaked = sorted(name for name in candidate_names | run_ids if name and name.lower() in lowered)
    if leaked:
        raise ValidationError(f"candidate identity leaked into HTML: {', '.join(leaked)}")

    html_path = eval_root / "blind-scoring.html"
    map_path = eval_root / "blind-map.csv"
    html_path.write_text(page, encoding="utf-8")
    with map_path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=["blind_id", "task", "prompt_id", "image_id", "duplicate_group"])
        writer.writeheader()
        writer.writerows(map_rows)
    return html_path, map_path, len(items)


def render_html(payload: str, assessment_id: str) -> str:
    return f"""<!doctype html>
<html lang="ja">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src file: data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'">
<title>ブラインド採点</title>
<style>
:root{{font-family:-apple-system,BlinkMacSystemFont,sans-serif;color:#f4f4f4;background:#171717}}
body{{margin:0}}main{{max-width:980px;margin:auto;padding:24px}}header{{display:flex;justify-content:space-between;gap:16px;align-items:center}}
.prompt{{font-size:24px;font-weight:650;margin:18px 0}}.pair{{display:grid;grid-template-columns:1fr 2fr;gap:16px;min-height:440px}}
.pair.single{{grid-template-columns:1fr}}figure{{margin:0;background:#242424;border-radius:10px;padding:12px;display:grid;place-items:center}}
img{{max-width:100%;max-height:65vh;object-fit:contain}}.scores{{display:flex;gap:12px;justify-content:center;margin:20px 0}}
button{{font:inherit;color:#fff;background:#303030;border:1px solid #666;border-radius:8px;padding:12px 20px;cursor:pointer}}
button:hover,button:focus{{border-color:#78a9ff}}.nav{{display:flex;justify-content:space-between;gap:12px}}.muted{{color:#aaa}}
</style>
</head>
<body><main>
<header><h1>ブラインド採点</h1><div id="progress" class="muted"></div></header>
<div id="prompt" class="prompt"></div>
<div id="pair" class="pair single"><figure id="queryBox" hidden><img id="queryImage" alt="類似検索のクエリ画像"></figure><figure><img id="resultImage" alt="採点対象画像"></figure></div>
<div class="scores"><button data-score="0">0: 非該当</button><button data-score="1">1: 該当</button><button data-score="2">2: 強く該当</button></div>
<div class="nav"><button id="previous">戻る</button><button id="export">CSVを書き出す</button><button id="next">次へ</button></div>
<p class="muted">0 / 1 / 2 キーで採点。回答はこのブラウザ内へ自動保存されます。</p>
</main>
<script>
const items={payload};
const storageKey='picker-blind-eval:{html.escape(assessment_id)}';
const saved=JSON.parse(localStorage.getItem(storageKey)||'{{}}');
let scores=saved.scores||{{}};
let index=Number.isInteger(saved.index)?Math.min(saved.index,items.length-1):Math.max(0,items.findIndex(x=>scores[x.blindId]===undefined));
const byId=id=>document.getElementById(id);
function persist(){{localStorage.setItem(storageKey,JSON.stringify({{index,scores}}));}}
function render(){{
 const item=items[index]; if(!item) return;
 byId('progress').textContent=`${{index+1}} / ${{items.length}}（採点済み ${{Object.keys(scores).length}}）`;
 byId('prompt').textContent=item.prompt;
 byId('resultImage').src=item.imageSrc;
 const similar=item.task==='similarity'; byId('queryBox').hidden=!similar; byId('pair').className=similar?'pair':'pair single';
 if(similar) byId('queryImage').src=item.queryImageSrc;
 persist();
}}
function setScore(value){{scores[items[index].blindId]=value;if(index<items.length-1)index++;persist();render();}}
document.querySelectorAll('[data-score]').forEach(button=>button.addEventListener('click',()=>setScore(Number(button.dataset.score))));
document.addEventListener('keydown',event=>{{if(['0','1','2'].includes(event.key))setScore(Number(event.key));}});
byId('previous').addEventListener('click',()=>{{index=Math.max(0,index-1);render();}});
byId('next').addEventListener('click',()=>{{index=Math.min(items.length-1,index+1);render();}});
byId('export').addEventListener('click',()=>{{
 const rows=['blind_id,score']; for(const item of items)rows.push(`${{item.blindId}},${{scores[item.blindId]??''}}`);
 const blob=new Blob([rows.join('\\n')+'\\n'],{{type:'text/csv;charset=utf-8'}});const url=URL.createObjectURL(blob);
 const link=document.createElement('a');link.href=url;link.download='blind-scores.csv';link.click();URL.revokeObjectURL(url);
}});
render();
</script></body></html>"""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--eval-root", type=Path, required=True)
    args = parser.parse_args()
    try:
        html_path, map_path, count = build(args.eval_root)
    except (OSError, json.JSONDecodeError, ValidationError) as error:
        print(f"build failed: {error}")
        return 1
    print(json.dumps({"html": str(html_path), "map": str(map_path), "items": count}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
