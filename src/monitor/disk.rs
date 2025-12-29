use std::fs;
use std::time::Instant;

use super::MetricValue;

/// Monitors disk I/O activity by reading /proc/diskstats
pub struct DiskMonitor {
    last_read_sectors: u64,
    last_write_sectors: u64,
    last_time: Instant,
    /// Sectors per second at last measurement
    activity_level: f64,
    /// Maximum observed activity for normalization
    max_activity: f64,
}

impl DiskMonitor {
    /// Minimum activity threshold to avoid division by very small numbers
    const MIN_MAX_ACTIVITY: f64 = 1000.0;

    pub fn new() -> Self {
        let (read_sectors, write_sectors) = Self::read_disk_stats();
        Self {
            last_read_sectors: read_sectors,
            last_write_sectors: write_sectors,
            last_time: Instant::now(),
            activity_level: 0.0,
            max_activity: Self::MIN_MAX_ACTIVITY,
        }
    }

    pub fn refresh(&mut self) {
        let (read_sectors, write_sectors) = Self::read_disk_stats();
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64();

        if elapsed > 0.0 {
            let read_delta = read_sectors.saturating_sub(self.last_read_sectors);
            let write_delta = write_sectors.saturating_sub(self.last_write_sectors);
            let total_delta = read_delta + write_delta;

            // Sectors per second
            self.activity_level = total_delta as f64 / elapsed;

            // Update max for normalization (with decay to adapt to changing workloads)
            if self.activity_level > self.max_activity {
                self.max_activity = self.activity_level;
            } else {
                // Slow decay of max activity
                self.max_activity = (self.max_activity * 0.999).max(Self::MIN_MAX_ACTIVITY);
            }
        }

        self.last_read_sectors = read_sectors;
        self.last_write_sectors = write_sectors;
        self.last_time = now;
    }

    /// Returns disk activity as a normalized value between 0.0 and 1.0
    pub fn activity(&self) -> MetricValue {
        MetricValue::new(self.activity_level / self.max_activity)
    }

    /// Read total sectors read/written from /proc/diskstats
    fn read_disk_stats() -> (u64, u64) {
        let content = match fs::read_to_string("/proc/diskstats") {
            Ok(c) => c,
            Err(_) => return (0, 0),
        };

        let mut total_read = 0u64;
        let mut total_write = 0u64;

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 14 {
                continue;
            }

            let device_name = parts[2];

            // Skip partitions (e.g., sda1) and only count whole devices (e.g., sda)
            // Also skip loop devices and ram disks
            if device_name.starts_with("loop")
                || device_name.starts_with("ram")
                || device_name.starts_with("dm-")
            {
                continue;
            }

            // Check if this is a partition (ends with a number after letters)
            let is_partition = device_name
                .chars()
                .last()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && device_name
                    .chars()
                    .rev()
                    .skip(1)
                    .next()
                    .map(|c| c.is_alphabetic())
                    .unwrap_or(false);

            if is_partition {
                continue;
            }

            // Field 6 is sectors read, field 10 is sectors written (0-indexed from field 3)
            if let (Ok(read), Ok(write)) = (parts[5].parse::<u64>(), parts[9].parse::<u64>()) {
                total_read += read;
                total_write += write;
            }
        }

        (total_read, total_write)
    }
}

impl Default for DiskMonitor {
    fn default() -> Self {
        Self::new()
    }
}
