//! Background aggregation scheduler for performance metrics.
//!
//! This module provides a background task that periodically aggregates
//! performance metrics into hourly_stats and daily_stats tables.
//!
//! ## Usage
//!
//! ```no_run
//! use forge_cost::{CostDatabase, Aggregator};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = Arc::new(CostDatabase::open("~/.forge/costs.db")?);
//!
//!     // Start background aggregation (runs every 10 minutes)
//!     let aggregator = Aggregator::new(db);
//!     let handle = aggregator.start();
//!
//!     // ... do other work ...
//!
//!     // Stop aggregation when done
//!     handle.abort();
//!     Ok(())
//! }
//! ```

use crate::db::CostDatabase;
use crate::error::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Default aggregation interval in seconds (10 minutes).
pub const DEFAULT_AGGREGATION_INTERVAL_SECS: u64 = 600;

/// Background aggregator for performance metrics.
///
/// Periodically runs aggregation queries to update hourly_stats, daily_stats,
/// worker_efficiency, and model_performance tables.
pub struct Aggregator {
    db: Arc<CostDatabase>,
    interval: Duration,
}

impl Aggregator {
    /// Create a new aggregator with the default interval (10 minutes).
    pub fn new(db: Arc<CostDatabase>) -> Self {
        Self {
            db,
            interval: Duration::from_secs(DEFAULT_AGGREGATION_INTERVAL_SECS),
        }
    }

    /// Create a new aggregator with a custom interval.
    pub fn with_interval(db: Arc<CostDatabase>, interval: Duration) -> Self {
        Self { db, interval }
    }

    /// Start the background aggregation task.
    ///
    /// Returns a JoinHandle that can be used to abort the task.
    pub fn start(self) -> JoinHandle<()> {
        info!(
            interval_secs = self.interval.as_secs(),
            "Starting background aggregator"
        );

        tokio::spawn(async move {
            self.run_loop().await;
        })
    }

    /// Run the aggregation loop.
    async fn run_loop(&self) {
        let mut interval = tokio::time::interval(self.interval);

        // Run immediately on startup
        self.run_aggregation();

        loop {
            interval.tick().await;
            self.run_aggregation();
        }
    }

    /// Run a single aggregation cycle.
    pub fn run_aggregation(&self) {
        debug!("Running aggregation cycle");

        match self.db.run_background_aggregation() {
            Ok(()) => {
                debug!("Aggregation cycle completed successfully");
            }
            Err(e) => {
                error!("Aggregation cycle failed: {}", e);
            }
        }
    }

    /// Run aggregation once (synchronous, for manual triggering).
    pub fn run_once(&self) -> Result<()> {
        self.db.run_background_aggregation()
    }

    /// Get the aggregation interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }
}

/// Configuration for the aggregator.
#[derive(Debug, Clone)]
pub struct AggregatorConfig {
    /// Interval between aggregation runs.
    pub interval: Duration,

    /// Whether to run aggregation immediately on startup.
    pub run_on_startup: bool,

    /// Number of previous hours to re-aggregate (for catching up).
    pub catchup_hours: u32,
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(DEFAULT_AGGREGATION_INTERVAL_SECS),
            run_on_startup: true,
            catchup_hours: 2,
        }
    }
}

impl AggregatorConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the aggregation interval.
    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    /// Set whether to run on startup.
    pub fn with_run_on_startup(mut self, run_on_startup: bool) -> Self {
        self.run_on_startup = run_on_startup;
        self
    }

    /// Set the catchup hours.
    pub fn with_catchup_hours(mut self, catchup_hours: u32) -> Self {
        self.catchup_hours = catchup_hours;
        self
    }
}

/// Builder for creating an aggregator with configuration.
pub struct AggregatorBuilder {
    db: Arc<CostDatabase>,
    config: AggregatorConfig,
}

impl AggregatorBuilder {
    /// Create a new builder.
    pub fn new(db: Arc<CostDatabase>) -> Self {
        Self {
            db,
            config: AggregatorConfig::default(),
        }
    }

    /// Set the configuration.
    pub fn config(mut self, config: AggregatorConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the aggregation interval.
    pub fn interval(mut self, interval: Duration) -> Self {
        self.config.interval = interval;
        self
    }

    /// Set whether to run on startup.
    pub fn run_on_startup(mut self, run_on_startup: bool) -> Self {
        self.config.run_on_startup = run_on_startup;
        self
    }

    /// Build the aggregator.
    pub fn build(self) -> Aggregator {
        Aggregator::with_interval(self.db, self.config.interval)
    }

    /// Build and start the aggregator.
    pub fn start(self) -> JoinHandle<()> {
        self.build().start()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregator_config_default() {
        let config = AggregatorConfig::default();
        assert_eq!(config.interval.as_secs(), 600);
        assert!(config.run_on_startup);
        assert_eq!(config.catchup_hours, 2);
    }

    #[test]
    fn test_aggregator_config_builder() {
        let config = AggregatorConfig::new()
            .with_interval(Duration::from_secs(300))
            .with_run_on_startup(false)
            .with_catchup_hours(4);

        assert_eq!(config.interval.as_secs(), 300);
        assert!(!config.run_on_startup);
        assert_eq!(config.catchup_hours, 4);
    }

    #[tokio::test]
    async fn test_aggregator_creation() {
        let db = Arc::new(CostDatabase::open_in_memory().unwrap());
        let aggregator = Aggregator::new(db);

        assert_eq!(aggregator.interval().as_secs(), 600);
    }

    #[tokio::test]
    async fn test_aggregator_run_once() {
        let db = Arc::new(CostDatabase::open_in_memory().unwrap());
        let aggregator = Aggregator::new(db);

        // Should not fail on empty database
        aggregator.run_once().unwrap();
    }

    #[tokio::test]
    async fn test_aggregator_builder() {
        let db = Arc::new(CostDatabase::open_in_memory().unwrap());
        let aggregator = AggregatorBuilder::new(db)
            .interval(Duration::from_secs(120))
            .run_on_startup(false)
            .build();

        assert_eq!(aggregator.interval().as_secs(), 120);
    }
}
