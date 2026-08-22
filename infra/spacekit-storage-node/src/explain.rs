//! EXPLAIN/ANALYZE for Query Execution Plans
//!
//! Provides query execution plan display, performance metrics,
//! and index usage statistics.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::info;

use crate::query_planner::{ExecutionPlan, ExecutionStep};
use crate::sql_query::{FactQuery, FileQuery, UserQuery};

/// EXPLAIN result with execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainResult {
    pub plan: ExecutionPlan,
    pub formatted_plan: String,
    pub estimated_cost: f64,
    pub estimated_rows: usize,
}

/// ANALYZE result with actual execution metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeResult {
    pub plan: ExecutionPlan,
    pub actual_execution_time_ms: u64,
    pub actual_rows_returned: usize,
    pub index_usage: Vec<IndexUsage>,
    pub warnings: Vec<String>,
}

/// Index usage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexUsage {
    pub index_name: String,
    pub table: String,
    pub column: String,
    pub scans: usize,
    pub rows_examined: usize,
    pub rows_returned: usize,
}

/// EXPLAIN/ANALYZE executor
pub struct ExplainAnalyzer {
    planner: crate::query_planner::QueryPlanner,
}

impl ExplainAnalyzer {
    /// Create a new EXPLAIN analyzer
    pub fn new(planner: crate::query_planner::QueryPlanner) -> Self {
        Self { planner }
    }

    /// EXPLAIN a file query
    pub fn explain_file_query(&self, query: &FileQuery) -> Result<ExplainResult> {
        let plan = self.planner.plan_file_query(query)?;
        let formatted = self.format_plan(&plan);

        Ok(ExplainResult {
            plan: plan.clone(),
            formatted_plan: formatted,
            estimated_cost: plan.estimated_cost,
            estimated_rows: plan.estimated_rows,
        })
    }

    /// EXPLAIN a fact query
    pub fn explain_fact_query(&self, query: &FactQuery) -> Result<ExplainResult> {
        let plan = self.planner.plan_fact_query(query)?;
        let formatted = self.format_plan(&plan);

        Ok(ExplainResult {
            plan: plan.clone(),
            formatted_plan: formatted,
            estimated_cost: plan.estimated_cost,
            estimated_rows: plan.estimated_rows,
        })
    }

    /// EXPLAIN a user query
    pub fn explain_user_query(&self, query: &UserQuery) -> Result<ExplainResult> {
        let plan = self.planner.plan_user_query(query)?;
        let formatted = self.format_plan(&plan);

        Ok(ExplainResult {
            plan: plan.clone(),
            formatted_plan: formatted,
            estimated_cost: plan.estimated_cost,
            estimated_rows: plan.estimated_rows,
        })
    }

    /// ANALYZE a file query (execute and collect metrics)
    pub async fn analyze_file_query(
        &self,
        query: &FileQuery,
        query_builder: &crate::sql_query::StorageQueryBuilder,
    ) -> Result<AnalyzeResult> {
        let plan = self.planner.plan_file_query(query)?;
        let start = Instant::now();

        let result = query_builder.query_files(query.clone()).await?;
        let execution_time = start.elapsed().as_millis() as u64;

        let index_usage = self.collect_index_usage(&plan);
        let warnings = self.collect_warnings(&plan, execution_time);

        Ok(AnalyzeResult {
            plan,
            actual_execution_time_ms: execution_time,
            actual_rows_returned: result.total_count,
            index_usage,
            warnings,
        })
    }

    /// ANALYZE a fact query
    pub async fn analyze_fact_query(
        &self,
        query: &FactQuery,
        query_builder: &crate::sql_query::StorageQueryBuilder,
    ) -> Result<AnalyzeResult> {
        let plan = self.planner.plan_fact_query(query)?;
        let start = Instant::now();

        let result = query_builder.query_facts(query.clone()).await?;
        let execution_time = start.elapsed().as_millis() as u64;

        let index_usage = self.collect_index_usage(&plan);
        let warnings = self.collect_warnings(&plan, execution_time);

        Ok(AnalyzeResult {
            plan,
            actual_execution_time_ms: execution_time,
            actual_rows_returned: result.total_count,
            index_usage,
            warnings,
        })
    }

    /// ANALYZE a user query
    pub async fn analyze_user_query(
        &self,
        query: &UserQuery,
        query_builder: &crate::sql_query::StorageQueryBuilder,
    ) -> Result<AnalyzeResult> {
        let plan = self.planner.plan_user_query(query)?;
        let start = Instant::now();

        let result = query_builder.query_users(query.clone()).await?;
        let execution_time = start.elapsed().as_millis() as u64;

        let index_usage = self.collect_index_usage(&plan);
        let warnings = self.collect_warnings(&plan, execution_time);

        Ok(AnalyzeResult {
            plan,
            actual_execution_time_ms: execution_time,
            actual_rows_returned: result.total_count,
            index_usage,
            warnings,
        })
    }

