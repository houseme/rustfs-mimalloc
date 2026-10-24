use crate::{
    release::checksum,
    util::{self, Args},
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const ORDER: [&str; 4] = ["A1", "B1", "B2", "A2"];
const CASES: [&str; 4] = [
    "alloc_dealloc/mimalloc/64",
    "alloc_dealloc/mimalloc/4096",
    "alloc_dealloc/mimalloc/65536",
    "cross_thread_free_batch/mimalloc/4096",
];

fn criterion_estimate(output: &str, case: &str) -> Result<Value> {
    let value = r"([0-9.]+)\s+(ns|µs|us|ms)";
    let pattern = Regex::new(&format!(
        r"{}\s+time:\s+\[{}\s+{}\s+{}\]",
        regex::escape(case),
        value,
        value,
        value
    ))?;
    let captures = pattern
        .captures(output)
        .with_context(|| format!("missing estimate for {case}"))?;
    let mut numbers = Vec::new();
    for index in [1, 3, 5] {
        let scale = match &captures[index + 1] {
            "ns" => 1.0,
            "us" | "µs" => 1000.0,
            "ms" => 1_000_000.0,
            _ => unreachable!(),
        };
        let value = captures[index].parse::<f64>()? * scale;
        ensure!(
            value.is_finite() && value > 0.0,
            "invalid benchmark estimate"
        );
        numbers.push(value);
    }
    Ok(json!({"lower_ns": numbers[0], "estimate_ns": numbers[1], "upper_ns": numbers[2]}))
}

fn comparison(values: [f64; 4], limit: f64, quality: bool, throughput: bool) -> Result<Value> {
    ensure!(
        values.iter().all(|v| v.is_finite() && *v > 0.0),
        "benchmark estimates must be finite and positive"
    );
    let [a1, b1, b2, a2] = values;
    let drift_a = (a2 / a1 - 1.0).abs();
    let drift_b = (b2 / b1 - 1.0).abs();
    let passed = quality && drift_a <= limit && drift_b <= limit;
    if throughput {
        Ok(
            json!({"baseline_drift":drift_a,"candidate_drift":drift_b,"gate":passed,
            "throughput_change":passed.then_some((b1+b2)/(a1+a2)-1.0)}),
        )
    } else {
        Ok(
            json!({"baseline_drift":drift_a,"candidate_drift":drift_b,"gate_passed":passed,
            "time_reduction":passed.then_some(1.0-(b1+b2)/(a1+a2))}),
        )
    }
}

fn unmodified_environment(reject_cflags: bool) -> Result<()> {
    let overrides: Vec<_> = std::env::vars_os()
        .filter(|(key, value)| {
            let key = key.to_string_lossy();
            key.starts_with("MIMALLOC_")
                || (reject_cflags && key.contains("CFLAGS") && !value.is_empty())
        })
        .map(|(key, _)| key)
        .collect();
    ensure!(
        overrides.is_empty(),
        "remove allocator/compiler environment overrides: {overrides:?}"
    );
    Ok(())
}

pub fn criterion(args: &mut Args, profile: bool) -> Result<()> {
    let output = util::path(args.value(
        "--output",
        if profile {
            "target/profile-abba"
        } else {
            "target/heap-abba"
        },
    )?);
    let offline = args.flag("--offline");
    let measurement: f64 = args.value("--measurement-time", "5")?.parse()?;
    let warmup: f64 = args.value("--warm-up-time", "3")?.parse()?;
    let samples: u64 = args.value("--sample-size", "50")?.parse()?;
    let limit: f64 = args.value("--drift-limit", "0.05")?.parse()?;
    args.finish()?;
    ensure!(
        samples >= 10
            && measurement.is_finite()
            && measurement > 0.0
            && warmup.is_finite()
            && warmup > 0.0,
        "use >=10 samples and positive finite measurement/warm-up times"
    );
    ensure!(
        limit.is_finite() && limit > 0.0 && limit < 1.0,
        "drift-limit must be between 0 and 1"
    );
    unmodified_environment(profile)?;
    util::fresh_directory(&output, true)?;
    let root = util::root();
    let mut report = json!({
        "commit":util::git(["rev-parse","HEAD"])?,
        "mimalloc_commit":util::git(["-C","rustfs-mimalloc-sys/c_src/mimalloc","rev-parse","HEAD"])?,
        "dirty":!util::git(["status","--porcelain"])?.is_empty(),
        "rustc":util::capture(Command::new("rustc").arg("--version"))?.trim(),
        "parameters":{"measurement_seconds":measurement,"warm_up_seconds":warmup,"samples":samples,"drift_limit":limit,"analysis_threads":1},
        "builds":{},"rounds":{},"comparisons":{}
    });
    let mut binaries = BTreeMap::new();
    for label in if profile { vec!["A", "B"] } else { vec!["A"] } {
        let mut command = util::cargo(&[], &root);
        command.args([
            "bench",
            "-p",
            "rustfs-mimalloc",
            "--bench",
            "alloc",
            "--no-run",
            "--locked",
            "--message-format=json",
        ]);
        if offline {
            command.arg("--offline");
        }
        if label == "B" {
            command.args(["--features", "no_profile"]);
        }
        let output_text = util::capture(&mut command)?;
        let artifacts: Vec<Value> = output_text
            .lines()
            .filter(|line| line.starts_with('{'))
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        let artifact = artifacts
            .iter()
            .find(|item| item["executable"].is_string() && item["target"]["name"] == "alloc")
            .context("missing alloc benchmark executable")?;
        let destination = output.join(format!("{label}{}", std::env::consts::EXE_SUFFIX));
        fs::copy(artifact["executable"].as_str().unwrap(), &destination)?;
        report["builds"][label] = json!({"features":artifact["features"],"binary_bytes":fs::metadata(&destination)?.len()});
        if !profile {
            report["features"] = artifact["features"].clone();
        }
        binaries.insert(label, destination);
    }
    for label in ORDER {
        let variant = if label.starts_with('A') {
            "lookup"
        } else {
            "cached"
        };
        let cases: Vec<String> = if profile {
            CASES.iter().map(|s| (*s).to_owned()).collect()
        } else {
            [64, 4096]
                .iter()
                .map(|size| format!("heap_switching/{variant}/{size}"))
                .collect()
        };
        let pattern = format!(
            "^({})$",
            cases
                .iter()
                .map(|s| regex::escape(s))
                .collect::<Vec<_>>()
                .join("|")
        );
        let binary = if profile && label.starts_with('B') {
            &binaries["B"]
        } else {
            &binaries["A"]
        };
        let mut command = Command::new(binary);
        command
            .current_dir(&root)
            .env("RAYON_NUM_THREADS", "1")
            .args([
                "--bench",
                &pattern,
                "--noplot",
                "--sample-size",
                &samples.to_string(),
                "--warm-up-time",
                &warmup.to_string(),
                "--measurement-time",
                &measurement.to_string(),
            ]);
        let result = util::output(&mut command)?;
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        fs::write(output.join(format!("{label}.log")), &log)?;
        ensure!(
            result.status.success(),
            "benchmark failed; inspect {label}.log"
        );
        for case in cases {
            let key = if profile {
                case.clone()
            } else {
                case.rsplit('/').next().unwrap().to_owned()
            };
            if report["rounds"][label].is_null() {
                report["rounds"][label] = json!({});
            }
            report["rounds"][label][key] = criterion_estimate(&log, &case)?;
        }
        println!("{label}: {}", report["rounds"][label]);
    }
    let cases = report["rounds"]["A1"]
        .as_object()
        .context("missing A1 estimates")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut passed = true;
    for case in cases {
        let mut values = [0.0; 4];
        for (index, label) in ORDER.iter().enumerate() {
            values[index] = report["rounds"][label][&case]["estimate_ns"]
                .as_f64()
                .context("missing estimate")?;
        }
        let result = comparison(values, limit, true, false)?;
        passed &= result["gate_passed"] == true;
        report["comparisons"][case] = result;
    }
    util::write_json(&output.join("results.json"), &report)?;
    println!("{}", report["comparisons"]);
    ensure!(
        passed,
        "drift gate failed: retain evidence and make no speedup claim"
    );
    Ok(())
}

fn workload_metrics(output: &str) -> Result<Value> {
    let fields: BTreeMap<_, _> = output
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let number = |name: &str| -> Result<u64> {
        fields
            .get(name)
            .with_context(|| format!("missing {name}"))?
            .parse()
            .with_context(|| format!("invalid {name}"))
    };
    let reported = number("reported_samples")?;
    let dropped = number("dropped_samples")?;
    let pending = number("buffered_samples")?;
    let total = reported as f64 + dropped as f64 + pending as f64;
    let fraction = if total > 0.0 {
        dropped as f64 / total
    } else {
        1.0
    };
    let throughput: f64 = fields
        .get("allocations_per_sec")
        .context("missing throughput")?
        .parse()?;
    ensure!(
        throughput.is_finite() && throughput > 0.0,
        "invalid throughput"
    );
    Ok(json!({"throughput":throughput,"drop_fraction":fraction,
        "quality":total>0.0 && fraction<=0.01 && pending==0 && number("dropped_live_samples")?==0 && number("periodic_reports")?>=2,
        "report_max_us":number("report_max_elapsed_us")?,
        "latency_p99_ns":fields.get("allocation_latency_p99_ns").map(|s| s.parse::<u64>()).transpose()?.unwrap_or(0)}))
}

fn affinity() -> Result<Vec<usize>> {
    ensure!(
        cfg!(target_os = "linux"),
        "run on Linux with affinity restricted to two available CPUs"
    );
    let status = fs::read_to_string("/proc/self/status")?;
    let list = status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
        .context("missing CPU affinity")?
        .trim();
    let mut cpus = Vec::new();
    for range in list.split(',') {
        if let Some((start, end)) = range.split_once('-') {
            let start: usize = start.parse()?;
            let end: usize = end.parse()?;
            ensure!(
                end >= start && end - start < 4096,
                "invalid CPU affinity range"
            );
            cpus.extend(start..=end);
        } else {
            cpus.push(range.parse()?);
        }
    }
    ensure!(cpus.len() == 2, "restrict affinity to two available CPUs");
    Ok(cpus)
}

fn workload(binary: &Path, environment: &BTreeMap<String, String>) -> Result<String> {
    let result = util::output(Command::new(binary).envs(environment))?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    ensure!(result.status.success(), "workload failed: {log}");
    Ok(log)
}

pub fn profiling(args: &mut Args) -> Result<()> {
    let baseline = util::path(args.required("--baseline")?);
    let candidate = util::path(args.required("--candidate")?);
    let output = util::path(args.required("--output")?);
    let duration: u64 = args.value("--duration-ms", "8000")?.parse()?;
    let mut cases = Vec::new();
    while let Some(case) = args.option("--case")? {
        cases.push(case);
    }
    args.finish()?;
    ensure!(duration > 0, "duration-ms must be positive");
    let affinity = affinity()?;
    unmodified_environment(false)?;
    ensure!(
        baseline.is_file() && candidate.is_file(),
        "both frozen executables must exist"
    );
    let definitions: [(&str, &[(&str, &str)]); 5] = [
        ("alloc-default", &[]),
        ("alloc-default-2", &[("MIMALLOC_BENCH_WORKERS", "2")]),
        (
            "alloc-dense",
            &[
                ("MIMALLOC_BENCH_SAMPLE_INTERVAL", "1024"),
                ("MIMALLOC_BENCH_MIN_SIZE", "65536"),
                ("MIMALLOC_BENCH_MAX_SIZE", "65536"),
            ],
        ),
        ("live-default", &[("MIMALLOC_BENCH_MODE", "live")]),
        (
            "live-low-rate",
            &[
                ("MIMALLOC_BENCH_MODE", "live"),
                ("MIMALLOC_BENCH_SAMPLE_INTERVAL", "16777216"),
                ("MIMALLOC_BENCH_REPORT_INTERVAL_MS", "200"),
                ("MIMALLOC_BENCH_LIVE_RETAINED_SIZE", "1048576"),
            ],
        ),
    ];
    ensure!(
        cases
            .iter()
            .all(|case| definitions.iter().any(|(name, _)| name == case)),
        "unknown profiling case"
    );
    if cases.is_empty() {
        cases = definitions.iter().map(|(name, _)| (*name).into()).collect();
    }
    util::fresh_directory(&output, false)?;
    let base: BTreeMap<String, String> = [
        ("MIMALLOC_BENCH_MODE", "active".into()),
        ("MIMALLOC_BENCH_DURATION_MS", duration.to_string()),
        ("MIMALLOC_BENCH_SAMPLE_INTERVAL", "1048576".into()),
        ("MIMALLOC_BENCH_STACK_CAPTURE", "portable".into()),
        ("MIMALLOC_BENCH_WORKERS", "1".into()),
        ("MIMALLOC_BENCH_REPORT_INTERVAL_MS", "25".into()),
        ("MIMALLOC_BENCH_RING_CAPACITY", "65536".into()),
        ("MIMALLOC_BENCH_REPORT_DRAIN_LIMIT", "131072".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    let mut report = json!({"affinity":affinity,"environment":base,"cases":{},"binaries":{
        "A":{"path":baseline,"sha256":checksum(&fs::read(&baseline)?)},
        "B":{"path":candidate,"sha256":checksum(&fs::read(&candidate)?)}
    }});
    let mut passed = true;
    for case in cases {
        let overrides: BTreeMap<String, String> = definitions
            .iter()
            .find(|(name, _)| name == &case)
            .unwrap()
            .1
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect();
        let mut environment = base.clone();
        environment.extend(overrides.clone());
        let mut warmup = environment.clone();
        warmup.insert("MIMALLOC_BENCH_DURATION_MS".into(), "1000".into());
        for binary in [&baseline, &candidate] {
            workload(binary, &warmup)?;
        }
        let mut rounds = json!({});
        for label in ORDER {
            let log = workload(
                if label.starts_with('A') {
                    &baseline
                } else {
                    &candidate
                },
                &environment,
            )?;
            fs::write(output.join(format!("{case}-{label}.log")), &log)?;
            rounds[label] = workload_metrics(&log)?;
        }
        let mut values = [0.0; 4];
        for (index, label) in ORDER.iter().enumerate() {
            values[index] = rounds[label]["throughput"]
                .as_f64()
                .context("missing throughput")?;
        }
        let comparison = comparison(
            values,
            0.05,
            ORDER.iter().all(|label| rounds[label]["quality"] == true),
            true,
        )?;
        passed &= comparison["gate"] == true;
        let mut row = comparison;
        row["overrides"] = json!(overrides);
        row["rounds"] = rounds;
        report["cases"][&case] = row;
        util::write_json(&output.join("results.json"), &report)?;
        println!("{case}: {}", report["cases"][&case]);
    }
    ensure!(
        passed,
        "drift/loss gate failed: retain evidence and make no speedup claim"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn criterion_units_and_drift_controls_preserve_null_conclusions() -> Result<()> {
        let value = criterion_estimate("example\n time: [1.0 µs 2.0 us 0.003 ms]", "example")?;
        assert_eq!(value["estimate_ns"], 2000.0);
        assert_eq!(value["upper_ns"], 3000.0);
        assert!(criterion_estimate("missing", "example").is_err());
        assert!(comparison([100., 80., 80., 120.], 0.05, true, false)?["time_reduction"].is_null());
        assert!(
            comparison([100., 110., 110., 100.], 0.05, false, true)?["throughput_change"].is_null()
        );
        assert_eq!(
            comparison([100., 80., 80., 100.], 0.05, true, false)?["gate_passed"],
            true
        );
        Ok(())
    }
    #[test]
    fn profiler_quality_rejects_loss_pending_and_missing_reports() -> Result<()> {
        let base = "reported_samples=1000\ndropped_samples=0\nbuffered_samples=0\ndropped_live_samples=0\nperiodic_reports=2\nreport_max_elapsed_us=30\nallocations_per_sec=1000";
        assert_eq!(workload_metrics(base)?["quality"], true);
        for (from, to) in [
            ("dropped_samples=0", "dropped_samples=100"),
            ("buffered_samples=0", "buffered_samples=1"),
            ("dropped_live_samples=0", "dropped_live_samples=1"),
            ("periodic_reports=2", "periodic_reports=1"),
        ] {
            assert_eq!(workload_metrics(&base.replace(from, to))?["quality"], false);
        }
        Ok(())
    }
}
