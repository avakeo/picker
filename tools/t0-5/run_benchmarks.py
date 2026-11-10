#!/usr/bin/env python3
"""Run all T0-5 cases. Large generated vectors and SQLite files remain temporary."""
import argparse,csv,json,resource,sqlite3,subprocess,sys,tempfile,time
from pathlib import Path
import numpy as np
FIELDS=["method","dimensions","count","precision","p50_ms","p95_ms","load_ms","storage_bytes","peak_rss_bytes","top10_recall_vs_f32","insert_ms","update_ms","delete_ms","restart_ms","status"]
def pct(v,p):return float(np.percentile(np.asarray(v),p,method="nearest"))
def rss():
 """Peak RSS of this process in bytes. Each SQLite case runs in its own
 process (see sqlite_case), so the value belongs to that case only, as for
 the Rust binary. macOS reports bytes, Linux kibibytes."""
 peak=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
 return peak if sys.platform=="darwin" else peak*1024
def read_top(p):return [[int(x)for x in line.split(",",1)[1].split(";")]for line in Path(p).read_text().splitlines()]
def recall(a,e):return sum(len(set(x)&set(y))/len(y)for x,y in zip(a,e))/len(e)
def normalized(rng,n,d):
 x=rng.standard_normal((n,d),dtype=np.float32);x/=np.linalg.norm(x,axis=1,keepdims=True);return x
def rust(binary,vectors,queries,d,n,precision,warmups,k,tmp):
 out=tmp/f"rust-{precision}.csv";tops=tmp/f"rust-{precision}.top";subprocess.run([str(binary),"--input",str(vectors),"--queries",str(queries),"--output",str(out),"--top-output",str(tops),"--dimensions",str(d),"--count",str(n),"--query-count","20","--warmups",str(warmups),"--top-k",str(k),"--precision",precision],check=True)
 with out.open()as f:row=next(csv.DictReader(f))
 return row,read_top(tops)
def connect(path):
 import sqlite_vec
 db=sqlite3.connect(path);db.enable_load_extension(True);sqlite_vec.load(db);db.enable_load_extension(False);return db
def sqlite_case(vectors,queries,d,n,precision,warmups,k,tmp):
 """Runs one SQLite case in a new process. Like the Rust binary, the child
 reads the vectors from the files, so input loading counts on both sides."""
 out=tmp/f"sqlite-{precision}.json"
 subprocess.run([sys.executable,__file__,"--sqlite-case","--vectors",str(vectors),"--queries",str(queries),"--dimensions",str(d),"--count",str(n),"--precision",precision,"--warmups",str(warmups),"--top-k",str(k),"--db",str(tmp/f"sqlite-{precision}.db"),"--output",str(out)],check=True)
 result=json.loads(out.read_text());return result["row"],result["tops"]
def sqlite_child(a):
 d,n=a.dimensions_one,a.count
 base=np.fromfile(a.vectors,dtype=np.float32).reshape(n,d);queries=np.fromfile(a.queries,dtype=np.float32).reshape(-1,d)
 row,tops=sqlite_measure(base,queries,d,n,a.precision,a.warmups,a.top_k,Path(a.db))
 Path(a.output).write_text(json.dumps({"row":row,"tops":tops}))
