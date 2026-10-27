use std::cmp::Ordering;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::process::Command;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Precision { F32, F16 }
impl Precision {
    fn parse(v: &str) -> Result<Self,String> { match v { "f32"=>Ok(Self::F32), "f16"=>Ok(Self::F16), _=>Err(format!("invalid precision: {v}")) } }
    fn name(self)->&'static str { match self { Self::F32=>"f32", Self::F16=>"f16" } }
}
struct Options { input:String, queries:String, output:String, top_output:String, dimensions:usize, count:usize, query_count:usize, warmups:usize, top_k:usize, precision:Precision }
fn value(args:&[String], name:&str)->Result<String,String>{let i=args.iter().position(|a|a==name).ok_or_else(||format!("missing {name}"))?;args.get(i+1).cloned().ok_or_else(||format!("missing value for {name}"))}
fn parse_options()->Result<Options,String>{
    let a:Vec<String>=env::args().skip(1).collect(); let n=|s:&str| value(&a,s)?.parse().map_err(|_|format!("invalid {s}"));
    let o=Options{input:value(&a,"--input")?,queries:value(&a,"--queries")?,output:value(&a,"--output")?,top_output:value(&a,"--top-output")?,dimensions:n("--dimensions")?,count:n("--count")?,query_count:n("--query-count")?,warmups:n("--warmups")?,top_k:n("--top-k")?,precision:Precision::parse(&value(&a,"--precision")?)?};
    if o.dimensions==0||o.count==0||o.query_count==0||o.top_k==0{return Err("numeric options must be positive".into())} Ok(o)
}
fn f32_to_f16(v:f32)->u16{let b=v.to_bits();let s=((b>>16)&0x8000)as u16;let e=((b>>23)&0xff)as i32-112;let m=b&0x7f_ffff;if e<=0{if e< -10{return s}let x=(m|0x80_0000)>>(1-e);return s|((x+0x1000)>>13)as u16}if e>=31{return s|0x7c00|if m==0{0}else{0x0200}}s|((e as u16)<<10)|(((m+0x1000)>>13)as u16&0x03ff)}
fn f16_to_f32(b:u16)->f32{let s=((b&0x8000)as u32)<<16;let e=(b>>10)&0x1f;let m=(b&0x03ff)as u32;let x=if e==0{if m==0{s}else{let(mut m,mut e)=(m,-14i32);while m&0x0400==0{m<<=1;e-=1}s|(((e+127)as u32)<<23)|((m&0x03ff)<<13)}}else if e==0x1f{s|0x7f80_0000|(m<<13)}else{s|(((e as u32)+112)<<23)|(m<<13)};f32::from_bits(x)}
fn read_f32(path:&str,expected:usize)->io::Result<Vec<f32>>{let b=fs::read(path)?;if b.len()!=expected*4{return Err(io::Error::new(io::ErrorKind::InvalidData,format!("expected {} bytes, got {}",expected*4,b.len())))}Ok(b.chunks_exact(4).map(|v|f32::from_le_bytes([v[0],v[1],v[2],v[3]])).collect())}
fn top_k(scores:&[f32],k:usize)->Vec<usize>{let mut ids:Vec<_>=(0..scores.len()).collect();ids.sort_unstable_by(|a,b|scores[*b].partial_cmp(&scores[*a]).unwrap_or(Ordering::Equal).then_with(||a.cmp(b)));ids.truncate(k.min(ids.len()));ids}
fn percentile(v:&[f64],p:f64)->f64{let mut x=v.to_vec();x.sort_by(|a,b|a.partial_cmp(b).unwrap_or(Ordering::Equal));x[((p*x.len()as f64).ceil()as usize).saturating_sub(1).min(x.len()-1)]}
fn rss()->u64{Command::new("ps").args(["-o","rss=","-p",&std::process::id().to_string()]).output().ok().and_then(|o|String::from_utf8(o.stdout).ok()).and_then(|s|s.trim().parse::<u64>().ok()).unwrap_or(0)*1024}
fn main()->Result<(),Box<dyn std::error::Error>>{
    let o=parse_options().map_err(io::Error::other)?;let start=Instant::now();let vf=read_f32(&o.input,o.count*o.dimensions)?;let qf=read_f32(&o.queries,o.query_count*o.dimensions)?;
    let(vh,qh)=match o.precision{Precision::F32=>(vec![],vec![]),Precision::F16=>(vf.iter().copied().map(f32_to_f16).collect(),qf.iter().copied().map(f32_to_f16).collect())};let load_ms=start.elapsed().as_secs_f64()*1000.;let mut peak=rss();
    let scan=|qi:usize|->Vec<f32>{match o.precision{Precision::F32=>{let q=&qf[qi*o.dimensions..(qi+1)*o.dimensions];vf.chunks_exact(o.dimensions).map(|v|v.iter().zip(q).map(|(a,b)|a*b).sum()).collect()},Precision::F16=>{let q=&qh[qi*o.dimensions..(qi+1)*o.dimensions];vh.chunks_exact(o.dimensions).map(|v|v.iter().zip(q).map(|(a,b)|f16_to_f32(*a)*f16_to_f32(*b)).sum()).collect()}}};
    for i in 0..o.warmups{let _=scan(i%o.query_count);}let mut times=vec![];let mut tops=vec![];for i in 0..o.query_count{let s=Instant::now();let scores=scan(i);let ids=top_k(&scores,o.top_k);times.push(s.elapsed().as_secs_f64()*1000.);tops.push(ids);peak=peak.max(rss())}
    let storage=o.count*o.dimensions*match o.precision{Precision::F32=>4,Precision::F16=>2};let mut f=File::create(&o.output)?;writeln!(f,"method,dimensions,count,precision,p50_ms,p95_ms,load_ms,storage_bytes,peak_rss_bytes,status")?;writeln!(f,"self-full-scan,{},{},{},{:.3},{:.3},{:.3},{},{},measured",o.dimensions,o.count,o.precision.name(),percentile(&times,0.5),percentile(&times,0.95),load_ms,storage,peak)?;let mut t=File::create(&o.top_output)?;for(i,ids)in tops.iter().enumerate(){writeln!(t,"{},{}",i,ids.iter().map(usize::to_string).collect::<Vec<_>>().join(";"))?}Ok(())
}
#[cfg(test)]mod tests{use super::*;#[test]fn half(){for v in[-1.0,-0.1,0.0,0.1,1.0]{assert!((f16_to_f32(f32_to_f16(v))-v).abs()<0.001)}}#[test]fn p95(){assert_eq!(percentile(&[1.0,2.0,3.0,4.0,5.0],0.95),5.0)}#[test]fn ties(){assert_eq!(top_k(&[1.0,2.0,2.0],2),vec![1,2])}}
