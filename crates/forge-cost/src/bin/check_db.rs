//! Check costs database schema and data

use forge_cost::CostDatabase;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = Path::new("/home/coder/.forge/costs.db");

    if !db_path.exists() {
        println!("Database does not exist at {:?}", db_path);
        return Ok(());
    }

    let db = CostDatabase::open(db_path)?;

    // Get connection to run raw queries
    let conn = db.connection();
    let conn = conn.lock().unwrap();

    // Check table schema using PRAGMA
    println!("=== api_calls table columns ===");
    let mut stmt = conn.prepare("PRAGMA table_info(api_calls)")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        let type_: String = row.get(2)?;
        Ok((name, type_))
    })?;

    for row in rows.flatten() {
        let (name, type_) = row;
        if name.contains("worker") || name.contains("bead") || name.contains("task") {
            println!("  {} : {}", name, type_);
        }
    }

    // Count records with worker_id
    println!("\n=== Cost data ===");
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM api_calls WHERE worker_id IS NOT NULL",
        [],
        |row| row.get(0)
    ).unwrap_or(0);
    println!("Records with worker_id: {}", count);

    // Show costs by worker
    println!("\n=== Costs by worker (top 5) ===");
    let mut stmt = conn.prepare(
        "SELECT worker_id, SUM(cost_usd), COUNT(*)
         FROM api_calls
         WHERE worker_id IS NOT NULL
         GROUP BY worker_id
         ORDER BY SUM(cost_usd) DESC
         LIMIT 5"
    )?;

    let rows = stmt.query_map([], |row| {
        let worker_id: String = row.get(0)?;
        let cost: f64 = row.get(1)?;
        let count: i64 = row.get(2)?;
        Ok((worker_id, cost, count))
    })?;

    for row in rows.flatten() {
        let (worker_id, cost, count) = row;
        println!("  {} : ${:.4} ({} calls)", worker_id, cost, count);
    }

    Ok(())
}
