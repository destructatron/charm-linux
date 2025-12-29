use sysinfo::System;

use super::MetricValue;

pub struct CpuMonitor {
    system: System,
}

impl CpuMonitor {
    pub fn new() -> Self {
        let mut system = System::new();
        // Initial refresh to get baseline
        system.refresh_cpu_usage();
        Self { system }
    }

    pub fn refresh(&mut self) {
        self.system.refresh_cpu_usage();
    }

    /// Returns CPU usage for each core as a value between 0.0 and 1.0
    pub fn per_core_usage(&self) -> Vec<MetricValue> {
        self.system
            .cpus()
            .iter()
            .map(|cpu| MetricValue::new(cpu.cpu_usage() as f64 / 100.0))
            .collect()
    }

    /// Returns average CPU usage across all cores
    pub fn average_usage(&self) -> MetricValue {
        let cpus = self.system.cpus();
        if cpus.is_empty() {
            return MetricValue::new(0.0);
        }

        let total: f32 = cpus.iter().map(|cpu| cpu.cpu_usage()).sum();
        MetricValue::new((total / cpus.len() as f32) as f64 / 100.0)
    }

    /// Returns the number of CPU cores
    pub fn core_count(&self) -> usize {
        self.system.cpus().len()
    }
}

impl Default for CpuMonitor {
    fn default() -> Self {
        Self::new()
    }
}
