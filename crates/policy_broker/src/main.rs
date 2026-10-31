//! `talos-policy-broker` binary. `--selftest` exercises the engine
//! plus a throwaway SQLite database and prints a PASS/FAIL summary.

use policy_broker::run_selftest;
use talos_common::init_tracing;

#[tokio::main]
async fn main() {
    if let Err(err) = init_tracing() {
        eprintln!("tracing init failed: {err}");
    }
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--selftest") {
        let report = run_selftest().await;
        println!(
            "talos-policy-broker selftest: {}/{} passed",
            report.passed, report.total
        );
        for (name, ok, detail) in &report.checks {
            println!("  [{}] {name}: {detail}", if *ok { "PASS" } else { "FAIL" });
        }
        if !report.ok() {
            std::process::exit(1);
        }
        return;
    }
    println!("talos-policy-broker: pass --selftest to run the engine selftest");
}