def sqlite_measure(base,queries,d,n,precision,warmups,k,path):
 data=base if precision=="f32"else base.astype(np.float16).astype(np.float32);qs=queries if precision=="f32"else queries.astype(np.float16).astype(np.float32);db=connect(path);db.execute(f"CREATE VIRTUAL TABLE items USING vec0(embedding float[{d}])")
 s=time.perf_counter()
 with db:
  for start in range(0,n,500):db.executemany("INSERT INTO items(rowid,embedding) VALUES (?,?)",((i+1,memoryview(data[i]))for i in range(start,min(start+500,n))))
 insert=(time.perf_counter()-s)*1000;sample=min(100,n);s=time.perf_counter()
 with db:db.executemany("UPDATE items SET embedding=? WHERE rowid=?",((memoryview(data[i]),i+1)for i in range(sample)))
 update=(time.perf_counter()-s)*1000;s=time.perf_counter()
 with db:
  db.executemany("DELETE FROM items WHERE rowid=?",((i+1,)for i in range(sample)));db.executemany("INSERT INTO items(rowid,embedding) VALUES (?,?)",((i+1,memoryview(data[i]))for i in range(sample)))
 delete=(time.perf_counter()-s)*1000;db.execute("PRAGMA wal_checkpoint(TRUNCATE)");db.close();s=time.perf_counter();db=connect(path);db.execute("SELECT count(*) FROM items").fetchone();restart=(time.perf_counter()-s)*1000;sql="SELECT rowid FROM items WHERE embedding MATCH ? AND k = ? ORDER BY distance"
 for i in range(warmups):db.execute(sql,(memoryview(qs[i%len(qs)]),k)).fetchall()
 times=[];tops=[]
 for q in qs:
  s=time.perf_counter();rows=db.execute(sql,(memoryview(q),k)).fetchall();times.append((time.perf_counter()-s)*1000);tops.append([r[0]-1 for r in rows])
 db.close();return{"method":"sqlite-vec","dimensions":d,"count":n,"precision":"f32"if precision=="f32"else"f16-expanded-f32","p50_ms":f"{pct(times,50):.3f}","p95_ms":f"{pct(times,95):.3f}","load_ms":f"{restart:.3f}","storage_bytes":path.stat().st_size,"peak_rss_bytes":rss(),"insert_ms":f"{insert:.3f}","update_ms":f"{update:.3f}","delete_ms":f"{delete:.3f}","restart_ms":f"{restart:.3f}","status":"measured"},tops
def main():
 p=argparse.ArgumentParser();p.add_argument("--output",required=True);p.add_argument("--dimensions",default="512,768,1152");p.add_argument("--sizes",default="10000,50000,100000");p.add_argument("--warmups",type=int,default=3);p.add_argument("--top-k",type=int,default=10);p.add_argument("--binary")
 # Internal: one SQLite case in a child process.
 p.add_argument("--sqlite-case",action="store_true",help=argparse.SUPPRESS);p.add_argument("--vectors",help=argparse.SUPPRESS);p.add_argument("--queries",help=argparse.SUPPRESS);p.add_argument("--count",type=int,help=argparse.SUPPRESS);p.add_argument("--precision",help=argparse.SUPPRESS);p.add_argument("--db",help=argparse.SUPPRESS)
 a=p.parse_args()
 if a.sqlite_case:
  a.dimensions_one=int(a.dimensions);sqlite_child(a);return
 if not a.binary:p.error("--binary is required")
 rows=[]
 with tempfile.TemporaryDirectory(prefix="picker-t0-5-")as td:
  tmp=Path(td)
  for d in map(int,a.dimensions.split(",")):
   for n in map(int,a.sizes.split(",")):
    print(f"case dimensions={d} count={n}",flush=True);rng=np.random.default_rng(0x5049434B4552^d^n);base=normalized(rng,n,d);queries=normalized(rng,20,d);vf=tmp/"vectors.bin";qf=tmp/"queries.bin";base.tofile(vf);queries.tofile(qf)
    f32,btop=rust(Path(a.binary),vf,qf,d,n,"f32",a.warmups,a.top_k,tmp);f32["top10_recall_vs_f32"]="1.0000";rows.append(f32);f16,htop=rust(Path(a.binary),vf,qf,d,n,"f16",a.warmups,a.top_k,tmp);f16["top10_recall_vs_f32"]=f"{recall(htop,btop):.4f}";rows.append(f16)
    s32,stop=sqlite_case(vf,qf,d,n,"f32",a.warmups,a.top_k,tmp);s32["top10_recall_vs_f32"]=f"{recall(stop,btop):.4f}";rows.append(s32);s16,shtop=sqlite_case(vf,qf,d,n,"f16",a.warmups,a.top_k,tmp);s16["top10_recall_vs_f32"]=f"{recall(shtop,btop):.4f}";rows.append(s16);rows.append({"method":"sqlite-vec","dimensions":d,"count":n,"precision":"f16-native","status":"unsupported-by-sqlite-vec-0.1.9"})
    for x in tmp.iterdir():
     if x.is_file():x.unlink()
 with open(a.output,"w",newline="")as f:w=csv.DictWriter(f,fieldnames=FIELDS,extrasaction="ignore");w.writeheader();w.writerows(rows)
 print(f"wrote {a.output}")
if __name__=="__main__":main()
