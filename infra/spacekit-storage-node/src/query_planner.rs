//! Query Planner for Storage Node
//!
//! Provides cost-based query optimization, index selection,
//! and join order optimization.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

use crate::sql_query::{FactQuery, FileQuery, Filter, FilterOp, Join, UserQuery};

/// Query execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionStep>,
    pub estimated_cost: f64,
    pub estimated_rows: usize,
}

/// Execution step in the plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStep {
    /// Sequential scan of a table
    SeqScan {
        table: String,
        filters: Vec<Filter>,
        estimated_rows: usize,
        cost: f64,
    },
    /// Index scan
    IndexScan {
        table: String,
        index_name: String,
        filters: Vec<Filter>,
        estimated_rows: usize,
        cost: f64,
    },
    /// Hash join
    HashJoin {
        left_table: String,
        right_table: String,
        join_condition: String,
        estimated_rows: usize,
        cost: f64,
    },
    /// Nested loop join
    NestedLoopJoin {
        left_table: String,
        right_table: String,
        join_condition: String,
        estimated_rows: usize,
        cost: f64,
    },
    /// Sort operation
    Sort {
        table: String,
        sort_field: String,
        estimated_rows: usize,
        cost: f64,
    },
    /// Limit operation
    Limit { limit: usize, offset: usize },
}

/// Query planner
pub struct QueryPlanner {
    table_stats: HashMap<String, TableStatistics>,
    index_stats: HashMap<String, IndexStatistics>,
}

/// Table statistics for cost estimation
#[derive(Debug, Clone)]
pub struct TableStatistics {
    pub row_count: usize,
    pub avg_row_size: usize,
    pub column_stats: HashMap<String, ColumnStatistics>,
}

/// Column statistics
#[derive(Debug, Clone)]
pub struct ColumnStatistics {
    pub distinct_count: usize,
    pub null_count: usize,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
}

/// Index statistics
#[derive(Debug, Clone)]
pub struct IndexStatistics {
    pub index_name: String,
    pub table_name: String,
    pub column_name: String,
    pub distinct_count: usize,
    pub height: usize, // B-tree height
}

impl QueryPlanner {
    /// Create a new query planner
    pub fn new() -> Self {
        Self {
            table_stats: HashMap::new(),
            index_stats: HashMap::new(),
        }
    }

    /// Plan a file query
    pub fn plan_file_query(&self, query: &FileQuery) -> Result<ExecutionPlan> {
        let mut steps = Vec::new();
        let mut total_cost = 0.0;
        let mut estimated_rows = 0;

        // Determine best access method
        let access_step = self.choose_access_method("files", &query.filters)?;
        total_cost += access_step.cost();
        estimated_rows = access_step.estimated_rows();
        steps.push(access_step);

        // Plan JOINs
        for join in &query.joins {
            let join_step = self.plan_join(&join, estimated_rows)?;
            total_cost += join_step.cost();
            estimated_rows = join_step.estimated_rows();
            steps.push(join_step);
        }

        // Plan sorting if needed
        if let Some(sort) = &query.sort_by {
            let sort_step = ExecutionStep::Sort {
                table: "files".to_string(),
                sort_field: sort.field.clone(),
                estimated_rows,
                cost: self.estimate_sort_cost(estimated_rows),
            };
            total_cost += sort_step.cost();
            steps.push(sort_step);
        }

        // Plan limit
        if let Some(limit) = query.limit {
            steps.push(ExecutionStep::Limit {
                limit,
                offset: query.offset.unwrap_or(0),
            });
        }

        info!(
            "Query plan created: {} steps, cost: {:.2}, rows: {}",
            steps.len(),
            total_cost,
            estimated_rows
        );

        Ok(ExecutionPlan {
            steps,
            estimated_cost: total_cost,
            estimated_rows,
        })
    }

