//! Quick Test Suite - Fast Version
//!
//! This example runs a subset of the production testing suite
//! with reduced iterations for faster execution.

use anyhow::Result;
use spacekit_compute_node::{testing::ProductionTestingSuite, ComputeConfig, ComputeNode};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 SpaceKit Quick Test Suite v1.5");
    println!("===============================");
    println!("Running quick test suite (reduced iterations)...\n");

    // Step 1: Create compute node configuration
    println!("📋 Step 1: Creating compute node configuration...");
    let config = ComputeConfig::default();
    let compute_node = Arc::new(ComputeNode::new(config).await?);
    println!("✅ Compute node created successfully");

    // Step 2: Initialize testing suite
    println!("\n🔧 Step 2: Initializing testing suite...");
    let mut testing_suite = ProductionTestingSuite::new(compute_node).await?;
    println!("✅ Testing suite initialized");

    // Step 3: Run individual test components (faster than full suite)
    println!("\n🚀 Step 3: Running quick tests...");

    // Test 1: Integration tests only
    println!("  🔗 Running integration tests...");
    let integration_results = testing_suite.run_integration_tests().await?;
    println!(
        "  ✅ Integration tests completed: {}/{} passed",
        integration_results.passed_tests, integration_results.total_tests
    );

    // Test 2: Quick performance benchmarks (reduced iterations)
    println!("  ⚡ Running quick performance benchmarks...");
    let performance_results = testing_suite.run_performance_benchmarks().await?;
    println!("  ✅ Performance benchmarks completed");

    // Test 3: Quick stress tests
    println!("  💪 Running quick stress tests...");
    let stress_results = testing_suite.run_stress_tests().await?;
    println!("  ✅ Stress tests completed");

    // Test 4: Consensus tests
    println!("  🏛️ Running consensus tests...");
    let consensus_results = testing_suite.run_consensus_tests().await?;
    println!(
        "  ✅ Consensus tests completed: {}/{} passed",
        consensus_results.passed_tests, consensus_results.total_tests
    );

    // Step 4: Display summary results
    println!("\n📊 Quick Test Results Summary:");
    println!("{}", "=".repeat(60));

    // Integration Tests
    println!("🔗 Integration Tests:");
    println!("  Total Tests: {}", integration_results.total_tests);
    println!("  Passed: {}", integration_results.passed_tests);
    println!("  Failed: {}", integration_results.failed_tests.len());

    if !integration_results.failed_tests.is_empty() {
        println!("  Failed Tests:");
        for failure in &integration_results.failed_tests {
            println!("    ❌ {}", failure);
        }
    }

    // Performance Benchmarks
    println!("\n⚡ Performance Benchmarks:");
    let perf = &performance_results;
    println!(
        "  Service Discovery: {:.2}ms avg",
        perf.service_discovery_latency.average_latency_ms
    );
    println!(
        "  Load Balancing: {:.2}ms overhead",
        perf.load_balancing_overhead.average_latency_ms
    );
    println!(
        "  Health Checks: {:.2}ms avg",
        perf.health_check_latency.average_latency_ms
    );
    println!(
        "  Storage Throughput: {:.2} ops/sec",
        perf.storage_throughput.throughput_ops_sec
    );
    println!(
        "  Quantum Encryption: {:.2}ms avg",
        perf.quantum_encryption_overhead.average_latency_ms
    );

    // Stress Test Results
    println!("\n💪 Stress Test Results:");
    let stress = &stress_results;
    println!(
        "  Max Concurrent Operations: {}",
        stress.max_concurrent_operations
    );
    println!(
        "  Failover Scenarios Tested: {}",
        stress.failover_scenarios_tested
    );
    println!(
        "  Reputation System Load: {} ops",
        stress.reputation_system_load_operations
    );
    println!(
        "  Quantum Encryption Scale: {} ops",
        stress.quantum_encryption_scale_operations
    );

    // Consensus Test Results
    println!("\n🏛️ Consensus Test Results:");
    println!("  Total Tests: {}", consensus_results.total_tests);
    println!("  Passed: {}", consensus_results.passed_tests);
    println!("  Failed: {}", consensus_results.failed_tests.len());
    println!(
        "  Consensus Latency: {:.2}ms",
        consensus_results.consensus_benchmarks.consensus_latency_ms
    );
    println!(
        "  Consensus Throughput: {:.2} TPS",
        consensus_results
            .consensus_benchmarks
            .consensus_throughput_tps
    );
    println!(
        "  Proposal Success Rate: {:.1}%",
        consensus_results.consensus_benchmarks.proposal_success_rate
    );

    // Overall success calculation
    let total_tests = integration_results.total_tests + consensus_results.total_tests;
    let total_passed = integration_results.passed_tests + consensus_results.passed_tests;
    let overall_success = total_passed == total_tests
        && integration_results.failed_tests.is_empty()
        && consensus_results.failed_tests.is_empty();

    println!("\n{}", "=".repeat(60));
    println!(
        "Overall Success: {}",
        if overall_success {
            "✅ PASS"
        } else {
            "❌ FAIL"
        }
    );
    println!(
        "Total Tests: {} | Passed: {} | Failed: {}",
        total_tests,
        total_passed,
        total_tests - total_passed
    );

    if overall_success {
        println!("🎉 Quick test suite completed successfully!");
    } else {
        println!(
            "⚠️  Some tests failed. Consider running the full test suite for detailed analysis."
        );
    }

    Ok(())
}
