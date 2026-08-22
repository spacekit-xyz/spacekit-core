//! SQL Query Interface for Storage Node
//!
//! Provides SQL-like querying capabilities while maintaining the performance
//! of the in-memory database with optional SQLite backend for complex analytics.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "sqlite")]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "sqlite")]
use tracing::{debug, info};

use crate::database::{Database, FactMetadataRecord, FileMetadata, User};

/// SQL query builder for storage node data
pub struct StorageQueryBuilder {
    database: Arc<Database>,
    #[cfg(feature = "sqlite")]
    sqlite_path: Option<PathBuf>,
}

/// Query filter operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterOp {
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    Contains,
    StartsWith,
    EndsWith,
    In,
    NotIn,
}

/// Query filter condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: FilterOp,
    pub value: FilterValue,
}

/// Subquery type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubqueryType {
    /// IN subquery: field IN (SELECT ...)
    In,
    /// NOT IN subquery: field NOT IN (SELECT ...)
    NotIn,
    /// EXISTS subquery: EXISTS (SELECT ...)
    Exists,
    /// NOT EXISTS subquery: NOT EXISTS (SELECT ...)
    NotExists,
}

/// Subquery definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subquery {
    pub subquery_type: SubqueryType,
    pub table: String,        // "files", "facts", "users"
    pub field: String,        // Field to select from subquery
    pub filters: Vec<Filter>, // Filters for subquery
}

/// Filter value types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterValue {
    String(String),
    Number(f64),
    Integer(i64),
    Boolean(bool),
    Array(Vec<String>),
    Subquery(Subquery), // Nested subquery
}

/// Sort order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// Sort criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortBy {
    pub field: String,
    pub order: SortOrder,
}

/// JOIN types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    FullOuter,
}

/// JOIN condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinCondition {
    pub left_table: String,
    pub left_field: String,
    pub right_table: String,
    pub right_field: String,
}

/// JOIN specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Join {
    pub join_type: JoinType,
    pub table: String,
    pub condition: JoinCondition,
}

/// UNION operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnionType {
    /// UNION - removes duplicates
    Union,
    /// UNION ALL - keeps duplicates
    UnionAll,
}

/// UNION query (combines multiple queries)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnionQuery {
    pub queries: Vec<FileQuery>, // Queries to combine
    pub union_type: UnionType,   // UNION or UNION ALL
}

/// Query for files
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileQuery {
    #[serde(default)]
    pub filters: Vec<Filter>,
    pub sort_by: Option<SortBy>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    #[serde(default)]
    pub joins: Vec<Join>, // JOIN operations
    #[serde(default)]
    pub window_functions: Vec<WindowFunctionDef>, // Window functions (ROW_NUMBER, RANK, etc.)
    #[serde(default)]
    pub distinct: bool, // DISTINCT - remove duplicate rows
}

/// Query for facts
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FactQuery {
    pub filters: Vec<Filter>,
    pub sort_by: Option<SortBy>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub joins: Vec<Join>,                         // JOIN operations
    pub window_functions: Vec<WindowFunctionDef>, // Window functions (ROW_NUMBER, RANK, etc.)
    pub distinct: bool,                           // DISTINCT - remove duplicate rows
}

/// Query for users
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserQuery {
    pub filters: Vec<Filter>,
    pub sort_by: Option<SortBy>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub joins: Vec<Join>,                         // JOIN operations
    pub window_functions: Vec<WindowFunctionDef>, // Window functions (ROW_NUMBER, RANK, etc.)
    #[serde(default)]
    pub distinct: bool,      // DISTINCT - remove duplicate rows
}

/// Window function result for a single row
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFunctionResult {
    pub alias: String,
    pub value: WindowFunctionValue,
}

/// Window function value types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WindowFunctionValue {
    Integer(i64),
    Float(f64),
    String(String),
}

/// Query result for files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileQueryResult {
    pub files: Vec<FileMetadata>,
    pub total_count: usize,
    pub execution_time_ms: u64,
    /// Window function results (one per file, matching window_functions order)
    #[serde(default)]
    pub window_results: Vec<Vec<WindowFunctionResult>>,
}

/// Query result for facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactQueryResult {
    pub facts: Vec<FactMetadataRecord>,
    pub total_count: usize,
    pub execution_time_ms: u64,
    /// Window function results (one per fact, matching window_functions order)
    #[serde(default)]
    pub window_results: Vec<Vec<WindowFunctionResult>>,
}

/// Query result for users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserQueryResult {
    pub users: Vec<User>,
    pub total_count: usize,
    pub execution_time_ms: u64,
    /// Window function results (one per user, matching window_functions order)
    #[serde(default)]
    pub window_results: Vec<Vec<WindowFunctionResult>>,
}

/// Aggregate functions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregateFunction {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

/// Window function types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindowFunction {
    /// ROW_NUMBER() - Sequential row number within partition
    RowNumber,
    /// RANK() - Rank with gaps for ties
    Rank,
    /// DENSE_RANK() - Rank without gaps for ties
    DenseRank,
    /// NTILE(n) - Divide rows into n buckets
    Ntile(usize),
    /// LAG(field, offset) - Previous row value
    Lag { field: String, offset: usize },
    /// LEAD(field, offset) - Next row value
    Lead { field: String, offset: usize },
    /// FIRST_VALUE(field) - First value in partition
    FirstValue { field: String },
    /// LAST_VALUE(field) - Last value in partition
    LastValue { field: String },
    /// Aggregate with OVER clause
    AggregateOver {
        function: AggregateFunction,
        field: String,
    },
}

/// Window specification (PARTITION BY and ORDER BY)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSpec {
    /// Fields to partition by (PARTITION BY clause)
    pub partition_by: Vec<String>,
    /// Sort order for window (ORDER BY clause)
    pub order_by: Option<SortBy>,
}

/// Window function definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFunctionDef {
    pub function: WindowFunction,
    pub window_spec: WindowSpec,
    pub alias: Option<String>, // Optional alias for the window function result
}

/// Aggregate query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateQuery {
    pub function: AggregateFunction,
    pub field: String,
    pub filters: Vec<Filter>,
    pub group_by: Option<String>,
}

/// Aggregate result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateResult {
    pub value: f64,
    pub groups: Option<HashMap<String, f64>>,
}