    /// Plan a fact query
    pub fn plan_fact_query(&self, query: &FactQuery) -> Result<ExecutionPlan> {
        let mut steps = Vec::new();
        let mut total_cost = 0.0;
        let mut estimated_rows = 0;

        let access_step = self.choose_access_method("facts", &query.filters)?;
        total_cost += access_step.cost();
        estimated_rows = access_step.estimated_rows();
        steps.push(access_step);

        for join in &query.joins {
            let join_step = self.plan_join(&join, estimated_rows)?;
            total_cost += join_step.cost();
            estimated_rows = join_step.estimated_rows();
            steps.push(join_step);
        }

        if let Some(sort) = &query.sort_by {
            let sort_step = ExecutionStep::Sort {
                table: "facts".to_string(),
                sort_field: sort.field.clone(),
                estimated_rows,
                cost: self.estimate_sort_cost(estimated_rows),
            };
            total_cost += sort_step.cost();
            steps.push(sort_step);
        }

        if let Some(limit) = query.limit {
            steps.push(ExecutionStep::Limit {
                limit,
                offset: query.offset.unwrap_or(0),
            });
        }

        Ok(ExecutionPlan {
            steps,
            estimated_cost: total_cost,
            estimated_rows,
        })
    }

    /// Plan a user query
    pub fn plan_user_query(&self, query: &UserQuery) -> Result<ExecutionPlan> {
        let mut steps = Vec::new();
        let mut total_cost = 0.0;
        let mut estimated_rows = 0;

        let access_step = self.choose_access_method("users", &query.filters)?;
        total_cost += access_step.cost();
        estimated_rows = access_step.estimated_rows();
        steps.push(access_step);

        for join in &query.joins {
            let join_step = self.plan_join(&join, estimated_rows)?;
            total_cost += join_step.cost();
            estimated_rows = join_step.estimated_rows();
            steps.push(join_step);
        }

        if let Some(sort) = &query.sort_by {
            let sort_step = ExecutionStep::Sort {
                table: "users".to_string(),
                sort_field: sort.field.clone(),
                estimated_rows,
                cost: self.estimate_sort_cost(estimated_rows),
            };
            total_cost += sort_step.cost();
            steps.push(sort_step);
        }

        if let Some(limit) = query.limit {
            steps.push(ExecutionStep::Limit {
                limit,
                offset: query.offset.unwrap_or(0),
            });
        }

        Ok(ExecutionPlan {
            steps,
            estimated_cost: total_cost,
            estimated_rows,
        })
    }

    /// Choose the best access method (sequential scan vs index scan)
    fn choose_access_method(&self, table: &str, filters: &[Filter]) -> Result<ExecutionStep> {
        let table_stats = self
            .table_stats
            .get(table)
            .cloned()
            .unwrap_or_else(|| TableStatistics {
                row_count: 10000, // Default estimate
                avg_row_size: 1024,
                column_stats: HashMap::new(),
            });

        // Check if we can use an index
        for filter in filters {
            if let Some(index) = self.find_index_for_filter(table, filter) {
                let index_stats = self
                    .index_stats
                    .get(&index.index_name)
                    .cloned()
                    .unwrap_or_else(|| IndexStatistics {
                        index_name: index.index_name.clone(),
                        table_name: table.to_string(),
                        column_name: filter.field.clone(),
                        distinct_count: 1000,
                        height: 3,
                    });

                // Estimate index scan cost
                let index_cost = self.estimate_index_scan_cost(&index_stats, &table_stats, filter);
                let seq_cost = self.estimate_seq_scan_cost(&table_stats, filters);

                if index_cost < seq_cost {
                    return Ok(ExecutionStep::IndexScan {
                        table: table.to_string(),
                        index_name: index.index_name.clone(),
                        filters: filters.to_vec(),
                        estimated_rows: self
                            .estimate_filtered_rows(&table_stats, &[filter.clone()]),
                        cost: index_cost,
                    });
                }
            }
        }

        // Use sequential scan
        Ok(ExecutionStep::SeqScan {
            table: table.to_string(),
            filters: filters.to_vec(),
            estimated_rows: self.estimate_filtered_rows(&table_stats, filters),
            cost: self.estimate_seq_scan_cost(&table_stats, filters),
        })
    }

