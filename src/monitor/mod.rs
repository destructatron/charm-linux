mod cpu;
mod disk;
mod memory;

pub use cpu::CpuMonitor;
pub use disk::DiskMonitor;
pub use memory::MemoryMonitor;

/// Represents a normalized metric value between 0.0 and 1.0
#[derive(Debug, Clone, Copy, Default)]
pub struct MetricValue(f64);

impl MetricValue {
    pub fn new(value: f64) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    pub fn get(&self) -> f64 {
        self.0
    }
}

/// Combined system metrics snapshot
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    /// Per-core CPU usage (0.0 to 1.0 each)
    pub cpu_cores: Vec<MetricValue>,
    /// Averaged CPU usage across all cores
    pub cpu_average: MetricValue,
    /// RAM usage percentage
    pub memory: MetricValue,
    /// Disk activity level
    pub disk: MetricValue,
}

/// Central monitor that collects all system metrics
pub struct SystemMonitor {
    cpu: CpuMonitor,
    memory: MemoryMonitor,
    disk: DiskMonitor,
}

impl SystemMonitor {
    pub fn new() -> Self {
        Self {
            cpu: CpuMonitor::new(),
            memory: MemoryMonitor::new(),
            disk: DiskMonitor::new(),
        }
    }

    /// Refresh all metrics and return a snapshot
    pub fn refresh(&mut self) -> SystemMetrics {
        self.cpu.refresh();
        self.memory.refresh();
        self.disk.refresh();

        SystemMetrics {
            cpu_cores: self.cpu.per_core_usage(),
            cpu_average: self.cpu.average_usage(),
            memory: self.memory.usage(),
            disk: self.disk.activity(),
        }
    }

    /// Returns the number of CPU cores
    pub fn core_count(&self) -> usize {
        self.cpu.core_count()
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}