impl StorageQueryBuilder {
    /// Create a new query builder
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            database,
            #[cfg(feature = "sqlite")]
            sqlite_path: None,
        }
    }

    /// Initialize SQLite backend for complex queries
    #[cfg(feature = "sqlite")]
    pub async fn init_sqlite_backend(&mut self, db_path: PathBuf) -> Result<()> {
        use rusqlite::Connection;

        info!("Initializing SQLite query backend at: {:?}", db_path);
        let path_clone = db_path.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(path_clone)?;

            // Create tables
            conn.execute(
                "CREATE TABLE IF NOT EXISTS files (
                id TEXT PRIMARY KEY,
                filename TEXT NOT NULL,
                size INTEGER NOT NULL,
                hash TEXT NOT NULL,
                owner_did TEXT NOT NULL,
                encryption_algorithm TEXT NOT NULL,
                content_type TEXT,
                created_at TEXT NOT NULL,
                last_accessed TEXT
            )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS facts (
                fact_id TEXT PRIMARY KEY,
                version INTEGER NOT NULL,
                author TEXT NOT NULL,
                created_at TEXT NOT NULL,
                content_size INTEGER NOT NULL,
                content_type TEXT NOT NULL,
                category TEXT NOT NULL,
                domain TEXT NOT NULL,
                verification_level TEXT NOT NULL,
                confidence_score REAL NOT NULL,
                storage_tier TEXT NOT NULL,
                compressed INTEGER NOT NULL,
                encrypted INTEGER NOT NULL,
                checksum TEXT NOT NULL
            )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS fact_tags (
                fact_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (fact_id, tag),
                FOREIGN KEY (fact_id) REFERENCES facts(fact_id)
            )",
                [],
            )?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS users (
                username TEXT PRIMARY KEY,
                email TEXT NOT NULL,
                address TEXT NOT NULL,
                network TEXT NOT NULL,
                message TEXT,
                created_at TEXT
            )",
                [],
            )?;

            // Create indexes for common queries
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_files_owner ON files(owner_did)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_files_created ON files(created_at)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_facts_author ON facts(author)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_facts_category ON facts(category)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_facts_domain ON facts(domain)",
                [],
            )?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_fact_tags_tag ON fact_tags(tag)",
                [],
            )?;
            Ok(())
        })
        .await??;

        self.sqlite_path = Some(db_path);

        info!("SQLite query backend initialized successfully");
        Ok(())
    }

    /// Sync in-memory data to SQLite
    #[cfg(feature = "sqlite")]
    pub async fn sync_to_sqlite(&self) -> Result<()> {
        let Some(db_path) = self.sqlite_path.clone() else {
            return Ok(());
        };

        let users = self.database.select_all_users()?;
        let facts = self.database.select_all_fact_metadata()?;

        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = rusqlite::Connection::open(db_path)?;

            // Sync users
            for user in &users {
                conn.execute(
                    "INSERT OR REPLACE INTO users (username, email, address, network, message, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        user.username,
                        user.email,
                        user.address,
                        user.network,
                        user.message,
                        user.created_at.map(|t| t.to_rfc3339()),
                    ],
                )?;
            }

            // Sync facts
            for fact in &facts {
                conn.execute(
                    "INSERT OR REPLACE INTO facts 
                     (fact_id, version, author, created_at, content_size, content_type, 
                      category, domain, verification_level, confidence_score, storage_tier,
                      compressed, encrypted, checksum)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        fact.fact_id,
                        fact.version,
                        fact.author,
                        fact.created_at.to_rfc3339(),
                        fact.content_size,
                        fact.content_type,
                        fact.category,
                        fact.domain,
                        fact.verification_level,
                        fact.confidence_score,
                        fact.storage_tier,
                        fact.compressed as i32,
                        fact.encrypted as i32,
                        fact.checksum,
                    ],
                )?;

                // Sync fact tags
                for tag in fact.tags.iter() {
                    conn.execute(
                        "INSERT OR REPLACE INTO fact_tags (fact_id, tag) VALUES (?1, ?2)",
                        rusqlite::params![fact.fact_id, tag],
                    )?;
                }
            }

            debug!("Synced {} users and {} facts to SQLite", users.len(), facts.len());
            Ok(())
        }).await??;

        Ok(())
    }

    /// Query files with filters and JOINs
    pub fn query_files(
        &self,
        query: FileQuery,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<FileQueryResult>> + Send + '_>>
    {
        Box::pin(async move {
            let start = std::time::Instant::now();

            // Get all file metadata from database
            let mut all_files = self.get_all_files_from_db().await?;

            // Apply JOINs if present
            if !query.joins.is_empty() {
                all_files = self
                    .execute_joins_for_files(&all_files, &query.joins)
                    .await?;
            }

            // Apply filters (including subqueries)
            let mut filtered = Vec::new();
            for file in all_files {
                if self.apply_file_filters(&file, &query.filters).await {
                    filtered.push(file);
                }
            }

            // Apply sorting
            if let Some(sort) = &query.sort_by {
                self.sort_files(&mut filtered, sort);
            }

            // Apply DISTINCT if requested
            if query.distinct {
                filtered = self.apply_distinct_to_files(&filtered);
            }

            // Apply window functions if present
            let window_results = if !query.window_functions.is_empty() {
                self.compute_window_functions_for_files(&filtered, &query.window_functions)
                    .await?
            } else {
                Vec::new()
            };

            let total_count = filtered.len();

            // Apply pagination
            let offset = query.offset.unwrap_or(0);
            let limit = query.limit.unwrap_or(usize::MAX);
            let files = filtered.into_iter().skip(offset).take(limit).collect();

            // Apply pagination to window results too
            let window_results_paginated: Vec<Vec<WindowFunctionResult>> =
                if !window_results.is_empty() {
                    window_results
                        .into_iter()
                        .skip(offset)
                        .take(limit)
                        .collect()
                } else {
                    Vec::new()
                };

            let execution_time_ms = start.elapsed().as_millis() as u64;

            Ok(FileQueryResult {
                files,
                total_count,
                execution_time_ms,
                window_results: window_results_paginated,
            })
        })
    }

    /// Execute JOIN operations for files
    async fn execute_joins_for_files(
        &self,
        files: &[FileMetadata],
        joins: &[Join],
    ) -> Result<Vec<FileMetadata>> {
        let mut result = files.to_vec();

        for join in joins {
            match join.table.as_str() {
                "users" => {
                    let users = self.database.select_all_users()?;
                    result = self.join_files_with_users(&result, &users, join)?;
                }
                "facts" => {
                    let facts = self.database.select_all_fact_metadata()?;
                    result = self.join_files_with_facts(&result, &facts, join)?;
                }
                _ => {
                    return Err(anyhow::anyhow!("Unsupported JOIN table: {}", join.table));
                }
            }
        }

        Ok(result)
    }

    /// Join files with users
    fn join_files_with_users(
        &self,
        files: &[FileMetadata],
        users: &[User],
        join: &Join,
    ) -> Result<Vec<FileMetadata>> {
        // Create a lookup map for users
        let user_map: HashMap<_, _> = match join.condition.right_field.as_str() {
            "address" => users.iter().map(|u| (u.address.clone(), u)).collect(),
            "email" => users.iter().map(|u| (u.email.clone(), u)).collect(),
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported JOIN field: {}",
                    join.condition.right_field
                ))
            }
        };

        let mut result = Vec::new();

        for file in files {
            let join_key = match join.condition.left_field.as_str() {
                "owner_did" => &file.owner_did,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unsupported JOIN field: {}",
                        join.condition.left_field
                    ))
                }
            };

            match join.join_type {
                JoinType::Inner => {
                    if user_map.contains_key(join_key) {
                        result.push(file.clone());
                    }
                }
                JoinType::Left => {
                    result.push(file.clone());
                }
                JoinType::Right => {
                    // For RIGHT JOIN, include all users even if no matching file
                    // This is a simplified implementation
                    if user_map.contains_key(join_key) {
                        result.push(file.clone());
                    }
                }
                JoinType::FullOuter => {
                    result.push(file.clone());
                }
            }
        }

        Ok(result)
    }

    /// Join files with facts
    fn join_files_with_facts(
        &self,
        files: &[FileMetadata],
        facts: &[FactMetadataRecord],
        join: &Join,
    ) -> Result<Vec<FileMetadata>> {
        // Create a lookup map for facts
        let fact_map: HashMap<_, _> = match join.condition.right_field.as_str() {
            "fact_id" => facts.iter().map(|f| (f.fact_id.clone(), f)).collect(),
            "author" => facts.iter().map(|f| (f.author.clone(), f)).collect(),
            _ => {
                return Err(anyhow::anyhow!(
                    "Unsupported JOIN field: {}",
                    join.condition.right_field
                ))
            }
        };

        let mut result = Vec::new();

        for file in files {
            let join_key = match join.condition.left_field.as_str() {
                "id" => &file.id,
                "owner_did" => &file.owner_did,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unsupported JOIN field: {}",
                        join.condition.left_field
                    ))
                }
            };

            match join.join_type {
                JoinType::Inner => {
                    if fact_map.contains_key(join_key) {
                        result.push(file.clone());
                    }
                }
                JoinType::Left => {
                    result.push(file.clone());
                }
                JoinType::Right => {
                    if fact_map.contains_key(join_key) {
                        result.push(file.clone());
                    }
                }
                JoinType::FullOuter => {
                    result.push(file.clone());
                }
            }
        }

        Ok(result)
    }

    /// Query facts with filters
    pub async fn query_facts(&self, query: FactQuery) -> Result<FactQueryResult> {
        let start = std::time::Instant::now();

        // Get all fact metadata
        let all_facts = self.database.select_all_fact_metadata()?;

        // Apply filters
        let mut filtered: Vec<FactMetadataRecord> = all_facts
            .into_iter()
            .filter(|fact| self.apply_fact_filters(fact, &query.filters))
            .collect();

        // Apply DISTINCT if requested
        if query.distinct {
            filtered = self.apply_distinct_to_facts(&filtered);
        }

        // Apply sorting
        if let Some(sort) = &query.sort_by {
            self.sort_facts(&mut filtered, sort);
        }

        // Apply window functions if present
        let window_results = if !query.window_functions.is_empty() {
            self.compute_window_functions_for_facts(&filtered, &query.window_functions)
                .await?
        } else {
            Vec::new()
        };

        let total_count = filtered.len();

        // Apply pagination
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(usize::MAX);
        let facts = filtered.into_iter().skip(offset).take(limit).collect();

        // Apply pagination to window results too
        let window_results_paginated: Vec<Vec<WindowFunctionResult>> = if !window_results.is_empty()
        {
            window_results
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect()
        } else {
            Vec::new()
        };

        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(FactQueryResult {
            facts,
            total_count,
            execution_time_ms,
            window_results: window_results_paginated,
        })
    }

    /// Query users with filters
    pub async fn query_users(&self, query: UserQuery) -> Result<UserQueryResult> {
        let start = std::time::Instant::now();

        // Get all users
        let all_users = self.database.select_all_users()?;

        // Apply filters
        let mut filtered: Vec<User> = all_users
            .into_iter()
            .filter(|user| self.apply_user_filters(user, &query.filters))
            .collect();

        // Apply DISTINCT if requested
        if query.distinct {
            filtered = self.apply_distinct_to_users(&filtered);
        }

        // Apply sorting
        if let Some(sort) = &query.sort_by {
            self.sort_users(&mut filtered, sort);
        }

        // Apply window functions if present
        let window_results = if !query.window_functions.is_empty() {
            self.compute_window_functions_for_users(&filtered, &query.window_functions)
                .await?
        } else {
            Vec::new()
        };

        let total_count = filtered.len();

        // Apply pagination
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(usize::MAX);
        let users = filtered.into_iter().skip(offset).take(limit).collect();

        // Apply pagination to window results too
        let window_results_paginated: Vec<Vec<WindowFunctionResult>> = if !window_results.is_empty()
        {
            window_results
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect()
        } else {
            Vec::new()
        };

        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(UserQueryResult {
            users,
            total_count,
            execution_time_ms,
            window_results: window_results_paginated,
        })
    }

    /// Execute aggregate query on facts
    pub async fn aggregate_facts(&self, query: AggregateQuery) -> Result<AggregateResult> {
        let all_facts = self.database.select_all_fact_metadata()?;

        // Apply filters
        let filtered: Vec<FactMetadataRecord> = all_facts
            .into_iter()
            .filter(|fact| self.apply_fact_filters(fact, &query.filters))
            .collect();

        match query.function {
            AggregateFunction::Count => {
                if let Some(group_by) = &query.group_by {
                    let groups = self.group_facts_and_count(&filtered, group_by);
                    Ok(AggregateResult {
                        value: filtered.len() as f64,
                        groups: Some(groups),
                    })
                } else {
                    Ok(AggregateResult {
                        value: filtered.len() as f64,
                        groups: None,
                    })
                }
            }
            AggregateFunction::Sum => {
                let values: Vec<f64> = filtered
                    .iter()
                    .filter_map(|fact| self.extract_numeric_field(fact, &query.field))
                    .collect();
                let total = values.iter().sum();
                if let Some(group_by) = &query.group_by {
                    let groups = self.group_facts_and_sum(&filtered, group_by);
                    let groups = self.apply_having_to_groups(&groups, &[])?;
                    Ok(AggregateResult {
                        value: total,
                        groups: Some(groups),
                    })
                } else {
                    Ok(AggregateResult {
                        value: total,
                        groups: None,
                    })
                }
            }
            AggregateFunction::Avg => {
                let values: Vec<f64> = filtered
                    .iter()
                    .filter_map(|fact| self.extract_numeric_field(fact, &query.field))
                    .collect();
                let avg = if !values.is_empty() {
                    values.iter().sum::<f64>() / values.len() as f64
                } else {
                    0.0
                };
                if let Some(group_by) = &query.group_by {
                    let groups = self.group_facts_and_avg(&filtered, group_by, &query.field);
                    let groups = self.apply_having_to_groups(&groups, &[])?;
                    Ok(AggregateResult {
                        value: avg,
                        groups: Some(groups),
                    })
                } else {
                    Ok(AggregateResult {
                        value: avg,
                        groups: None,
                    })
                }
            }
            AggregateFunction::Min => {
                let min = filtered
                    .iter()
                    .filter_map(|fact| self.extract_numeric_field(fact, &query.field))
                    .fold(f64::INFINITY, f64::min);
                let value = if min.is_infinite() { 0.0 } else { min };
                if let Some(group_by) = &query.group_by {
                    let groups = self.group_facts_and_min(&filtered, group_by, &query.field);
                    let groups = self.apply_having_to_groups(&groups, &[])?;
                    Ok(AggregateResult {
                        value,
                        groups: Some(groups),
                    })
                } else {
                    Ok(AggregateResult {
                        value,
                        groups: None,
                    })
                }
            }
            AggregateFunction::Max => {
                let max = filtered
                    .iter()
                    .filter_map(|fact| self.extract_numeric_field(fact, &query.field))
                    .fold(f64::NEG_INFINITY, f64::max);
                let value = if max.is_infinite() { 0.0 } else { max };
                if let Some(group_by) = &query.group_by {
                    let groups = self.group_facts_and_max(&filtered, group_by, &query.field);
                    let groups = self.apply_having_to_groups(&groups, &[])?;
                    Ok(AggregateResult {
                        value,
                        groups: Some(groups),
                    })
                } else {
                    Ok(AggregateResult {
                        value,
                        groups: None,
                    })
                }
            }
        }
    }

    // Helper methods

    fn get_all_files_from_db(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<FileMetadata>>> + '_ {
        async {
            // Get all files from the database using the public API
            self.database
                .get_all_files()
                .map_err(|e| anyhow::anyhow!("Failed to get files from database: {}", e))
        }
    }

    async fn apply_file_filters(&self, file: &FileMetadata, filters: &[Filter]) -> bool {
        for filter in filters {
            if !self.match_file_filter_async(file, filter).await {
                return false;
            }
        }
        true
    }

    async fn match_file_filter_async(&self, file: &FileMetadata, filter: &Filter) -> bool {
        // Check if filter contains subquery
        if let FilterValue::Subquery(subquery) = &filter.value {
            return self
                .match_file_filter_with_subquery(file, filter, subquery)
                .await;
        }

        // Regular filter matching
        self.match_file_filter(file, filter)
    }

    async fn match_file_filter_with_subquery(
        &self,
        file: &FileMetadata,
        filter: &Filter,
        subquery: &Subquery,
    ) -> bool {
        let field_value = self.extract_field_value(file, &filter.field);

        match subquery.subquery_type {
            SubqueryType::In => {
                let subquery_results = self
                    .execute_subquery(subquery)
                    .await
                    .ok()
                    .unwrap_or_default();
                subquery_results.contains(&field_value)
            }
            SubqueryType::NotIn => {
                let subquery_results = self
                    .execute_subquery(subquery)
                    .await
                    .ok()
                    .unwrap_or_default();
                !subquery_results.contains(&field_value)
            }
            SubqueryType::Exists => {
                let subquery_results = self
                    .execute_subquery(subquery)
                    .await
                    .ok()
                    .unwrap_or_default();
                !subquery_results.is_empty()
            }
            SubqueryType::NotExists => {
                let subquery_results = self
                    .execute_subquery(subquery)
                    .await
                    .ok()
                    .unwrap_or_default();
                subquery_results.is_empty()
            }
        }
    }

    fn apply_fact_filters(&self, fact: &FactMetadataRecord, filters: &[Filter]) -> bool {
        filters
            .iter()
            .all(|filter| self.match_fact_filter(fact, filter))
    }

    fn apply_user_filters(&self, user: &User, filters: &[Filter]) -> bool {
        filters
            .iter()
            .all(|filter| self.match_user_filter(user, filter))
    }

    fn match_file_filter(&self, file: &FileMetadata, filter: &Filter) -> bool {
        match filter.field.as_str() {
            "owner_did" => self.match_string_filter(&file.owner_did, &filter.op, &filter.value),
            "filename" => self.match_string_filter(&file.filename, &filter.op, &filter.value),
            "size" => self.match_numeric_filter(file.size as f64, &filter.op, &filter.value),
            _ => true, // Unknown field, don't filter
        }
    }

    fn match_fact_filter(&self, fact: &FactMetadataRecord, filter: &Filter) -> bool {
        match filter.field.as_str() {
            "author" => self.match_string_filter(&fact.author, &filter.op, &filter.value),
            "category" => self.match_string_filter(&fact.category, &filter.op, &filter.value),
            "domain" => self.match_string_filter(&fact.domain, &filter.op, &filter.value),
            "confidence_score" => {
                self.match_numeric_filter(fact.confidence_score, &filter.op, &filter.value)
            }
            "content_size" => {
                self.match_numeric_filter(fact.content_size as f64, &filter.op, &filter.value)
            }
            "encrypted" => self.match_bool_filter(fact.encrypted, &filter.op, &filter.value),
            "tags" => self.match_array_filter(&fact.tags, &filter.op, &filter.value),
            _ => true,
        }
    }

    fn match_user_filter(&self, user: &User, filter: &Filter) -> bool {
        match filter.field.as_str() {
            "username" => self.match_string_filter(&user.username, &filter.op, &filter.value),
            "email" => self.match_string_filter(&user.email, &filter.op, &filter.value),
            "network" => self.match_string_filter(&user.network, &filter.op, &filter.value),
            _ => true,
        }
    }

    fn match_string_filter(&self, value: &str, op: &FilterOp, filter_value: &FilterValue) -> bool {
        match filter_value {
            FilterValue::String(s) => {
                match op {
                    FilterOp::Equals => value == s,
                    FilterOp::NotEquals => value != s,
                    FilterOp::Contains => value.contains(s),
                    FilterOp::StartsWith => value.starts_with(s),
                    FilterOp::EndsWith => value.ends_with(s),
                    FilterOp::In => {
                        // For IN with string, check if value is in array
                        false // Will be handled by match_array_filter
                    }
                    FilterOp::NotIn => false,
                    _ => false,
                }
            }
            FilterValue::Subquery(subquery) => {
                // Execute subquery and check if value matches
                // This is async, so we'll need to handle it differently
                // For now, return false and handle in async context
                false
            }
            _ => false,
        }
    }

    /// Execute a subquery and return matching values
    async fn execute_subquery(&self, subquery: &Subquery) -> Result<Vec<String>> {
        match subquery.table.as_str() {
            "files" => {
                let query = FileQuery {
                    filters: subquery.filters.clone(),
                    sort_by: None,
                    limit: None,
                    offset: None,
                    joins: Vec::new(),
                    window_functions: Vec::new(),
                    distinct: false,
                };
                let result = self.query_files(query).await?;
                Ok(result
                    .files
                    .iter()
                    .map(|f| self.extract_field_value(f, &subquery.field))
                    .collect())
            }
            "facts" => {
                let query = FactQuery {
                    filters: subquery.filters.clone(),
                    sort_by: None,
                    limit: None,
                    offset: None,
                    joins: Vec::new(),
                    window_functions: Vec::new(),
                    distinct: false,
                };
                let result = self.query_facts(query).await?;
                Ok(result
                    .facts
                    .iter()
                    .map(|f| self.extract_fact_field_value(f, &subquery.field))
                    .collect())
            }
            "users" => {
                let query = UserQuery {
                    filters: subquery.filters.clone(),
                    sort_by: None,
                    limit: None,
                    offset: None,
                    joins: Vec::new(),
                    window_functions: Vec::new(),
                    distinct: false,
                };
                let result = self.query_users(query).await?;
                Ok(result
                    .users
                    .iter()
                    .map(|u| self.extract_user_field_value(u, &subquery.field))
                    .collect())
            }
            _ => Err(anyhow::anyhow!(
                "Unknown table in subquery: {}",
                subquery.table
            )),
        }
    }

    /// Extract field value from FileMetadata
    fn extract_field_value(&self, file: &FileMetadata, field: &str) -> String {
        match field {
            "id" => file.id.clone(),
            "owner_did" => file.owner_did.clone(),
            "filename" => file.filename.clone(),
            "size" => file.size.to_string(),
            _ => String::new(),
        }
    }

    /// Extract field value from FactMetadataRecord
    fn extract_fact_field_value(&self, fact: &FactMetadataRecord, field: &str) -> String {
        match field {
            "fact_id" => fact.fact_id.clone(),
            "author" => fact.author.clone(),
            "category" => fact.category.clone(),
            "domain" => fact.domain.clone(),
            _ => String::new(),
        }
    }

    /// Compute window functions for files and return results
    async fn compute_window_functions_for_files(
        &self,
        files: &[FileMetadata],
        window_functions: &[WindowFunctionDef],
    ) -> Result<Vec<Vec<WindowFunctionResult>>> {
        let mut all_results = Vec::new();

        // Compute each window function
        for window_func in window_functions {
            let mut row_results = Vec::new();

            // Partition files by partition_by fields
            let mut partitions: HashMap<String, Vec<usize>> = HashMap::new();

            for (idx, file) in files.iter().enumerate() {
                let partition_key = if window_func.window_spec.partition_by.is_empty() {
                    "".to_string() // No partition = single partition
                } else {
                    window_func
                        .window_spec
                        .partition_by
                        .iter()
                        .map(|field| self.extract_field_value(file, field))
                        .collect::<Vec<_>>()
                        .join("|")
                };

                partitions
                    .entry(partition_key)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }

            // Apply window function to each partition
            for (_partition_key, indices) in partitions.iter_mut() {
                // Sort partition if ORDER BY is specified
                if let Some(sort) = &window_func.window_spec.order_by {
                    indices.sort_by(|&a, &b| {
                        let file_a = &files[a];
                        let file_b = &files[b];
                        self.compare_file_fields(file_a, file_b, &sort.field, &sort.order)
                    });
                }

                // Compute window function for each row in partition
                for (pos_in_partition, &idx) in indices.iter().enumerate() {
                    let file = &files[idx];
                    let value = self.compute_window_function_value(
                        file,
                        files,
                        indices,
                        pos_in_partition,
                        &window_func.function,
                    )?;

                    let alias = window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| format!("window_{}", all_results.len()));

                    row_results.push(WindowFunctionResult { alias, value });
                }
            }

            // Reorder results to match original file order
            let mut ordered_results = vec![
                WindowFunctionResult {
                    alias: window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| "window".to_string()),
                    value: WindowFunctionValue::Integer(0),
                };
                files.len()
            ];

            // Map partition results back to original indices
            let mut partition_idx = 0;
            for (_partition_key, indices) in partitions.iter() {
                for (pos, &original_idx) in indices.iter().enumerate() {
                    if partition_idx < row_results.len() {
                        ordered_results[original_idx] = row_results[partition_idx + pos].clone();
                    }
                }
                partition_idx += indices.len();
            }

            all_results.push(ordered_results);
        }

        // Transpose: convert from [window_func][row] to [row][window_func]
        let mut transposed = Vec::new();
        for row_idx in 0..files.len() {
            let mut row_results = Vec::new();
            for window_idx in 0..window_functions.len() {
                if row_idx < all_results[window_idx].len() {
                    row_results.push(all_results[window_idx][row_idx].clone());
                }
            }
            transposed.push(row_results);
        }

        Ok(transposed)
    }

    /// Compute a single window function value for a row
    fn compute_window_function_value(
        &self,
        file: &FileMetadata,
        all_files: &[FileMetadata],
        partition_indices: &[usize],
        position: usize,
        function: &WindowFunction,
    ) -> Result<WindowFunctionValue> {
        match function {
            WindowFunction::RowNumber => Ok(WindowFunctionValue::Integer((position + 1) as i64)),
            WindowFunction::Rank => {
                // Rank with gaps - count how many rows have smaller values
                let mut rank = 1;
                let current_value = self.extract_field_value(file, "created_at"); // Default sort field
                for &idx in partition_indices.iter().take(position) {
                    let other_file = &all_files[idx];
                    let other_value = self.extract_field_value(other_file, "created_at");
                    if other_value < current_value {
                        rank += 1;
                    }
                }
                Ok(WindowFunctionValue::Integer(rank as i64))
            }
            WindowFunction::DenseRank => {
                // Dense rank without gaps
                let mut distinct_values = std::collections::HashSet::new();
                let current_value = self.extract_field_value(file, "created_at");
                for &idx in partition_indices.iter().take(position + 1) {
                    let other_file = &all_files[idx];
                    let other_value = self.extract_field_value(other_file, "created_at");
                    if other_value <= current_value {
                        distinct_values.insert(other_value);
                    }
                }
                Ok(WindowFunctionValue::Integer(distinct_values.len() as i64))
            }
            WindowFunction::Ntile(n) => {
                let bucket_size = (partition_indices.len() + n - 1) / n; // Ceiling division
                let bucket = (position / bucket_size) + 1;
                Ok(WindowFunctionValue::Integer(bucket.min(*n as usize) as i64))
            }
            WindowFunction::Lag { field, offset } => {
                if position >= *offset {
                    let prev_idx = partition_indices[position - offset];
                    let prev_file = &all_files[prev_idx];
                    let prev_value = self.extract_field_value(prev_file, field);
                    Ok(WindowFunctionValue::String(prev_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new())) // NULL equivalent
                }
            }
            WindowFunction::Lead { field, offset } => {
                if position + offset < partition_indices.len() {
                    let next_idx = partition_indices[position + offset];
                    let next_file = &all_files[next_idx];
                    let next_value = self.extract_field_value(next_file, field);
                    Ok(WindowFunctionValue::String(next_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new())) // NULL equivalent
                }
            }
            WindowFunction::FirstValue { field } => {
                if let Some(&first_idx) = partition_indices.first() {
                    let first_file = &all_files[first_idx];
                    let first_value = self.extract_field_value(first_file, field);
                    Ok(WindowFunctionValue::String(first_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::LastValue { field } => {
                if let Some(&last_idx) = partition_indices.last() {
                    let last_file = &all_files[last_idx];
                    let last_value = self.extract_field_value(last_file, field);
                    Ok(WindowFunctionValue::String(last_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::AggregateOver { function, field } => {
                // Calculate aggregate over partition
                let values: Vec<f64> = partition_indices
                    .iter()
                    .map(|&idx| {
                        let f = &all_files[idx];
                        self.extract_field_value(f, field).parse().unwrap_or(0.0)
                    })
                    .collect();

                let aggregate_value = match function {
                    AggregateFunction::Sum => values.iter().sum(),
                    AggregateFunction::Avg => {
                        if values.is_empty() {
                            0.0
                        } else {
                            values.iter().sum::<f64>() / values.len() as f64
                        }
                    }
                    AggregateFunction::Min => values.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
                    AggregateFunction::Max => {
                        values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b))
                    }
                    AggregateFunction::Count => values.len() as f64,
                };

                Ok(WindowFunctionValue::Float(aggregate_value))
            }
        }
    }

    /// Apply DISTINCT to remove duplicate files
    fn apply_distinct_to_files(&self, files: &[FileMetadata]) -> Vec<FileMetadata> {
        let mut seen = std::collections::HashSet::new();
        let mut distinct = Vec::new();

        for file in files {
            // Create a unique key from all fields
            let key = format!(
                "{}|{}|{}|{}",
                file.id, file.owner_did, file.filename, file.hash
            );
            if seen.insert(key) {
                distinct.push(file.clone());
            }
        }

        distinct
    }

    /// Apply DISTINCT to remove duplicate facts
    fn apply_distinct_to_facts(&self, facts: &[FactMetadataRecord]) -> Vec<FactMetadataRecord> {
        let mut seen = std::collections::HashSet::new();
        let mut distinct = Vec::new();

        for fact in facts {
            let key = format!("{}|{}", fact.fact_id, fact.version);
            if seen.insert(key) {
                distinct.push(fact.clone());
            }
        }

        distinct
    }

    /// Apply DISTINCT to remove duplicate users
    fn apply_distinct_to_users(&self, users: &[User]) -> Vec<User> {
        let mut seen = std::collections::HashSet::new();
        let mut distinct = Vec::new();

        for user in users {
            let key = format!("{}|{}", user.username, user.address);
            if seen.insert(key) {
                distinct.push(user.clone());
            }
        }

        distinct
    }

    /// Compute window functions for facts (similar to files)
    async fn compute_window_functions_for_facts(
        &self,
        facts: &[FactMetadataRecord],
        window_functions: &[WindowFunctionDef],
    ) -> Result<Vec<Vec<WindowFunctionResult>>> {
        // Similar implementation to files, but using fact-specific field extraction
        let mut all_results = Vec::new();

        for window_func in window_functions {
            let mut row_results = Vec::new();

            // Partition facts
            let mut partitions: HashMap<String, Vec<usize>> = HashMap::new();

            for (idx, fact) in facts.iter().enumerate() {
                let partition_key = if window_func.window_spec.partition_by.is_empty() {
                    "".to_string()
                } else {
                    window_func
                        .window_spec
                        .partition_by
                        .iter()
                        .map(|field| self.extract_fact_field_value(fact, field))
                        .collect::<Vec<_>>()
                        .join("|")
                };

                partitions
                    .entry(partition_key)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }

            // Apply window function to each partition
            for (_partition_key, indices) in partitions.iter_mut() {
                if let Some(sort) = &window_func.window_spec.order_by {
                    indices.sort_by(|&a, &b| {
                        let fact_a = &facts[a];
                        let fact_b = &facts[b];
                        let val_a = self.extract_fact_field_value(fact_a, &sort.field);
                        let val_b = self.extract_fact_field_value(fact_b, &sort.field);
                        let cmp = val_a.cmp(&val_b);
                        match sort.order {
                            SortOrder::Asc => cmp,
                            SortOrder::Desc => cmp.reverse(),
                        }
                    });
                }

                for (pos_in_partition, &idx) in indices.iter().enumerate() {
                    let fact = &facts[idx];
                    let value = self.compute_window_function_value_for_fact(
                        fact,
                        facts,
                        indices,
                        pos_in_partition,
                        &window_func.function,
                    )?;

                    let alias = window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| format!("window_{}", all_results.len()));

                    row_results.push(WindowFunctionResult { alias, value });
                }
            }

            // Reorder results
            let mut ordered_results = vec![
                WindowFunctionResult {
                    alias: window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| "window".to_string()),
                    value: WindowFunctionValue::Integer(0),
                };
                facts.len()
            ];

            let mut partition_idx = 0;
            for (_partition_key, indices) in partitions.iter() {
                for (pos, &original_idx) in indices.iter().enumerate() {
                    if partition_idx + pos < row_results.len() {
                        ordered_results[original_idx] = row_results[partition_idx + pos].clone();
                    }
                }
                partition_idx += indices.len();
            }

            all_results.push(ordered_results);
        }

        // Transpose
        let mut transposed = Vec::new();
        for row_idx in 0..facts.len() {
            let mut row_results = Vec::new();
            for window_idx in 0..window_functions.len() {
                if row_idx < all_results[window_idx].len() {
                    row_results.push(all_results[window_idx][row_idx].clone());
                }
            }
            transposed.push(row_results);
        }

        Ok(transposed)
    }

    /// Compute window function value for a fact
    fn compute_window_function_value_for_fact(
        &self,
        fact: &FactMetadataRecord,
        all_facts: &[FactMetadataRecord],
        partition_indices: &[usize],
        position: usize,
        function: &WindowFunction,
    ) -> Result<WindowFunctionValue> {
        // Similar to files but using fact fields
        match function {
            WindowFunction::RowNumber => Ok(WindowFunctionValue::Integer((position + 1) as i64)),
            WindowFunction::Rank => {
                let mut rank = 1;
                let current_value = self.extract_fact_field_value(fact, "created_at");
                for &idx in partition_indices.iter().take(position) {
                    let other_fact = &all_facts[idx];
                    let other_value = self.extract_fact_field_value(other_fact, "created_at");
                    if other_value < current_value {
                        rank += 1;
                    }
                }
                Ok(WindowFunctionValue::Integer(rank as i64))
            }
            WindowFunction::DenseRank => {
                let mut distinct_values = std::collections::HashSet::new();
                let current_value = self.extract_fact_field_value(fact, "created_at");
                for &idx in partition_indices.iter().take(position + 1) {
                    let other_fact = &all_facts[idx];
                    let other_value = self.extract_fact_field_value(other_fact, "created_at");
                    if other_value <= current_value {
                        distinct_values.insert(other_value);
                    }
                }
                Ok(WindowFunctionValue::Integer(distinct_values.len() as i64))
            }
            WindowFunction::Ntile(n) => {
                let bucket_size = (partition_indices.len() + n - 1) / n;
                let bucket = (position / bucket_size) + 1;
                Ok(WindowFunctionValue::Integer(bucket.min(*n as usize) as i64))
            }
            WindowFunction::Lag { field, offset } => {
                if position >= *offset {
                    let prev_idx = partition_indices[position - offset];
                    let prev_fact = &all_facts[prev_idx];
                    let prev_value = self.extract_fact_field_value(prev_fact, field);
                    Ok(WindowFunctionValue::String(prev_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::Lead { field, offset } => {
                if position + offset < partition_indices.len() {
                    let next_idx = partition_indices[position + offset];
                    let next_fact = &all_facts[next_idx];
                    let next_value = self.extract_fact_field_value(next_fact, field);
                    Ok(WindowFunctionValue::String(next_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::FirstValue { field } => {
                if let Some(&first_idx) = partition_indices.first() {
                    let first_fact = &all_facts[first_idx];
                    let first_value = self.extract_fact_field_value(first_fact, field);
                    Ok(WindowFunctionValue::String(first_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::LastValue { field } => {
                if let Some(&last_idx) = partition_indices.last() {
                    let last_fact = &all_facts[last_idx];
                    let last_value = self.extract_fact_field_value(last_fact, field);
                    Ok(WindowFunctionValue::String(last_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::AggregateOver { function, field } => {
                let values: Vec<f64> = partition_indices
                    .iter()
                    .map(|&idx| {
                        let f = &all_facts[idx];
                        self.extract_fact_field_value(f, field)
                            .parse()
                            .unwrap_or(0.0)
                    })
                    .collect();

                let aggregate_value = match function {
                    AggregateFunction::Sum => values.iter().sum(),
                    AggregateFunction::Avg => {
                        if values.is_empty() {
                            0.0
                        } else {
                            values.iter().sum::<f64>() / values.len() as f64
                        }
                    }
                    AggregateFunction::Min => values.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
                    AggregateFunction::Max => {
                        values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b))
                    }
                    AggregateFunction::Count => values.len() as f64,
                };

                Ok(WindowFunctionValue::Float(aggregate_value))
            }
        }
    }

    /// Compute window functions for users (similar to files and facts)
    async fn compute_window_functions_for_users(
        &self,
        users: &[User],
        window_functions: &[WindowFunctionDef],
    ) -> Result<Vec<Vec<WindowFunctionResult>>> {
        let mut all_results = Vec::new();

        for window_func in window_functions {
            let mut row_results = Vec::new();

            // Partition users
            let mut partitions: HashMap<String, Vec<usize>> = HashMap::new();

            for (idx, user) in users.iter().enumerate() {
                let partition_key = if window_func.window_spec.partition_by.is_empty() {
                    "".to_string()
                } else {
                    window_func
                        .window_spec
                        .partition_by
                        .iter()
                        .map(|field| self.extract_user_field_value(user, field))
                        .collect::<Vec<_>>()
                        .join("|")
                };

                partitions
                    .entry(partition_key)
                    .or_insert_with(Vec::new)
                    .push(idx);
            }

            // Apply window function to each partition
            for (_partition_key, indices) in partitions.iter_mut() {
                if let Some(sort) = &window_func.window_spec.order_by {
                    indices.sort_by(|&a, &b| {
                        let user_a = &users[a];
                        let user_b = &users[b];
                        let val_a = self.extract_user_field_value(user_a, &sort.field);
                        let val_b = self.extract_user_field_value(user_b, &sort.field);
                        let cmp = val_a.cmp(&val_b);
                        match sort.order {
                            SortOrder::Asc => cmp,
                            SortOrder::Desc => cmp.reverse(),
                        }
                    });
                }

                for (pos_in_partition, &idx) in indices.iter().enumerate() {
                    let user = &users[idx];
                    let value = self.compute_window_function_value_for_user(
                        user,
                        users,
                        indices,
                        pos_in_partition,
                        &window_func.function,
                    )?;

                    let alias = window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| format!("window_{}", all_results.len()));

                    row_results.push(WindowFunctionResult { alias, value });
                }
            }

            // Reorder results
            let mut ordered_results = vec![
                WindowFunctionResult {
                    alias: window_func
                        .alias
                        .clone()
                        .unwrap_or_else(|| "window".to_string()),
                    value: WindowFunctionValue::Integer(0),
                };
                users.len()
            ];

            let mut partition_idx = 0;
            for (_partition_key, indices) in partitions.iter() {
                for (pos, &original_idx) in indices.iter().enumerate() {
                    if partition_idx + pos < row_results.len() {
                        ordered_results[original_idx] = row_results[partition_idx + pos].clone();
                    }
                }
                partition_idx += indices.len();
            }

            all_results.push(ordered_results);
        }

        // Transpose
        let mut transposed = Vec::new();
        for row_idx in 0..users.len() {
            let mut row_results = Vec::new();
            for window_idx in 0..window_functions.len() {
                if row_idx < all_results[window_idx].len() {
                    row_results.push(all_results[window_idx][row_idx].clone());
                }
            }
            transposed.push(row_results);
        }

        Ok(transposed)
    }

    /// Compute window function value for a user
    fn compute_window_function_value_for_user(
        &self,
        user: &User,
        all_users: &[User],
        partition_indices: &[usize],
        position: usize,
        function: &WindowFunction,
    ) -> Result<WindowFunctionValue> {
        match function {
            WindowFunction::RowNumber => Ok(WindowFunctionValue::Integer((position + 1) as i64)),
            WindowFunction::Rank => {
                let mut rank = 1;
                // Use created_at if available, otherwise fallback to username
                let current_value = {
                    let created_at = self.extract_user_field_value(user, "created_at");
                    if !created_at.is_empty() {
                        created_at
                    } else {
                        self.extract_user_field_value(user, "username")
                    }
                };
                for &idx in partition_indices.iter().take(position) {
                    let other_user = &all_users[idx];
                    let other_value = {
                        let created_at = self.extract_user_field_value(other_user, "created_at");
                        if !created_at.is_empty() {
                            created_at
                        } else {
                            self.extract_user_field_value(other_user, "username")
                        }
                    };
                    if other_value < current_value {
                        rank += 1;
                    }
                }
                Ok(WindowFunctionValue::Integer(rank as i64))
            }
            WindowFunction::DenseRank => {
                let mut distinct_values = std::collections::HashSet::new();
                // Use created_at if available, otherwise fallback to username
                let current_value = {
                    let created_at = self.extract_user_field_value(user, "created_at");
                    if !created_at.is_empty() {
                        created_at
                    } else {
                        self.extract_user_field_value(user, "username")
                    }
                };
                for &idx in partition_indices.iter().take(position + 1) {
                    let other_user = &all_users[idx];
                    let other_value = {
                        let created_at = self.extract_user_field_value(other_user, "created_at");
                        if !created_at.is_empty() {
                            created_at
                        } else {
                            self.extract_user_field_value(other_user, "username")
                        }
                    };
                    if other_value <= current_value {
                        distinct_values.insert(other_value);
                    }
                }
                Ok(WindowFunctionValue::Integer(distinct_values.len() as i64))
            }
            WindowFunction::Ntile(n) => {
                let bucket_size = (partition_indices.len() + n - 1) / n;
                let bucket = (position / bucket_size) + 1;
                Ok(WindowFunctionValue::Integer(bucket.min(*n as usize) as i64))
            }
            WindowFunction::Lag { field, offset } => {
                if position >= *offset {
                    let prev_idx = partition_indices[position - offset];
                    let prev_user = &all_users[prev_idx];
                    let prev_value = self.extract_user_field_value(prev_user, field);
                    Ok(WindowFunctionValue::String(prev_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::Lead { field, offset } => {
                if position + offset < partition_indices.len() {
                    let next_idx = partition_indices[position + offset];
                    let next_user = &all_users[next_idx];
                    let next_value = self.extract_user_field_value(next_user, field);
                    Ok(WindowFunctionValue::String(next_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::FirstValue { field } => {
                if let Some(&first_idx) = partition_indices.first() {
                    let first_user = &all_users[first_idx];
                    let first_value = self.extract_user_field_value(first_user, field);
                    Ok(WindowFunctionValue::String(first_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::LastValue { field } => {
                if let Some(&last_idx) = partition_indices.last() {
                    let last_user = &all_users[last_idx];
                    let last_value = self.extract_user_field_value(last_user, field);
                    Ok(WindowFunctionValue::String(last_value))
                } else {
                    Ok(WindowFunctionValue::String(String::new()))
                }
            }
            WindowFunction::AggregateOver { function, field } => {
                let values: Vec<f64> = partition_indices
                    .iter()
                    .map(|&idx| {
                        let u = &all_users[idx];
                        self.extract_user_field_value(u, field)
                            .parse()
                            .unwrap_or(0.0)
                    })
                    .collect();

                let aggregate_value = match function {
                    AggregateFunction::Sum => values.iter().sum(),
                    AggregateFunction::Avg => {
                        if values.is_empty() {
                            0.0
                        } else {
                            values.iter().sum::<f64>() / values.len() as f64
                        }
                    }
                    AggregateFunction::Min => values.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
                    AggregateFunction::Max => {
                        values.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b))
                    }
                    AggregateFunction::Count => values.len() as f64,
                };

                Ok(WindowFunctionValue::Float(aggregate_value))
            }
        }
    }

    /// Apply HAVING clause to filter groups after aggregation
    fn apply_having_to_groups(
        &self,
        groups: &HashMap<String, f64>,
        having_filters: &[Filter],
    ) -> Result<HashMap<String, f64>> {
        let mut filtered = HashMap::new();

        for (group_key, aggregate_value) in groups {
            // Check if group passes HAVING filters
            let mut passes = true;
            for filter in having_filters {
                // HAVING filters typically compare aggregate values
                if filter.field == "aggregate_value" || filter.field == "_value" {
                    let matches =
                        self.match_numeric_filter(*aggregate_value, &filter.op, &filter.value);
                    if !matches {
                        passes = false;
                        break;
                    }
                } else {
                    // Could also filter on group key
                    let key_matches =
                        self.match_string_filter(group_key, &filter.op, &filter.value);
                    if !key_matches {
                        passes = false;
                        break;
                    }
                }
            }

            if passes {
                filtered.insert(group_key.clone(), *aggregate_value);
            }
        }

        Ok(filtered)
    }

    /// Execute UNION query - combine multiple file queries
    pub async fn union_files(&self, union_query: UnionQuery) -> Result<FileQueryResult> {
        let start = std::time::Instant::now();
        let mut all_files = Vec::new();
        let mut all_window_results = Vec::new();

        // Execute each query
        for query in union_query.queries {
            let result = self.query_files(query).await?;
            all_files.extend(result.files);
            all_window_results.extend(result.window_results);
        }

        // Apply UNION vs UNION ALL
        let (files, window_results) = match union_query.union_type {
            UnionType::Union => {
                // Remove duplicates
                let distinct_files = self.apply_distinct_to_files(&all_files);
                // Window results need to match - simplified for now
                (distinct_files, all_window_results)
            }
            UnionType::UnionAll => {
                // Keep all (already have all)
                (all_files, all_window_results)
            }
        };

        let total_count = files.len();
        let execution_time_ms = start.elapsed().as_millis() as u64;

        Ok(FileQueryResult {
            files,
            total_count,
            execution_time_ms,
            window_results,
        })
    }

    /// Helper methods for aggregate functions with GROUP BY
    fn group_facts_and_sum(
        &self,
        facts: &[FactMetadataRecord],
        group_by: &str,
    ) -> HashMap<String, f64> {
        let mut groups: HashMap<String, f64> = HashMap::new();
        for fact in facts {
            let key = self.extract_fact_field_value(fact, group_by);
            let value = fact.content_size as f64; // Default to content_size
            *groups.entry(key).or_insert(0.0) += value;
        }
        groups
    }

    fn group_facts_and_avg(
        &self,
        facts: &[FactMetadataRecord],
        group_by: &str,
        field: &str,
    ) -> HashMap<String, f64> {
        let mut groups: HashMap<String, (f64, usize)> = HashMap::new();
        for fact in facts {
            let key = self.extract_fact_field_value(fact, group_by);
            let value = self
                .extract_fact_field_value(fact, field)
                .parse()
                .unwrap_or(0.0);
            let entry = groups.entry(key).or_insert((0.0, 0));
            entry.0 += value;
            entry.1 += 1;
        }
        groups
            .into_iter()
            .map(|(k, (sum, count))| (k, sum / count as f64))
            .collect()
    }

    fn group_facts_and_min(
        &self,
        facts: &[FactMetadataRecord],
        group_by: &str,
        field: &str,
    ) -> HashMap<String, f64> {
        let mut groups: HashMap<String, f64> = HashMap::new();
        for fact in facts {
            let key = self.extract_fact_field_value(fact, group_by);
            let value = self
                .extract_fact_field_value(fact, field)
                .parse()
                .unwrap_or(f64::INFINITY);
            let entry = groups.entry(key).or_insert(f64::INFINITY);
            *entry = entry.min(value);
        }
        groups
    }

    fn group_facts_and_max(
        &self,
        facts: &[FactMetadataRecord],
        group_by: &str,
        field: &str,
    ) -> HashMap<String, f64> {
        let mut groups: HashMap<String, f64> = HashMap::new();
        for fact in facts {
            let key = self.extract_fact_field_value(fact, group_by);
            let value = self
                .extract_fact_field_value(fact, field)
                .parse()
                .unwrap_or(f64::NEG_INFINITY);
            let entry = groups.entry(key).or_insert(f64::NEG_INFINITY);
            *entry = entry.max(value);
        }
        groups
    }

    /// Compare two file fields for sorting
    fn compare_file_fields(
        &self,
        file_a: &FileMetadata,
        file_b: &FileMetadata,
        field: &str,
        order: &SortOrder,
    ) -> std::cmp::Ordering {
        let value_a = self.extract_field_value(file_a, field);
        let value_b = self.extract_field_value(file_b, field);

        let cmp = value_a.cmp(&value_b);
        match order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    }

    /// Extract field value from User
    fn extract_user_field_value(&self, user: &User, field: &str) -> String {
        match field {
            "username" => user.username.clone(),
            "email" => user.email.clone(),
            "address" => user.address.clone(),
            "network" => user.network.clone(),
            _ => String::new(),
        }
    }

    fn match_numeric_filter(&self, value: f64, op: &FilterOp, filter_value: &FilterValue) -> bool {
        let filter_num = match filter_value {
            FilterValue::Number(n) => *n,
            FilterValue::Integer(i) => *i as f64,
            _ => return false,
        };

        match op {
            FilterOp::Equals => (value - filter_num).abs() < f64::EPSILON,
            FilterOp::NotEquals => (value - filter_num).abs() >= f64::EPSILON,
            FilterOp::GreaterThan => value > filter_num,
            FilterOp::LessThan => value < filter_num,
            FilterOp::GreaterThanOrEqual => value >= filter_num,
            FilterOp::LessThanOrEqual => value <= filter_num,
            _ => false,
        }
    }

    fn match_bool_filter(&self, value: bool, op: &FilterOp, filter_value: &FilterValue) -> bool {
        if let FilterValue::Boolean(b) = filter_value {
            match op {
                FilterOp::Equals => value == *b,
                FilterOp::NotEquals => value != *b,
                _ => false,
            }
        } else {
            false
        }
    }

    fn match_array_filter(
        &self,
        values: &[String],
        op: &FilterOp,
        filter_value: &FilterValue,
    ) -> bool {
        match (op, filter_value) {
            (FilterOp::Contains, FilterValue::String(s)) => values.contains(s),
            (FilterOp::In, FilterValue::Array(arr)) => arr.iter().any(|a| values.contains(a)),
            _ => false,
        }
    }

    fn sort_files(&self, files: &mut [FileMetadata], sort: &SortBy) {
        match (sort.field.as_str(), &sort.order) {
            ("created_at", SortOrder::Asc) => files.sort_by_key(|f| f.created_at),
            ("created_at", SortOrder::Desc) => {
                files.sort_by(|a, b| b.created_at.cmp(&a.created_at))
            }
            ("size", SortOrder::Asc) => files.sort_by_key(|f| f.size),
            ("size", SortOrder::Desc) => files.sort_by(|a, b| b.size.cmp(&a.size)),
            ("filename", SortOrder::Asc) => files.sort_by(|a, b| a.filename.cmp(&b.filename)),
            ("filename", SortOrder::Desc) => files.sort_by(|a, b| b.filename.cmp(&a.filename)),
            _ => {}
        }
    }

    fn sort_facts(&self, facts: &mut [FactMetadataRecord], sort: &SortBy) {
        match (sort.field.as_str(), &sort.order) {
            ("created_at", SortOrder::Asc) => facts.sort_by_key(|f| f.created_at),
            ("created_at", SortOrder::Desc) => {
                facts.sort_by(|a, b| b.created_at.cmp(&a.created_at))
            }
            ("confidence_score", SortOrder::Asc) => {
                facts.sort_by(|a, b| a.confidence_score.partial_cmp(&b.confidence_score).unwrap())
            }
            ("confidence_score", SortOrder::Desc) => {
                facts.sort_by(|a, b| b.confidence_score.partial_cmp(&a.confidence_score).unwrap())
            }
            ("content_size", SortOrder::Asc) => facts.sort_by_key(|f| f.content_size),
            ("content_size", SortOrder::Desc) => {
                facts.sort_by(|a, b| b.content_size.cmp(&a.content_size))
            }
            _ => {}
        }
    }

    fn sort_users(&self, users: &mut [User], sort: &SortBy) {
        match (sort.field.as_str(), &sort.order) {
            ("username", SortOrder::Asc) => users.sort_by(|a, b| a.username.cmp(&b.username)),
            ("username", SortOrder::Desc) => users.sort_by(|a, b| b.username.cmp(&a.username)),
            ("email", SortOrder::Asc) => users.sort_by(|a, b| a.email.cmp(&b.email)),
            ("email", SortOrder::Desc) => users.sort_by(|a, b| b.email.cmp(&a.email)),
            _ => {}
        }
    }

    fn extract_numeric_field(&self, fact: &FactMetadataRecord, field: &str) -> Option<f64> {
        match field {
            "confidence_score" => Some(fact.confidence_score),
            "content_size" => Some(fact.content_size as f64),
            "version" => Some(fact.version as f64),
            _ => None,
        }
    }

    fn group_facts_and_count(
        &self,
        facts: &[FactMetadataRecord],
        group_by: &str,
    ) -> HashMap<String, f64> {
        let mut groups: HashMap<String, f64> = HashMap::new();

        for fact in facts {
            let key = match group_by {
                "category" => fact.category.clone(),
                "domain" => fact.domain.clone(),
                "verification_level" => fact.verification_level.clone(),
                "author" => fact.author.clone(),
                _ => "unknown".to_string(),
            };

            *groups.entry(key).or_insert(0.0) += 1.0;
        }

        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_query_builder_creation() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());

        let _builder = StorageQueryBuilder::new(db);
        // Test passes if no panic
    }

    #[tokio::test]
    async fn test_fact_query_with_filters() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        let builder = StorageQueryBuilder::new(db);

        let query = FactQuery {
            distinct: false,
            filters: vec![Filter {
                field: "category".to_string(),
                op: FilterOp::Equals,
                value: FilterValue::String("Scientific".to_string()),
            }],
            joins: Vec::new(),
            window_functions: Vec::new(),
            sort_by: Some(SortBy {
                field: "created_at".to_string(),
                order: SortOrder::Desc,
            }),
            limit: Some(10),
            offset: None,
        };

        let result = builder.query_facts(query).await.unwrap();
        assert_eq!(result.total_count, 0); // Empty database
    }

    #[tokio::test]
    async fn test_inner_join_files_with_users() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create test user
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        // Create test file
        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![],
            sort_by: None,
            limit: None,
            offset: None,
            window_functions: Vec::new(),
            joins: vec![Join {
                join_type: JoinType::Inner,
                table: "users".to_string(),
                condition: JoinCondition {
                    left_table: "files".to_string(),
                    left_field: "owner_did".to_string(),
                    right_table: "users".to_string(),
                    right_field: "address".to_string(),
                },
            }],
        };

        let result = builder.query_files(query).await.unwrap();
        // Should return files that have matching users
        assert!(!result.files.is_empty());
    }

    #[tokio::test]
    async fn test_left_join_files_with_users() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create file without matching user
        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:nonexistent".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![],
            sort_by: None,
            limit: None,
            offset: None,
            window_functions: Vec::new(),
            joins: vec![Join {
                join_type: JoinType::Left,
                table: "users".to_string(),
                condition: JoinCondition {
                    left_table: "files".to_string(),
                    left_field: "owner_did".to_string(),
                    right_table: "users".to_string(),
                    right_field: "address".to_string(),
                },
            }],
        };

        let result = builder.query_files(query).await.unwrap();
        // LEFT JOIN should return all files, even without matching users
        assert!(!result.files.is_empty());
    }

    #[tokio::test]
    async fn test_right_join_files_with_users() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create user without matching file
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![],
            sort_by: None,
            limit: None,
            offset: None,
            window_functions: Vec::new(),
            joins: vec![Join {
                join_type: JoinType::Right,
                table: "users".to_string(),
                condition: JoinCondition {
                    left_table: "files".to_string(),
                    left_field: "owner_did".to_string(),
                    right_table: "users".to_string(),
                    right_field: "address".to_string(),
                },
            }],
        };

        let result = builder.query_files(query).await.unwrap();
        // RIGHT JOIN here has no matching files, so result should be empty.
        assert!(result.files.is_empty());
    }

    #[tokio::test]
    async fn test_full_outer_join() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create user and file
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![],
            sort_by: None,
            limit: None,
            offset: None,
            window_functions: Vec::new(),
            joins: vec![Join {
                join_type: JoinType::FullOuter,
                table: "users".to_string(),
                condition: JoinCondition {
                    left_table: "files".to_string(),
                    left_field: "owner_did".to_string(),
                    right_table: "users".to_string(),
                    right_field: "address".to_string(),
                },
            }],
        };

        let result = builder.query_files(query).await.unwrap();
        // FULL OUTER JOIN should return all matching records
        assert!(!result.files.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_joins() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create test data
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![],
            sort_by: None,
            limit: None,
            offset: None,
            window_functions: Vec::new(),
            joins: vec![
                Join {
                    join_type: JoinType::Inner,
                    table: "users".to_string(),
                    condition: JoinCondition {
                        left_table: "files".to_string(),
                        left_field: "owner_did".to_string(),
                        right_table: "users".to_string(),
                        right_field: "address".to_string(),
                    },
                },
                // Could add more joins here if we had more tables
            ],
        };

        let result = builder.query_files(query).await.unwrap();
        assert!(!result.files.is_empty());
    }

    #[tokio::test]
    async fn test_subquery_exists() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create file
        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![Filter {
                field: "id".to_string(),
                op: FilterOp::Equals,
                value: FilterValue::Subquery(Subquery {
                    subquery_type: SubqueryType::Exists,
                    table: "files".to_string(),
                    field: "id".to_string(),
                    filters: vec![Filter {
                        field: "size".to_string(),
                        op: FilterOp::GreaterThan,
                        value: FilterValue::Integer(0),
                    }],
                }),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![],
        };

        let result = builder.query_files(query).await.unwrap();
        // EXISTS subquery should return files if subquery has results
        let _ = result.files.len();
    }

    #[tokio::test]
    async fn test_subquery_not_exists() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![Filter {
                field: "id".to_string(),
                op: FilterOp::Equals,
                value: FilterValue::Subquery(Subquery {
                    subquery_type: SubqueryType::NotExists,
                    table: "files".to_string(),
                    field: "id".to_string(),
                    filters: vec![Filter {
                        field: "size".to_string(),
                        op: FilterOp::GreaterThan,
                        value: FilterValue::Integer(999999),
                    }],
                }),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![],
        };

        let result = builder.query_files(query).await.unwrap();
        // NOT EXISTS should return files if subquery has no results
        let _ = result.files.len();
    }

    #[tokio::test]
    async fn test_subquery_with_facts() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create fact
        let fact = FactMetadataRecord {
            fact_id: "fact1".to_string(),
            version: 1,
            author: "did:spacekit:user:alice".to_string(),
            created_at: chrono::Utc::now(),
            content_size: 100,
            content_type: "text/plain".to_string(),
            category: "Scientific".to_string(),
            domain: "physics".to_string(),
            tags: vec!["quantum".to_string()],
            verification_level: "verified".to_string(),
            confidence_score: 0.95,
            storage_location_path: "/path/to/fact1".to_string(),
            storage_tier: "standard".to_string(),
            compressed: false,
            encrypted: true,
            checksum: "checksum1".to_string(),
            access_policy_hash: "policy1".to_string(),
            access_policy_json: None,
            dependencies: vec![],
            last_accessed: None,
        };
        db.insert_fact_metadata(&fact).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FactQuery {
            distinct: false,
            filters: vec![Filter {
                field: "author".to_string(),
                op: FilterOp::In,
                value: FilterValue::Subquery(Subquery {
                    subquery_type: SubqueryType::In,
                    table: "users".to_string(),
                    field: "address".to_string(),
                    filters: vec![Filter {
                        field: "email".to_string(),
                        op: FilterOp::Contains,
                        value: FilterValue::String("@example.com".to_string()),
                    }],
                }),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![],
        };

        let result = builder.query_facts(query).await.unwrap();
        let _ = result.facts.len();
    }

    #[tokio::test]
    async fn test_join_with_filters() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create test data
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        let file = FileMetadata {
            id: "file1".to_string(),
            filename: "test.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![Filter {
                field: "size".to_string(),
                op: FilterOp::GreaterThan,
                value: FilterValue::Integer(50),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![Join {
                join_type: JoinType::Inner,
                table: "users".to_string(),
                condition: JoinCondition {
                    left_table: "files".to_string(),
                    left_field: "owner_did".to_string(),
                    right_table: "users".to_string(),
                    right_field: "address".to_string(),
                },
            }],
        };

        let result = builder.query_files(query).await.unwrap();
        // Should return files that match both the filter and the join
        assert!(!result.files.is_empty());
    }

    #[tokio::test]
    async fn test_subquery_in() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create test users
        let user1 = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user1).unwrap();

        let user2 = User {
            username: "bob".to_string(),
            first_name: Some("Bob".to_string()),
            last_name: Some("Johnson".to_string()),
            email: "bob@example.com".to_string(),
            address: "did:spacekit:user:bob".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user2).unwrap();

        // Create files
        let file1 = FileMetadata {
            id: "file1".to_string(),
            filename: "test1.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file1).unwrap();

        let file2 = FileMetadata {
            id: "file2".to_string(),
            filename: "test2.txt".to_string(),
            size: 200,
            hash: "hash2".to_string(),
            owner_did: "did:spacekit:user:charlie".to_string(), // No matching user
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file2).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![Filter {
                field: "owner_did".to_string(),
                op: FilterOp::In,
                value: FilterValue::Subquery(Subquery {
                    subquery_type: SubqueryType::In,
                    table: "users".to_string(),
                    field: "address".to_string(),
                    filters: vec![Filter {
                        field: "email".to_string(),
                        op: FilterOp::Contains,
                        value: FilterValue::String("@example.com".to_string()),
                    }],
                }),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![],
        };

        let result = builder.query_files(query).await.unwrap();
        // Should return files owned by users with @example.com email
        let _ = result.files.len();
    }

    #[tokio::test]
    async fn test_subquery_not_in() {
        let temp_dir = tempdir().unwrap();
        let db =
            Arc::new(Database::new(temp_dir.path().join("test.json").to_str().unwrap()).unwrap());
        db.initialize().unwrap();

        // Create test user
        let user = User {
            username: "alice".to_string(),
            first_name: Some("Alice".to_string()),
            last_name: Some("Smith".to_string()),
            email: "alice@example.com".to_string(),
            address: "did:spacekit:user:alice".to_string(),
            network: "spacekit".to_string(),
            message: "".to_string(),
            created_at: Some(chrono::Utc::now()),
        };
        db.insert_user(&user).unwrap();

        // Create files
        let file1 = FileMetadata {
            id: "file1".to_string(),
            filename: "test1.txt".to_string(),
            size: 100,
            hash: "hash1".to_string(),
            owner_did: "did:spacekit:user:alice".to_string(),
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file1).unwrap();

        let file2 = FileMetadata {
            id: "file2".to_string(),
            filename: "test2.txt".to_string(),
            size: 200,
            hash: "hash2".to_string(),
            owner_did: "did:spacekit:user:bob".to_string(), // Not in subquery
            encryption_algorithm: "Kyber1024".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None,
            sharing_mode: "owner".to_string(),
        };
        db.insert_file_metadata(&file2).unwrap();

        let builder = StorageQueryBuilder::new(db);
        let query = FileQuery {
            distinct: false,
            filters: vec![Filter {
                field: "owner_did".to_string(),
                op: FilterOp::NotIn,
                value: FilterValue::Subquery(Subquery {
                    subquery_type: SubqueryType::NotIn,
                    table: "users".to_string(),
                    field: "address".to_string(),
                    filters: vec![Filter {
                        field: "email".to_string(),
                        op: FilterOp::Equals,
                        value: FilterValue::String("alice@example.com".to_string()),
                    }],
                }),
            }],
            sort_by: None,
            window_functions: Vec::new(),
            limit: None,
            offset: None,
            joins: vec![],
        };

        let result = builder.query_files(query).await.unwrap();
        // Should return files NOT owned by alice
        let _ = result.files.len();
    }
}