    /// Plan a JOIN operation
    fn plan_join(&self, join: &Join, left_rows: usize) -> Result<ExecutionStep> {
        let right_stats = self
            .table_stats
            .get(&join.table)
            .cloned()
            .unwrap_or_else(|| TableStatistics {
                row_count: 10000,
                avg_row_size: 1024,
                column_stats: HashMap::new(),
            });

        // Estimate join result size
        let estimated_rows =
            self.estimate_join_rows(left_rows, right_stats.row_count, &join.join_type);

        // Choose join algorithm (hash join vs nested loop)
        let hash_join_cost = self.estimate_hash_join_cost(left_rows, right_stats.row_count);
        let nested_loop_cost = self.estimate_nested_loop_cost(left_rows, right_stats.row_count);

        if hash_join_cost < nested_loop_cost {
            Ok(ExecutionStep::HashJoin {
                left_table: "current".to_string(),
                right_table: join.table.clone(),
                join_condition: format!(
                    "{}.{} = {}.{}",
                    join.condition.left_table,
                    join.condition.left_field,
                    join.condition.right_table,
                    join.condition.right_field
                ),
                estimated_rows,
                cost: hash_join_cost,
            })
        } else {
            Ok(ExecutionStep::NestedLoopJoin {
                left_table: "current".to_string(),
                right_table: join.table.clone(),
                join_condition: format!(
                    "{}.{} = {}.{}",
                    join.condition.left_table,
                    join.condition.left_field,
                    join.condition.right_table,
                    join.condition.right_field
                ),
                estimated_rows,
                cost: nested_loop_cost,
            })
        }
    }

    /// Find index for a filter
    fn find_index_for_filter(&self, table: &str, filter: &Filter) -> Option<&IndexStatistics> {
        // Check if there's an index on the filter field
        self.index_stats
            .values()
            .find(|idx| idx.table_name == table && idx.column_name == filter.field)
    }

    /// Estimate filtered rows
    fn estimate_filtered_rows(&self, stats: &TableStatistics, filters: &[Filter]) -> usize {
        let mut selectivity = 1.0;

        for filter in filters {
            match filter.op {
                FilterOp::Equals => selectivity *= 0.1, // Assume 10% selectivity
                FilterOp::GreaterThan | FilterOp::LessThan => selectivity *= 0.3,
                FilterOp::Contains => selectivity *= 0.2,
                _ => selectivity *= 0.5,
            }
        }

        (stats.row_count as f64 * selectivity) as usize
    }

    /// Estimate sequential scan cost
    fn estimate_seq_scan_cost(&self, stats: &TableStatistics, filters: &[Filter]) -> f64 {
        // Base cost: pages to read
        let pages = (stats.row_count * stats.avg_row_size) as f64 / 8192.0; // 8KB pages
        let cpu_cost = stats.row_count as f64 * 0.01; // CPU cost per row
        pages * 1.0 + cpu_cost
    }

    /// Estimate index scan cost
    fn estimate_index_scan_cost(
        &self,
        index: &IndexStatistics,
        table: &TableStatistics,
        filter: &Filter,
    ) -> f64 {
        // Index lookup cost: tree height + leaf pages
        let index_cost = index.height as f64 * 1.0;
        let table_cost = self.estimate_filtered_rows(table, &[filter.clone()]) as f64 * 0.01;
        index_cost + table_cost
    }

    /// Estimate hash join cost
    fn estimate_hash_join_cost(&self, left_rows: usize, right_rows: usize) -> f64 {
        // Build hash table + probe
        let build_cost = right_rows as f64 * 0.02;
        let probe_cost = left_rows as f64 * 0.01;
        build_cost + probe_cost
    }

