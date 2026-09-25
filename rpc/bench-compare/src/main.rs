mod types;

use std::collections::BTreeMap;
use types::{BenchResult, ScenarioResult};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: bench-compare <baseline.json> <candidate.json>");
        std::process::exit(2);
    }

    let baseline = load_results(&args[1]);
    let candidate = load_results(&args[2]);

    let baseline_label = variant_label(&baseline);
    let candidate_label = variant_label(&candidate);

    println!(
        "Comparing: {} ({}) vs {} ({})\n",
        args[1], baseline_label, args[2], candidate_label
    );

    let mut regression = false;

    let candidate_map: BTreeMap<String, &ScenarioResult> = candidate
        .scenarios
        .iter()
        .filter(|s| s.params.get("measurement").and_then(|v| v.as_str()) != Some("run"))
        .map(|s| (s.name.clone(), s))
        .collect();

    let baseline_map: BTreeMap<String, &ScenarioResult> = baseline
        .scenarios
        .iter()
        .filter(|s| s.params.get("measurement").and_then(|v| v.as_str()) != Some("run"))
        .map(|s| (s.name.clone(), s))
        .collect();

    let mut all_keys: Vec<String> = baseline_map
        .keys()
        .chain(candidate_map.keys())
        .cloned()
        .collect();
    all_keys.sort();
    all_keys.dedup();

    println!(
        "{:<42} {:>10} {:>12} {:>12} {:>8}",
        "scenario", "metric", baseline_label, candidate_label, "delta"
    );
    println!("{}", "-".repeat(86));

    for key in &all_keys {
        let b = baseline_map.get(key);
        let c = candidate_map.get(key);

        match (b, c) {
            (Some(b), Some(c)) => {
                regression |= print_comparison(b, c);
            }
            (Some(b), None) => {
                print_one_sided(&b.name, b, &baseline_label);
            }
            (None, Some(c)) => {
                print_one_sided(&c.name, c, &candidate_label);
            }
            (None, None) => unreachable!(),
        }
    }

    println!();
    if regression {
        println!("REGRESSION DETECTED (p99 latency +15% or throughput -10%)");
        std::process::exit(1);
    } else {
        println!("No regressions detected.");
    }
}

fn load_results(path: &str) -> BenchResult {
    let data = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Failed to read {path}: {e}");
        std::process::exit(2);
    });
    serde_json::from_str(&data).unwrap_or_else(|e| {
        eprintln!("Failed to parse {path}: {e}");
        std::process::exit(2);
    })
}

fn variant_label(r: &BenchResult) -> String {
    if let Some(policy) = &r.metadata.dispatch_policy {
        return policy.clone();
    }
    if r.metadata
        .feature_flags
        .contains(&"concurrent-dispatch".to_string())
    {
        "concurrent".to_string()
    } else {
        "serial".to_string()
    }
}

fn pct_delta(baseline: f64, candidate: f64) -> f64 {
    if baseline > 0.0 {
        (candidate - baseline) / baseline * 100.0
    } else {
        0.0
    }
}

fn print_comparison(b: &ScenarioResult, c: &ScenarioResult) -> bool {
    let mut regression = false;

    if b.timed_out && c.timed_out {
        println!(
            "{:<42} {:>10} {:>12} {:>12} {:>8}",
            b.name, "", "TIMEOUT", "TIMEOUT", "--"
        );
        return false;
    }
    if b.timed_out && !c.timed_out {
        println!(
            "{:<42} {:>10} {:>12} {:>12.1} {:>8}",
            b.name,
            "p99 µs",
            "TIMEOUT",
            c.latency_us.as_ref().unwrap().p99,
            "--"
        );
        return false;
    }
    if !b.timed_out && c.timed_out {
        println!(
            "{:<42} {:>10} {:>12.1} {:>12} {:>8}",
            b.name,
            "p99 µs",
            b.latency_us.as_ref().unwrap().p99,
            "TIMEOUT",
            "--"
        );
        return true;
    }

    let b_p99 = b.latency_us.as_ref().unwrap().p99;
    let c_p99 = c.latency_us.as_ref().unwrap().p99;
    let p99_delta = pct_delta(b_p99, c_p99);
    if p99_delta > 15.0 {
        regression = true;
    }
    println!(
        "{:<42} {:>10} {:>12.1} {:>12.1} {:>+7.1}%",
        b.name, "p99 µs", b_p99, c_p99, p99_delta
    );

    if let (Some(b_tp), Some(c_tp)) = (b.throughput_calls_per_sec, c.throughput_calls_per_sec) {
        let tp_delta = pct_delta(b_tp, c_tp);
        if tp_delta < -10.0 {
            regression = true;
        }
        println!(
            "{:<42} {:>10} {:>12.0} {:>12.0} {:>+7.1}%",
            "", "calls/s", b_tp, c_tp, tp_delta
        );
    }

    regression
}

fn print_one_sided(name: &str, s: &ScenarioResult, label: &str) {
    if s.timed_out {
        println!("{:<42} {:>10} {:>12}   ({label} only)", name, "", "TIMEOUT");
        return;
    }
    let lat = s.latency_us.as_ref().unwrap();
    println!(
        "{:<42} {:>10} {:>12.1}   ({label} only)",
        name, "p99 µs", lat.p99
    );
    if let Some(tp) = s.throughput_calls_per_sec {
        println!("{:<42} {:>10} {:>12.0}   ({label} only)", "", "calls/s", tp);
    }
}
