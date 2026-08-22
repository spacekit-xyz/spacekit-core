//! Example: Running the Complete Production Testing Suite
//!
//! This example shows how to run the comprehensive testing suite
//! that includes integration tests, performance benchmarks, stress tests,
//! and consensus testing.

use anyhow::Result;
use spacekit_compute_node::{testing::ProductionTestingSuite, ComputeConfig, ComputeNode};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🧪 SpaceKit Production Testing Suite v1.5");
    println!("=======================================");
    println!("Running complete test suite with unified consensus testing...\n");

    // Step 1: Create compute node configuration
    println!("📋 Step 1: Creating compute node configuration...");
    let config = ComputeConfig::default();
    let compute_node = Arc::new(ComputeNode::new(config).await?);
    println!("✅ Compute node created successfully");

    // Step 2: Initialize testing suite
    println!("\n🔧 Step 2: Initializing production testing suite...");
    let mut testing_suite = ProductionTestingSuite::new(compute_node).await?;
    println!("✅ Testing suite initialized with unified consensus components");

    // Step 3: Run complete test suite
    println!("\n🚀 Step 3: Running complete test suite...");
    let report = testing_suite.run_complete_test_suite().await?;

    // Step 4: Display results
    println!("\n📊 Test Results Summary:");
    println!("{}", "=".repeat(60));
    println!("Total Duration: {}ms", report.total_duration_ms);
    println!(
        "Overall Success: {}",
        if report.overall_success {
            "✅ PASS"
        } else {
            "❌ FAIL"
        }
    );

    // Integration Tests
    println!("\n🔗 Integration Tests:");
    println!("  Total Tests: {}", report.integration_results.total_tests);
    println!("  Passed: {}", report.integration_results.passed_tests);
    println!(
        "  Failed: {}",
        report.integration_results.failed_tests.len()
    );

    if !report.integration_results.failed_tests.is_empty() {
        println!("  Failed Tests:");
        for failure in &report.integration_results.failed_tests {
            println!("    ❌ {}", failure);
        }
    }

    // Performance Benchmarks
    println!("\n⚡ Performance Benchmarks:");
    let perf = &report.performance_results;
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
    let stress = &report.stress_results;
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

    // Consensus Test Results (if available)
    if let Some(consensus_results) = &report.consensus_results {
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
    }

    // Recommendations
    println!("\n💡 Recommendations:");
    for (i, recommendation) in report.recommendations.iter().enumerate() {
        println!("  {}. {}", i + 1, recommendation);
    }

    println!("\n{}", "=".repeat(60));

    if report.overall_success {
        println!("🎉 All tests passed successfully!");
        Ok(())
    } else {
        println!("⚠️  Some tests failed. Please review the results above.");
        Ok(())
    }
}