    /// Estimate nested loop join cost
    fn estimate_nested_loop_cost(&self, left_rows: usize, right_rows: usize) -> f64 {
        // Nested loop: left_rows * right_rows
        left_rows as f64 * right_rows as f64 * 0.01
    }

    /// Estimate join result rows
    fn estimate_join_rows(
        &self,
        left_rows: usize,
        right_rows: usize,
        join_type: &crate::sql_query::JoinType,
    ) -> usize {
        match join_type {
            crate::sql_query::JoinType::Inner => {
                // Assume 1:1 or 1:many relationship
                left_rows.min(right_rows)
            }
            crate::sql_query::JoinType::Left => left_rows,
            crate::sql_query::JoinType::Right => right_rows,
            crate::sql_query::JoinType::FullOuter => left_rows.max(right_rows),
        }
    }

    /// Estimate sort cost
    fn estimate_sort_cost(&self, rows: usize) -> f64 {
        // O(n log n) sort
        rows as f64 * (rows as f64).log2() * 0.01
    }

    /// Update table statistics
    pub fn update_table_stats(&mut self, table: String, stats: TableStatistics) {
        self.table_stats.insert(table, stats);
    }

    /// Update index statistics
    pub fn update_index_stats(&mut self, index_name: String, stats: IndexStatistics) {
        self.index_stats.insert(index_name, stats);
    }
}

impl ExecutionStep {
    /// Get cost of this step
    pub fn cost(&self) -> f64 {
        match self {
            ExecutionStep::SeqScan { cost, .. } => *cost,
            ExecutionStep::IndexScan { cost, .. } => *cost,
            ExecutionStep::HashJoin { cost, .. } => *cost,
            ExecutionStep::NestedLoopJoin { cost, .. } => *cost,
            ExecutionStep::Sort { cost, .. } => *cost,
            ExecutionStep::Limit { .. } => 0.0,
        }
    }

    /// Get estimated rows for this step
    pub fn estimated_rows(&self) -> usize {
        match self {
            ExecutionStep::SeqScan { estimated_rows, .. } => *estimated_rows,
            ExecutionStep::IndexScan { estimated_rows, .. } => *estimated_rows,
            ExecutionStep::HashJoin { estimated_rows, .. } => *estimated_rows,
            ExecutionStep::NestedLoopJoin { estimated_rows, .. } => *estimated_rows,
            ExecutionStep::Sort { estimated_rows, .. } => *estimated_rows,
            ExecutionStep::Limit { limit, .. } => *limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_query::{FactQuery, FileQuery, Filter, FilterOp, FilterValue};

    #[test]
    fn test_plan_file_query() {
        let planner = QueryPlanner::new();

        let query = FileQuery {
            distinct: false,
            window_functions: Vec::new(),
            filters: vec![Filter {
                field: "owner_did".to_string(),
                op: FilterOp::Equals,
                value: FilterValue::String("did:spacekit:user:alice".to_string()),
            }],
            sort_by: None,
            limit: Some(10),
            offset: None,
            joins: vec![],
        };

        let plan = planner.plan_file_query(&query).unwrap();
        assert!(!plan.steps.is_empty());
        assert!(plan.estimated_cost > 0.0);
    }

    #[test]
    fn test_plan_fact_query() {
        let planner = QueryPlanner::new();

        let query = FactQuery {
            distinct: false,
            window_functions: Vec::new(),
            filters: vec![Filter {
                field: "category".to_string(),
                op: FilterOp::Equals,
                value: FilterValue::String("Scientific".to_string()),
            }],
            sort_by: None,
            limit: Some(20),
            offset: None,
            joins: vec![],
        };

        let plan = planner.plan_fact_query(&query).unwrap();
        assert!(!plan.steps.is_empty());
    }
}
