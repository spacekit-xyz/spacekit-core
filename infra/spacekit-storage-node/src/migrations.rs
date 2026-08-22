//! Database Migration System
//!
//! Enterprise-grade migration support with versioned scripts, rollback, and validation

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::database::Database;

/// Current schema version - increment this when adding new migrations
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Migration record for tracking applied migrations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub version: u32,
    pub name: String,
    pub applied_at: DateTime<Utc>,
    pub checksum: String,
    pub rollback_available: bool,
}

/// Migration script definition
pub struct Migration {
    pub version: u32,
    pub name: String,
    pub description: String,
    pub up: Box<dyn Fn(&Database) -> Result<()> + Send + Sync>,
    pub down: Option<Box<dyn Fn(&Database) -> Result<()> + Send + Sync>>,
}

/// Migration manager for handling schema upgrades and rollbacks
pub struct MigrationManager {
    migrations: Vec<Migration>,
    migration_history_path: PathBuf,
}

impl MigrationManager {
    /// Create a new migration manager
    pub fn new(migration_history_path: PathBuf) -> Self {
        Self {
            migrations: Vec::new(),
            migration_history_path,
        }
    }

    /// Register a migration script
    pub fn register(&mut self, migration: Migration) {
        self.migrations.push(migration);
        // Sort by version to ensure correct order
        self.migrations.sort_by_key(|m| m.version);
    }

    /// Get all registered migrations
    pub fn migrations(&self) -> &[Migration] {
        &self.migrations
    }

    /// Load migration history from disk
    pub async fn load_history(&self) -> Result<HashMap<u32, MigrationRecord>> {
        if !self.migration_history_path.exists() {
            return Ok(HashMap::new());
        }

        let content = tokio::fs::read_to_string(&self.migration_history_path)
            .await
            .context("Failed to read migration history")?;

        let history: Vec<MigrationRecord> =
            serde_json::from_str(&content).context("Failed to parse migration history")?;

        Ok(history.into_iter().map(|r| (r.version, r)).collect())
    }

