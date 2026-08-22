//! Enhanced Persistence Demo
//!
//! This example demonstrates the advanced persistence features of the custom storage system:
//! - Write-ahead logging (WAL)
//! - Backup rotation
//! - Crash recovery
//! - Data integrity verification
//! - Manual operations

use chrono::Utc;
use spacekit_storage_node::database::{ContactMessage, Database, PersistenceConfig, User};
use std::path::PathBuf;
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for better observability
    tracing_subscriber::fmt::init();

    println!("🚀 Enhanced Persistence Demo");
    println!("==============================\n");

    // Create temporary directory for demo
    let temp_dir = tempdir()?;
    let db_path = temp_dir.path().join("demo_storage.json");

    // Demo 1: Basic Enhanced Storage
    demo_basic_enhanced_storage(&db_path)?;

    // Demo 2: Custom Configuration
    demo_custom_configuration(&db_path)?;

    // Demo 3: Backup and Recovery
    demo_backup_and_recovery(&db_path)?;

    // Demo 4: Data Integrity
    demo_data_integrity(&db_path)?;

    // Demo 5: Manual Operations
    demo_manual_operations(&db_path)?;

    println!("\n✅ All demos completed successfully!");
    println!("📁 Demo files are in: {:?}", temp_dir.path());

    Ok(())
}

fn demo_basic_enhanced_storage(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("📂 Demo 1: Basic Enhanced Storage");
    println!("----------------------------------");

    // Create database with default enhanced configuration
    let db = Database::new(db_path.to_str().unwrap())?;
    db.initialize()?;

    println!("✓ Database created with enhanced persistence features");
    println!("  - WAL enabled: ✓");
    println!("  - Backup rotation: ✓ (5 backups)");
    println!("  - Checksum verification: ✓");

    // Add some data
    let user = User {
        username: "alice".to_string(),
        first_name: Some("Alice".to_string()),
        last_name: Some("Smith".to_string()),
        email: "alice@example.com".to_string(),
        address: "did:spacekit:alice".to_string(),
        network: "mainnet".to_string(),
        message: "Hello from enhanced storage!".to_string(),
        created_at: Some(Utc::now()),
    };

    db.insert_user(&user)?;
    println!("✓ User inserted with WAL logging");

    // Check stats
    let stats = db.get_storage_stats()?;
    println!("✓ Database stats:");
    println!("  - Version: {}", stats.database_version);
    println!("  - Users: {}", stats.user_count);
    println!("  - WAL enabled: {}", stats.wal_enabled);
    println!("  - Data size: {} bytes", stats.data_file_size);

    println!();
    Ok(())
}