    /// Format execution plan as human-readable string
    fn format_plan(&self, plan: &ExecutionPlan) -> String {
        let mut output = format!(
            "Query Plan (cost={:.2}..{:.2} rows={})\n",
            plan.estimated_cost, plan.estimated_cost, plan.estimated_rows
        );

        for (i, step) in plan.steps.iter().enumerate() {
            let indent = "  ".repeat(i);
            output.push_str(&format!("{}{}\n", indent, self.format_step(step)));
        }

        output
    }

    /// Format a single execution step
    fn format_step(&self, step: &ExecutionStep) -> String {
        match step {
            ExecutionStep::SeqScan {
                table,
                filters,
                estimated_rows,
                cost,
            } => {
                format!(
                    "Seq Scan on {} (cost={:.2}..{:.2} rows={} filters={:?})",
                    table,
                    cost,
                    cost,
                    estimated_rows,
                    filters.len()
                )
            }
            ExecutionStep::IndexScan {
                table,
                index_name,
                filters,
                estimated_rows,
                cost,
            } => {
                format!(
                    "Index Scan using {} on {} (cost={:.2}..{:.2} rows={} filters={:?})",
                    index_name,
                    table,
                    cost,
                    cost,
                    estimated_rows,
                    filters.len()
                )
            }
            ExecutionStep::HashJoin {
                left_table,
                right_table,
                join_condition,
                estimated_rows,
                cost,
            } => {
                format!(
                    "Hash Join (cost={:.2}..{:.2} rows={} condition={})",
                    cost, cost, estimated_rows, join_condition
                )
            }
            ExecutionStep::NestedLoopJoin {
                left_table,
                right_table,
                join_condition,
                estimated_rows,
                cost,
            } => {
                format!(
                    "Nested Loop Join (cost={:.2}..{:.2} rows={} condition={})",
                    cost, cost, estimated_rows, join_condition
                )
            }
            ExecutionStep::Sort {
                table,
                sort_field,
                estimated_rows,
                cost,
            } => {
                format!(
                    "Sort on {} by {} (cost={:.2}..{:.2} rows={})",
                    table, sort_field, cost, cost, estimated_rows
                )
            }
            ExecutionStep::Limit { limit, offset } => {
                format!("Limit (rows={} offset={})", limit, offset)
            }
        }
    }

    /// Collect index usage statistics from plan
    fn collect_index_usage(&self, plan: &ExecutionPlan) -> Vec<IndexUsage> {
        let mut usage = Vec::new();

        for step in &plan.steps {
            if let ExecutionStep::IndexScan {
                table,
                index_name,
                estimated_rows,
                ..
            } = step
            {
                usage.push(IndexUsage {
                    index_name: index_name.clone(),
                    table: table.clone(),
                    column: "unknown".to_string(), // Would need to track this
                    scans: 1,
                    rows_examined: *estimated_rows,
                    rows_returned: *estimated_rows,
                });
            }
        }

        usage
    }

    /// Collect warnings about query performance
    fn collect_warnings(&self, plan: &ExecutionPlan, execution_time_ms: u64) -> Vec<String> {
        let mut warnings = Vec::new();

        // Warn about high cost
        if plan.estimated_cost > 1000.0 {
            warnings.push(format!("High estimated cost: {:.2}", plan.estimated_cost));
        }

        // Warn about slow execution
        if execution_time_ms > 1000 {
            warnings.push(format!("Slow query execution: {}ms", execution_time_ms));
        }

        // Warn about sequential scans on large tables
        for step in &plan.steps {
            if let ExecutionStep::SeqScan { estimated_rows, .. } = step {
                if *estimated_rows > 10000 {
                    warnings.push(format!(
                        "Sequential scan on large table: {} rows",
                        estimated_rows
                    ));
                }
            }
        }

        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query_planner::QueryPlanner;
    use crate::sql_query::{FileQuery, Filter, FilterOp, FilterValue};

    #[test]
    fn test_explain_file_query() {
        let planner = QueryPlanner::new();
        let analyzer = ExplainAnalyzer::new(planner);

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

        let result = analyzer.explain_file_query(&query).unwrap();
        assert!(!result.formatted_plan.is_empty());
        assert!(result.estimated_cost > 0.0);
    }
}
