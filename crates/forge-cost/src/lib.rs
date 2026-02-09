//! # forge-cost
//!
//! Cost tracking and analysis for FORGE worker agents.
//!
//! This crate provides:
//! - [`LogParser`] - Parse API usage events from worker log files
//! - [`CostDatabase`] - SQLite storage with efficient aggregation
//! - [`CostQuery`] - Query functions for cost analysis
//!
//! ## Supported API Formats
//!
//! - Anthropic (Claude): input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens
//! - OpenAI: prompt_tokens, completion_tokens
//! - DeepSeek: input_tokens, output_tokens
//! - GLM (via z.ai proxy): Uses Anthropic-compatible format with modelUsage
//!
//! ## Example
//!
//! ```no_run
//! use forge_cost::{CostDatabase, LogParser, CostQuery};
//!
//! fn main() -> anyhow::Result<()> {
//!     // Open or create cost database
//!     let db = CostDatabase::open("~/.forge/costs.db")?;
//!
//!     // Parse logs from worker files
//!     let parser = LogParser::new();
//!     let log_dir = std::path::Path::new("~/.forge/logs");
//!     let api_calls = parser.parse_directory(log_dir)?;
//!
//!     // Insert parsed calls
//!     db.insert_api_calls(&api_calls)?;
//!
//!     // Query costs
//!     let query = CostQuery::new(&db);
//!     let today = query.get_today_costs()?;
//!     println!("Today's costs: {:?}", today);
//!
//!     Ok(())
//! }
//! ```

pub mod db;
pub mod error;
pub mod models;
pub mod parser;
pub mod query;

// Re-export main types
pub use db::CostDatabase;
pub use error::{CostError, Result};
pub use models::{ApiCall, CostBreakdown, DailyCost, ModelCost, ProjectedCost};
pub use parser::LogParser;
pub use query::CostQuery;