fn demo_custom_configuration(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("⚙️  Demo 2: Custom Configuration");
    println!("-------------------------------");

    let custom_path = db_path.with_file_name("custom_config.json");

    // Create custom configuration
    let config = PersistenceConfig {
        enable_wal: true,
        backup_count: 3,
        sync_interval_ms: 1000,
        compress_backups: false,
        verify_checksums: true,
        ..Default::default()
    };

    let db = Database::with_config(custom_path.to_str().unwrap(), config)?;
    db.initialize()?;

    println!("✓ Database created with custom configuration:");
    println!("  - WAL enabled: ✓");
    println!("  - Backup count: 3");
    println!("  - Sync interval: 1000ms");
    println!("  - Checksum verification: ✓");

    // Add multiple items to trigger backup rotation
    for i in 0..5 {
        let user = User {
            username: format!("user_{}", i),
            first_name: Some(format!("First_{}", i)),
            last_name: Some(format!("Last_{}", i)),
            email: format!("user_{}@example.com", i),
            address: format!("did:spacekit:user_{}", i),
            network: "testnet".to_string(),
            message: format!("Message from user {}", i),
            created_at: None,
        };

        db.insert_user(&user)?;
        println!("✓ User {} inserted and backed up", i);

        // Small delay to ensure distinct timestamps
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    println!("✓ Backup rotation demonstrated (keeping only 3 backups)");
    println!();
    Ok(())
}

fn demo_backup_and_recovery(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("💾 Demo 3: Backup and Recovery");
    println!("------------------------------");

    let recovery_path = db_path.with_file_name("recovery_demo.json");

    // Create database and add data
    let db = Database::new(recovery_path.to_str().unwrap())?;
    db.initialize()?;

    // Add critical data
    let important_user = User {
        username: "critical_user".to_string(),
        first_name: Some("Critical".to_string()),
        last_name: Some("User".to_string()),
        email: "critical@example.com".to_string(),
        address: "did:spacekit:critical".to_string(),
        network: "mainnet".to_string(),
        message: "This is critical data that must not be lost!".to_string(),
        created_at: None,
    };

    db.insert_user(&important_user)?;
    println!("✓ Critical data inserted and automatically backed up");

    // Create manual backup
    let backup_path = db.create_manual_backup()?;
    println!(
        "✓ Manual backup created: {:?}",
        backup_path.file_name().unwrap()
    );

    // Simulate recovery scenario by creating a new database instance
    // In a real crash scenario, this would automatically recover from backups
    let recovered_db = Database::new(recovery_path.to_str().unwrap())?;
    let users = recovered_db.select_all_users()?;

    println!("✓ Recovery simulation successful:");
    println!("  - Recovered {} users", users.len());
    println!(
        "  - Critical user found: {}",
        users.iter().any(|u| u.username == "critical_user")
    );

    println!();
    Ok(())
}

fn demo_data_integrity(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔒 Demo 4: Data Integrity");
    println!("-------------------------");

    let integrity_path = db_path.with_file_name("integrity_demo.json");

    let db = Database::new(integrity_path.to_str().unwrap())?;
    db.initialize()?;

    // Add data with checksums
    let message = ContactMessage {
        name: "Integrity Tester".to_string(),
        email: "integrity@example.com".to_string(),
        message: "Testing data integrity features".to_string(),
        created_at: None,
    };

    db.insert_message(&message)?;
    println!("✓ Message inserted with automatic checksum calculation");

    // Verify integrity
    let is_valid = db.verify_integrity()?;
    println!(
        "✓ Data integrity verification: {}",
        if is_valid { "PASSED" } else { "FAILED" }
    );

    // Get stats to show metadata
    let stats = db.get_storage_stats()?;
    println!("✓ Database metadata:");
    println!("  - Version: {}", stats.database_version);
    println!(
        "  - Last saved: {}",
        stats.last_saved.format("%Y-%m-%d %H:%M:%S")
    );
    println!("  - Messages: {}", stats.message_count);

    println!();
    Ok(())
}

fn demo_manual_operations(db_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔧 Demo 5: Manual Operations");
    println!("----------------------------");

    let manual_path = db_path.with_file_name("manual_ops_demo.json");

    let db = Database::new(manual_path.to_str().unwrap())?;
    db.initialize()?;

    // Add some data
    for i in 0..3 {
        let user = User {
            username: format!("manual_user_{}", i),
            first_name: Some(format!("Manual_{}", i)),
            last_name: Some(format!("User_{}", i)),
            email: format!("manual_{}@example.com", i),
            address: format!("did:spacekit:manual_{}", i),
            network: "testnet".to_string(),
            message: "Data for manual operations demo".to_string(),
            created_at: None,
        };

        db.insert_user(&user)?;
    }

    println!("✓ Sample data inserted");

    // Create manual backup
    let backup_path = db.create_manual_backup()?;
    println!(
        "✓ Manual backup created: {:?}",
        backup_path.file_name().unwrap()
    );

    // Force checkpoint (flush all pending operations)
    db.checkpoint()?;
    println!("✓ Database checkpoint completed");
    println!("  - All pending operations flushed");
    println!("  - WAL file cleaned up");

    // Verify integrity manually
    let integrity_ok = db.verify_integrity()?;
    println!(
        "✓ Manual integrity check: {}",
        if integrity_ok { "PASSED" } else { "FAILED" }
    );

    // Show final stats
    let final_stats = db.get_storage_stats()?;
    println!("✓ Final database state:");
    println!("  - Version: {}", final_stats.database_version);
    println!("  - Total users: {}", final_stats.user_count);
    println!("  - WAL enabled: {}", final_stats.wal_enabled);
    println!("  - Backup count configured: {}", final_stats.backup_count);

    println!();
    Ok(())
}
