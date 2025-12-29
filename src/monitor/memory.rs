use sysinfo::System;

use super::MetricValue;

pub struct MemoryMonitor {
    system: System,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_memory();
        Self { system }
    }

    pub fn refresh(&mut self) {
        self.system.refresh_memory();
    }

    /// Returns memory usage as a value between 0.0 and 1.0
    pub fn usage(&self) -> MetricValue {
        let total = self.system.total_memory();
        let used = self.system.used_memory();

        if total == 0 {
            return MetricValue::new(0.0);
        }

        MetricValue::new(used as f64 / total as f64)
    }

    /// Returns total memory in bytes
    pub fn total_bytes(&self) -> u64 {
        self.system.total_memory()
    }

    /// Returns used memory in bytes
    pub fn used_bytes(&self) -> u64 {
        self.system.used_memory()
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}
