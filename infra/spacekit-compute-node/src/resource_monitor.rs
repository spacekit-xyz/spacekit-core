//! Resource Monitoring Module
//!
//! Provides real-time system resource monitoring for accurate cost calculation

use anyhow::Result;
use std::process;
use std::time::{Duration, Instant};
use sysinfo::{Cpu, Pid, Process, System};
use tokio::time::sleep;

/// Resource monitor that tracks system metrics
pub struct ResourceMonitor {
    system: System,
    start_time: Instant,
    start_memory: u64,
    start_cpu_usage: f32,
    process_id: u32,
}

#[derive(Debug, Clone)]
pub struct ResourceMetrics {
    pub cpu_usage_percent: f32,
    pub memory_usage_mb: u64,
    pub memory_peak_mb: u64,
    pub cpu_time_ms: u64,
    pub execution_time_ms: u64,
    pub energy_consumed_kwh: f64,
    pub compute_units_used: u64,
}

impl ResourceMonitor {
    pub fn new() -> Result<Self> {
        let mut system = System::new_all();
        system.refresh_all();

        let process_id = process::id();
        let start_time = Instant::now();

        // Get initial metrics
        let (start_memory, start_cpu_usage) =
            if let Some(process) = system.process(Pid::from_u32(process_id)) {
                (process.memory(), process.cpu_usage())
            } else {
                (0, 0.0)
            };

        Ok(Self {
            system,
            start_time,
            start_memory,
            start_cpu_usage,
            process_id,
        })
    }

    pub async fn start_monitoring(&mut self) -> Result<()> {
        // Refresh system information
        self.system.refresh_all();
        self.start_time = Instant::now();

        // Get initial metrics
        if let Some(process) = self.system.process(Pid::from_u32(self.process_id)) {
            self.start_memory = process.memory();
            self.start_cpu_usage = process.cpu_usage();
        }

        Ok(())
    }

    pub async fn get_current_metrics(&mut self) -> Result<ResourceMetrics> {
        // Refresh system information
        self.system.refresh_all();

        let execution_time = self.start_time.elapsed();
        let execution_time_ms = execution_time.as_millis() as u64;

        // Get current process metrics
        let (current_memory, current_cpu_usage, memory_peak) =
            if let Some(process) = self.system.process(Pid::from_u32(self.process_id)) {
                (
                    process.memory(),
                    process.cpu_usage(),
                    process.memory().max(self.start_memory),
                )
            } else {
                (self.start_memory, self.start_cpu_usage, self.start_memory)
            };

        // Calculate metrics
        let memory_usage_mb = current_memory / 1024 / 1024;
        let memory_peak_mb = memory_peak / 1024 / 1024;
        let cpu_usage_percent = current_cpu_usage;

        // Estimate CPU time based on usage and execution time
        let cpu_time_ms = (execution_time_ms as f32 * cpu_usage_percent / 100.0) as u64;

        // Estimate energy consumption (simplified model)
        let energy_consumed_kwh =
            self.estimate_energy_consumption(cpu_usage_percent, memory_usage_mb, execution_time_ms);

        // Calculate compute units (simplified based on CPU time and memory)
        let compute_units_used = cpu_time_ms + (memory_usage_mb * 10);

        Ok(ResourceMetrics {
            cpu_usage_percent,
            memory_usage_mb,
            memory_peak_mb,
            cpu_time_ms,
            execution_time_ms,
            energy_consumed_kwh,
            compute_units_used,
        })
    }

    pub async fn monitor_execution<F, R>(&mut self, execution_fn: F) -> Result<(R, ResourceMetrics)>
    where
        F: std::future::Future<Output = Result<R>>,
    {
        // Start monitoring
        self.start_monitoring().await?;

        // Execute the function
        let result = execution_fn.await?;

        // Get final metrics
        let metrics = self.get_current_metrics().await?;

        Ok((result, metrics))
    }

    fn estimate_energy_consumption(
        &self,
        cpu_usage: f32,
        memory_mb: u64,
        execution_time_ms: u64,
    ) -> f64 {
        // Simplified energy consumption model
        // Based on typical desktop CPU power consumption

        // Base power consumption (watts)
        let base_power = 5.0; // 5W baseline

        // CPU power consumption (proportional to usage)
        let cpu_power = (cpu_usage / 100.0) * 50.0; // Max 50W for CPU

        // Memory power consumption (simplified)
        let memory_power = (memory_mb as f64 / 1024.0) * 2.0; // ~2W per GB

        // Total power in watts
        let total_power = base_power + cpu_power as f64 + memory_power;

        // Energy in kWh
        let execution_time_hours = execution_time_ms as f64 / (1000.0 * 3600.0);
        let energy_kwh = total_power * execution_time_hours / 1000.0;

        energy_kwh
    }

    pub fn get_system_info(&mut self) -> SystemInfo {
        self.system.refresh_all();

        // Calculate average CPU usage
        let cpu_usage_percent = self
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage())
            .sum::<f32>()
            / self.system.cpus().len() as f32;

        SystemInfo {
            total_memory_mb: self.system.total_memory() / 1024 / 1024,
            used_memory_mb: self.system.used_memory() / 1024 / 1024,
            total_cpu_cores: self.system.cpus().len() as u32,
            cpu_usage_percent,
            available_memory_mb: self.system.available_memory() / 1024 / 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub total_cpu_cores: u32,
    pub cpu_usage_percent: f32,
    pub available_memory_mb: u64,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| {
            // Fallback if system monitoring fails
            Self {
                system: System::new(),
                start_time: Instant::now(),
                start_memory: 0,
                start_cpu_usage: 0.0,
                process_id: process::id(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_resource_monitor() {
        let mut monitor = ResourceMonitor::new().unwrap();

        // Start monitoring
        monitor.start_monitoring().await.unwrap();

        // Simulate some work
        sleep(Duration::from_millis(100)).await;

        // Get metrics
        let metrics = monitor.get_current_metrics().await.unwrap();

        assert!(metrics.execution_time_ms >= 100);
        assert!(metrics.memory_usage_mb > 0);
    }

    #[tokio::test]
    async fn test_monitor_execution() {
        let mut monitor = ResourceMonitor::new().unwrap();

        // Monitor an async function
        let (result, metrics) = monitor
            .monitor_execution(async {
                sleep(Duration::from_millis(50)).await;
                Ok::<String, anyhow::Error>("test result".to_string())
            })
            .await
            .unwrap();

        assert_eq!(result, "test result");
        assert!(metrics.execution_time_ms >= 50);
    }
}