    /// Save migration history to disk
    async fn save_history(&self, history: &HashMap<u32, MigrationRecord>) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = self.migration_history_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("Failed to create migration history directory")?;
        }

        let records: Vec<&MigrationRecord> = history.values().collect();
        let content = serde_json::to_string_pretty(&records)
            .context("Failed to serialize migration history")?;

        tokio::fs::write(&self.migration_history_path, content)
            .await
            .context("Failed to write migration history")?;

        Ok(())
    }

    /// Apply pending migrations to the database
    pub async fn migrate(&self, database: &Database) -> Result<()> {
        let mut history = self.load_history().await?;
        let current_version = database.get_schema_version()?;

        info!("Current database schema version: {}", current_version);
        info!("Target schema version: {}", CURRENT_SCHEMA_VERSION);

        if current_version >= CURRENT_SCHEMA_VERSION {
            debug!("Database is up to date, no migrations needed");
            return Ok(());
        }

        // Find migrations that need to be applied
        let pending: Vec<&Migration> = self
            .migrations
            .iter()
            .filter(|m| m.version > current_version && m.version <= CURRENT_SCHEMA_VERSION)
            .collect();

        if pending.is_empty() {
            info!("No pending migrations found");
            return Ok(());
        }

        info!("Applying {} pending migration(s)...", pending.len());

        for migration in pending {
            info!(
                "Applying migration {}: {}",
                migration.version, migration.name
            );

            // Calculate checksum for migration
            let checksum = self.calculate_migration_checksum(migration);

            // Apply migration (Database is Arc-based, so we need to handle it differently)
            // Since Database methods take &self, we can call them directly
            (migration.up)(database).with_context(|| {
                format!(
                    "Failed to apply migration {}: {}",
                    migration.version, migration.name
                )
            })?;

            // Update schema version
            database.set_schema_version(migration.version)?;

            // Record migration
            let record = MigrationRecord {
                version: migration.version,
                name: migration.name.clone(),
                applied_at: Utc::now(),
                checksum: checksum.clone(),
                rollback_available: migration.down.is_some(),
            };

            history.insert(migration.version, record);

            info!("✅ Migration {} applied successfully", migration.version);
        }

        // Save migration history
        self.save_history(&history).await?;

        info!(
            "✅ All migrations applied successfully. Database version: {}",
            CURRENT_SCHEMA_VERSION
        );
        Ok(())
    }

    /// Rollback the last migration
    pub async fn rollback(&self, database: &Database) -> Result<()> {
        let mut history = self.load_history().await?;
        let current_version = database.get_schema_version()?;

        if current_version == 0 {
            return Err(anyhow::anyhow!("Cannot rollback: database is at version 0"));
        }

        // Find the last applied migration
        let last_migration = self
            .migrations
            .iter()
            .rev()
            .find(|m| m.version == current_version);

        let migration = match last_migration {
            Some(m) => m,
            None => {
                return Err(anyhow::anyhow!(
                    "No migration found for version {}",
                    current_version
                ))
            }
        };

        if migration.down.is_none() {
            return Err(anyhow::anyhow!(
                "Migration {} does not support rollback",
                migration.version
            ));
        }

        info!(
            "Rolling back migration {}: {}",
            migration.version, migration.name
        );

        // Execute rollback
        (migration.down.as_ref().unwrap())(database).with_context(|| {
            format!(
                "Failed to rollback migration {}: {}",
                migration.version, migration.name
            )
        })?;

        // Determine previous version
        let previous_version = if current_version > 1 {
            self.migrations
                .iter()
                .rev()
                .find(|m| m.version < current_version)
                .map(|m| m.version)
                .unwrap_or(0)
        } else {
            0
        };

        // Update schema version
        database.set_schema_version(previous_version)?;

        // Remove from history
        history.remove(&migration.version);
        self.save_history(&history).await?;

        info!(
            "✅ Migration {} rolled back successfully. Database version: {}",
            migration.version, previous_version
        );
        Ok(())
    }

    /// Rollback to a specific version
    pub async fn rollback_to(&self, database: &Database, target_version: u32) -> Result<()> {
        let current_version = database.get_schema_version()?;

        if target_version >= current_version {
            return Err(anyhow::anyhow!(
                "Target version {} must be less than current version {}",
                target_version,
                current_version
            ));
        }

        info!(
            "Rolling back from version {} to version {}",
            current_version, target_version
        );

        // Rollback migrations in reverse order
        while database.get_schema_version()? > target_version {
            self.rollback(database).await?;
        }

        info!("✅ Rollback to version {} completed", target_version);
        Ok(())
    }

    /// Validate all migrations are properly defined
    pub fn validate(&self) -> Result<()> {
        // Check for duplicate versions
        let mut versions: Vec<u32> = self.migrations.iter().map(|m| m.version).collect();
        versions.sort();

        for i in 1..versions.len() {
            if versions[i] == versions[i - 1] {
                return Err(anyhow::anyhow!(
                    "Duplicate migration version: {}",
                    versions[i]
                ));
            }
        }

        // Check for gaps (warn, not error)
        for i in 1..versions.len() {
            if versions[i] != versions[i - 1] + 1 {
                warn!(
                    "Gap in migration versions: {} -> {}",
                    versions[i - 1],
                    versions[i]
                );
            }
        }

        info!(
            "✅ Migration validation passed: {} migrations registered",
            self.migrations.len()
        );
        Ok(())
    }

    /// Get migration status
    pub async fn status(&self, database: &Database) -> Result<MigrationStatus> {
        let history = self.load_history().await?;
        let current_version = database.get_schema_version()?;

        let applied: Vec<&MigrationRecord> = history.values().collect();
        let pending: Vec<&Migration> = self
            .migrations
            .iter()
            .filter(|m| m.version > current_version)
            .collect();

        Ok(MigrationStatus {
            current_version,
            target_version: CURRENT_SCHEMA_VERSION,
            applied_count: applied.len(),
            pending_count: pending.len(),
            applied_migrations: applied.into_iter().map(|r| r.clone()).collect(),
            pending_migrations: pending.iter().map(|m| m.name.clone()).collect(),
        })
    }

    /// Calculate checksum for a migration (for validation)
    fn calculate_migration_checksum(&self, migration: &Migration) -> String {
        use blake3;
        let data = format!(
            "{}:{}:{}",
            migration.version, migration.name, migration.description
        );
        hex::encode(blake3::hash(data.as_bytes()).as_bytes())
    }
}

/// Migration status information
#[derive(Debug, Clone, Serialize)]
pub struct MigrationStatus {
    pub current_version: u32,
    pub target_version: u32,
    pub applied_count: usize,
    pub pending_count: usize,
    pub applied_migrations: Vec<MigrationRecord>,
    pub pending_migrations: Vec<String>,
}

/// Default migrations - register all schema migrations here
pub fn create_default_migrations() -> MigrationManager {
    let mut manager = MigrationManager::new(PathBuf::from("./migrations/history.json"));

    // Migration 1: Initial schema (already applied if database exists)
    manager.register(Migration {
        version: 1,
        name: "initial_schema".to_string(),
        description: "Initial database schema with users, files, and facts".to_string(),
        up: Box::new(|_db| {
            // Initial schema is created by Database::initialize()
            // This migration is a no-op for existing databases
            Ok(())
        }),
        down: Some(Box::new(|_db| {
            // Rollback would require clearing all data
            // In production, you'd want to backup first
            warn!("Rollback of initial schema would clear all data - skipping");
            Ok(())
        })),
    });

    // Add more migrations here as schema evolves
    // Example:
    // manager.register(Migration {
    //     version: 2,
    //     name: "add_indexes".to_string(),
    //     description: "Add indexes for performance".to_string(),
    //     up: Box::new(|db| {
    //         // Add indexes
    //         Ok(())
    //     }),
    //     down: Some(Box::new(|db| {
    //         // Remove indexes
    //         Ok(())
    //     })),
    // });

    manager
}
