use super::{Case, Result, Value, json, root};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

const COUNT: usize = 4;

pub fn parse(value: &str) -> Result<usize> {
    let number = value.parse::<usize>()?;
    if !(1..=COUNT).contains(&number) { return Err("shard must be between 1 and 4".into()); }
    Ok(number)
}

pub fn number(id: &str) -> usize {
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    (hash % COUNT as u64) as usize + 1
}

pub fn write_report(path: &Path, report: &Value) -> Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    serde_json::to_writer(&mut output, report)?;
    output.flush()?;
    Ok(())
}

fn assemble(reports: Vec<Value>, cases: &[Case], metadata: &Value, kind: &str) -> Result<Value> {
    if reports.len() != COUNT { return Err("incomplete shard set".into()); }
    let mut results = BTreeMap::new();
    for (index, mut report) in reports.into_iter().enumerate() {
        if report["metadata"] != *metadata || report["candidate_kind"] != kind { return Err("stale shard evidence".into()); }
        let shard_results = std::mem::take(report["results"].as_array_mut().ok_or("invalid shard results")?);
        for result in shard_results {
            let id = result["case"]["id"].as_str().ok_or("missing shard case ID")?;
            let id = id.to_owned();
            if number(&id) != index + 1 || results.insert(id, result).is_some() {
                return Err("duplicate or misplaced shard case".into());
            }
        }
    }
    let mut ordered = Vec::with_capacity(cases.len());
    for case in cases {
        let result = results.remove(&case.id).ok_or("incomplete shard result accounting")?;
        if result["case"] != serde_json::to_value(case)? { return Err("stale shard case".into()); }
        ordered.push(result);
    }
    if !results.is_empty() { return Err("unknown shard case".into()); }
    Ok(Value::Object(serde_json::Map::from_iter([
        ("metadata".into(), metadata.clone()),
        ("candidate_kind".into(), Value::String(kind.into())),
        ("results".into(), Value::Array(ordered)),
    ])))
}

pub fn record(shard: usize, directory: &Path, cases: &[Case], metadata: &Value, baseline: bool) -> Result<()> {
    let name = if baseline { "baseline" } else { "current" };
    let kind = if baseline { "baseline-2ae5660" } else { "current" };
    fs::write(root().join(format!(".work/parity-{name}-shard-{shard}")), directory.as_os_str().as_encoded_bytes())?;
    let mut reports = Vec::new();
    for index in 1..=COUNT {
        let pointer = root().join(format!(".work/parity-{name}-shard-{index}"));
        let directory = match fs::read_to_string(pointer) {
            Ok(directory) => PathBuf::from(directory),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let report: Value = serde_json::from_reader(BufReader::new(File::open(directory.join("results.json"))?))?;
        if report["metadata"] != *metadata || report["candidate_kind"] != kind { return Ok(()); }
        reports.push(report);
    }
    let report = assemble(reports, cases, metadata, kind)?;
    let combined = super::stdio::world()?.keep();
    write_report(&combined.join("results.json"), &report)?;
    fs::write(root().join(format!(".work/parity-{name}")), combined.as_os_str().as_encoded_bytes())?;
    println!("complete {name} corpus assembled: {}", combined.display());
    Ok(())
}

pub fn selftest() -> Result<()> {
    let cases = super::cases()?;
    let cases = &cases[..cases.len().min(32)];
    let metadata = json!({"version":1});
    let mut reports: Vec<_> = (0..COUNT).map(|_| json!({"metadata":metadata,"candidate_kind":"current","results":[]})).collect();
    for case in cases {
        reports[number(&case.id) - 1]["results"].as_array_mut().unwrap().push(json!({"case":case,"outcome":"pass"}));
    }
    let result = assemble(reports.clone(), cases, &metadata, "current")?;
    let ids: Vec<_> = result["results"].as_array().unwrap().iter().map(|result| result["case"]["id"].as_str().unwrap()).collect();
    if ids != cases.iter().map(|case| case.id.as_str()).collect::<Vec<_>>() { return Err("shard assembly changed corpus order".into()); }
    let mut incomplete = reports.clone();
    let occupied = incomplete.iter_mut().find_map(|report| {
        let entries = report["results"].as_array_mut().unwrap();
        (!entries.is_empty()).then_some(entries)
    }).unwrap();
    occupied.pop();
    if assemble(incomplete, cases, &metadata, "current").is_ok() { return Err("incomplete shard evidence was accepted".into()); }
    reports[0]["metadata"] = json!({"version":2});
    if assemble(reports, cases, &metadata, "current").is_ok() { return Err("stale shard evidence was accepted".into()); }
    println!("fixed-ID shards preserve complete corpus order and reject stale evidence");
    Ok(())
}
