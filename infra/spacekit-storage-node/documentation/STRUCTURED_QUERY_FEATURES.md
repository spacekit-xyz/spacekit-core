# Structured Query Interface - SQL Feature Support

## Overview

SpaceKit Storage Node provides **SQL-like query capabilities** via a structured query interface (not raw SQL parsing). This document details what SQL Standard features are available through the query builder.

## ✅ Fully Supported SQL Features

### 1. SELECT Queries
**Status**: ✅ **Fully Supported**

All query types support:
- Field selection (implicit - all fields returned)
- Filtering (WHERE clause)
- Sorting (ORDER BY)
- Pagination (LIMIT, OFFSET)

**Example:**
```json
{
  "filters": [
    {
      "field": "owner_did",
      "op": "Equals",
      "value": "did:spacekit:user:alice"
    }
  ],
  "sort_by": {
    "field": "created_at",
    "order": "Desc"
  },
  "limit": 10,
  "offset": 0
}
```

### 2. JOIN Operations
**Status**: ✅ **Fully Supported**

**Supported JOIN Types:**
- ✅ **INNER JOIN** - Returns matching rows from both tables
- ✅ **LEFT JOIN** - Returns all rows from left table + matching rows from right
- ✅ **RIGHT JOIN** - Returns all rows from right table + matching rows from left
- ✅ **FULL OUTER JOIN** - Returns all rows from both tables

**Implementation:**
- JOINs are executed in-memory after loading base data
- Supports joining files ↔ users, files ↔ facts
- JOIN conditions specified via `JoinCondition` struct

**Example:**
```json
{
  "filters": [],
  "joins": [
    {
      "join_type": "Inner",
      "table": "users",
      "condition": {
        "left_table": "files",
        "left_field": "owner_did",
        "right_table": "users",
        "right_field": "address"
      }
    }
  ]
}
```

### 3. Subqueries
**Status**: ✅ **Fully Supported**

**Supported Subquery Types:**
- ✅ **IN subquery** - `field IN (SELECT ...)`
- ✅ **NOT IN subquery** - `field NOT IN (SELECT ...)`
- ✅ **EXISTS subquery** - `EXISTS (SELECT ...)`
- ✅ **NOT EXISTS subquery** - `NOT EXISTS (SELECT ...)`

**Implementation:**
- Subqueries are executed recursively
- Support nested filters within subqueries
- Can query across tables (files, facts, users)

**Example:**
```json
{
  "filters": [
    {
      "field": "owner_did",
      "op": "In",
      "value": {
        "Subquery": {
          "subquery_type": "In",
          "table": "users",
          "field": "address",
          "filters": [
            {
              "field": "network",
              "op": "Equals",
              "value": "spacekit"
            }
          ]
        }
      }
    }
  ]
}
```

### 4. Filtering (WHERE Clause)
**Status**: ✅ **Fully Supported**

**Supported Operators:**
- ✅ `Equals` - `field = value`
- ✅ `NotEquals` - `field != value`
- ✅ `GreaterThan` - `field > value`
- ✅ `LessThan` - `field < value`
- ✅ `GreaterThanOrEqual` - `field >= value`
- ✅ `LessThanOrEqual` - `field <= value`
- ✅ `Contains` - `field LIKE '%value%'`
- ✅ `StartsWith` - `field LIKE 'value%'`
- ✅ `EndsWith` - `field LIKE '%value'`
- ✅ `In` - `field IN (value1, value2, ...)`
- ✅ `NotIn` - `field NOT IN (value1, value2, ...)`

**Supported Value Types:**
- String
- Number (f64)
- Integer (i64)
- Boolean
- Array (for IN operations)
- Subquery (nested queries)

### 5. Sorting (ORDER BY)
**Status**: ✅ **Fully Supported**

**Features:**
- Single field sorting
- Ascending/Descending order
- Applied after filtering, before pagination

**Example:**
```json
{
  "sort_by": {
    "field": "created_at",
    "order": "Desc"
  }
}
```

### 6. Pagination
**Status**: ✅ **Fully Supported**

**Features:**
- `limit` - Maximum number of results
- `offset` - Number of results to skip
- Applied after filtering and sorting

**Example:**
```json
{
  "limit": 20,
  "offset": 40
}
```

### 7. Aggregations
**Status**: ✅ **Fully Supported**

**Supported Functions:**
- ✅ `COUNT` - Count of rows
- ✅ `SUM` - Sum of numeric values
- ✅ `AVG` - Average of numeric values
- ✅ `MIN` - Minimum value
- ✅ `MAX` - Maximum value

**Features:**
- Can be used with `GROUP BY`
- Supports filtering before aggregation

**Example:**
```json
{
  "function": "Count",
  "field": "id",
  "filters": [
    {
      "field": "owner_did",
      "op": "Equals",
      "value": "did:spacekit:user:alice"
    }
  ],
  "group_by": "owner_did"
}
```

### 8. GROUP BY
**Status**: ✅ **Fully Supported**

**Features:**
- Group results by field value
- Works with aggregate functions
- Returns grouped results with aggregate values

**Example:**
```json
{
  "function": "Sum",
  "field": "size",
  "group_by": "owner_did"
}
```

### 9. Window Functions
**Status**: ✅ **Fully Supported**

**Window Function Types:**
- ✅ `RowNumber` - Sequential row number within partition
- ✅ `Rank` - Rank with gaps for ties
- ✅ `DenseRank` - Rank without gaps for ties
- ✅ `Ntile(n)` - Divide rows into n buckets
- ✅ `Lag(field, offset)` - Previous row value
- ✅ `Lead(field, offset)` - Next row value
- ✅ `FirstValue(field)` - First value in partition
- ✅ `LastValue(field)` - Last value in partition
- ✅ `AggregateOver(function, field)` - Aggregate with OVER clause (SUM, AVG, MIN, MAX, COUNT)

**Window Specification:**
- ✅ `PARTITION BY` - Group rows into partitions
- ✅ `ORDER BY` - Sort rows within partition

**Implementation:**
- Window function results returned in `window_results` array
- Each row has corresponding window function values
- Results include alias for easy identification
- Supports multiple window functions per query

**Example:**
```json
{
  "filters": [],
  "window_functions": [
    {
      "function": {
        "RowNumber": null
      },
      "window_spec": {
        "partition_by": ["owner_did"],
        "order_by": {
          "field": "created_at",
          "order": "Asc"
        }
      },
      "alias": "row_num"
    },
    {
      "function": {
        "AggregateOver": {
          "function": "Sum",
          "field": "size"
        }
      },
      "window_spec": {
        "partition_by": ["owner_did"],
        "order_by": {
          "field": "created_at",
          "order": "Asc"
        }
      },
      "alias": "total_size"
    }
  ]
}
```

**Response Format:**
```json
{
  "files": [...],
  "total_count": 10,
  "execution_time_ms": 5,
  "window_results": [
    [
      {"alias": "row_num", "value": {"Integer": 1}},
      {"alias": "total_size", "value": {"Float": 1024.0}}
    ],
    [
      {"alias": "row_num", "value": {"Integer": 2}},
      {"alias": "total_size", "value": {"Float": 2048.0}}
    ]
  ]
}
```

### 10. DISTINCT
**Status**: ✅ **Fully Supported**

**Features:**
- Remove duplicate rows from query results
- Applied after filtering, before sorting
- Works with all query types (files, facts, users)
- Uses field-based deduplication

**Example:**
```json
{
  "filters": [],
  "distinct": true,
  "sort_by": {
    "field": "created_at",
    "order": "Desc"
  }
}
```

**Implementation:**
- Creates unique key from all fields (id, owner_did, filename, hash for files)
- Preserves first occurrence of duplicate rows
- Efficient HashSet-based deduplication

### 11. HAVING Clause
**Status**: ✅ **Fully Supported**

**Features:**
- Filter groups after aggregation
- Works with GROUP BY
- Supports all filter operators
- Applied after GROUP BY aggregation

**Example:**
```json
{
  "function": "Sum",
  "field": "size",
  "filters": [
    {
      "field": "created_at",
      "op": "GreaterThan",
      "value": "2025-01-01T00:00:00Z"
    }
  ],
  "group_by": "owner_did",
  "having": [
    {
      "field": "aggregate_value",
      "op": "GreaterThan",
      "value": 1000
    }
  ]
}
```

**Implementation:**
- Filters groups based on aggregate values
- Can also filter on group key
- Returns only groups that pass HAVING conditions

### 12. UNION Operations
**Status**: ✅ **Fully Supported**

**Features:**
- Combine multiple file queries
- UNION - removes duplicates
- UNION ALL - keeps all rows (including duplicates)
- Supports multiple queries in single operation

**Example:**
```json
{
  "queries": [
    {
      "filters": [
        {
          "field": "owner_did",
          "op": "Equals",
          "value": "did:spacekit:user:alice"
        }
      ]
    },
    {
      "filters": [
        {
          "field": "owner_did",
          "op": "Equals",
          "value": "did:spacekit:user:bob"
        }
      ]
    }
  ],
  "union_type": "Union"
}
```

**Implementation:**
- Executes each query independently
- Combines results in order
- Applies DISTINCT for UNION (not UNION ALL)
- Preserves window function results when applicable

## 📊 SQL Standard Compliance

### SQL-92 Core Features

| Feature | SQL-92 Standard | SpaceKit Support | Notes |
|---------|----------------|-----------------|-------|
| **SELECT** | ✅ Core | ✅ Supported | Via query builder |
| **WHERE** | ✅ Core | ✅ Supported | Via filters |
| **JOIN** | ✅ Core | ✅ Supported | Inner, Left, Right, Full |
| **Subqueries** | ✅ Core | ✅ Supported | IN, NOT IN, EXISTS |
| **ORDER BY** | ✅ Core | ✅ Supported | Single field |
| **GROUP BY** | ✅ Core | ✅ Supported | With aggregations |
| **HAVING** | ✅ Core | ✅ **Fully Supported** | Filter groups after aggregation |
| **UNION** | ✅ Core | ✅ **Fully Supported** | Combine query results (UNION & UNION ALL) |
| **DISTINCT** | ✅ Core | ✅ **Fully Supported** | Remove duplicate rows |
| **Window Functions** | ✅ SQL:1999 | ✅ **Fully Supported** | All window function types implemented |
| **CTEs (WITH)** | ✅ SQL:1999 | ❌ Not yet | Common table expressions |

### Advanced SQL Features

| Feature | SQL Standard | SpaceKit Support | Notes |
|---------|--------------|-----------------|-------|
| **Multiple ORDER BY** | ✅ Standard | ❌ Not yet | Only single field |
| **CASE expressions** | ✅ Standard | ❌ Not yet | Conditional logic |
| **NULL handling** | ✅ Standard | ⚠️ Partial | Basic NULL checks |
| **Date functions** | ✅ Standard | ❌ Not yet | Date arithmetic |
| **String functions** | ✅ Standard | ⚠️ Partial | Contains, StartsWith, EndsWith |
| **Numeric functions** | ✅ Standard | ✅ Supported | Via aggregations |

## Implementation Details

### Query Execution Flow

```
1. Load base data from database
   ↓
2. Apply JOINs (if any)
   ↓
3. Apply filters (including subqueries)
   ↓
4. Apply DISTINCT (if requested)
   ↓
5. Apply sorting
   ↓
6. Apply window functions (if any)
   ↓
7. Apply pagination
   ↓
8. Return results (with window_results if applicable)
```

**For Aggregations with GROUP BY:**
```
1. Load base data from database
   ↓
2. Apply filters (WHERE clause)
   ↓
3. Apply GROUP BY aggregation
   ↓
4. Apply HAVING filters (if any)
   ↓
5. Return aggregated results
```

### Performance Characteristics

- **In-Memory Processing**: All queries execute in-memory for speed
- **JOIN Performance**: O(n*m) for JOINs (can be optimized with indexes)
- **Subquery Performance**: Each subquery executes independently
- **Filtering**: O(n) linear scan (can be optimized with indexes)

### Limitations

1. **No Raw SQL**: Must use structured JSON queries
2. **Single ORDER BY Field**: Cannot sort by multiple fields
3. **No CTEs (WITH clause)**: Cannot use common table expressions
4. **No Multiple ORDER BY in Window Functions**: Window ORDER BY is single field

## Usage Examples

### Complex Query with JOIN and Subquery

```json
{
  "filters": [
    {
      "field": "size",
      "op": "GreaterThan",
      "value": 1024
    },
    {
      "field": "owner_did",
      "op": "In",
      "value": {
        "Subquery": {
          "subquery_type": "In",
          "table": "users",
          "field": "address",
          "filters": [
            {
              "field": "network",
              "op": "Equals",
              "value": "spacekit"
            }
          ]
        }
      }
    }
  ],
  "joins": [
    {
      "join_type": "Left",
      "table": "users",
      "condition": {
        "left_table": "files",
        "left_field": "owner_did",
        "right_table": "users",
        "right_field": "address"
      }
    }
  ],
  "sort_by": {
    "field": "created_at",
    "order": "Desc"
  },
  "limit": 50,
  "offset": 0
}
```

### Aggregation with GROUP BY

```json
{
  "function": "Sum",
  "field": "size",
  "filters": [
    {
      "field": "created_at",
      "op": "GreaterThan",
      "value": "2025-01-01T00:00:00Z"
    }
  ],
  "group_by": "owner_did"
}
```

## Roadmap

### Completed Features ✅

1. **Window Functions** ✅
   - All window function types implemented
   - Result storage in query responses
   - PARTITION BY and ORDER BY support

2. **UNION Support** ✅
   - Combine multiple query results
   - UNION and UNION ALL implemented

3. **DISTINCT Support** ✅
   - Remove duplicate rows
   - Field-based deduplication

4. **HAVING Clause** ✅
   - Filter groups after aggregation
   - Post-aggregation filtering

### Planned Enhancements

1. **Multiple ORDER BY Fields**
   - Support sorting by multiple fields
   - Priority-based sorting

2. **CTEs (Common Table Expressions)**
   - WITH clause support
   - Named subqueries

3. **Enhanced Window Functions**
   - Multiple ORDER BY fields in window specs
   - Window frame specifications (ROWS/RANGE)
   - Performance optimizations for large datasets

## Summary

**SpaceKit Storage Node provides comprehensive SQL-like query capabilities** through its structured query interface:

✅ **Fully Supported:**
- SELECT queries
- WHERE filtering (all operators)
- JOINs (all types)
- Subqueries (all types)
- ORDER BY
- GROUP BY
- Aggregations
- Pagination
- **Window Functions** (all types: ROW_NUMBER, RANK, DENSE_RANK, NTILE, LAG, LEAD, FIRST_VALUE, LAST_VALUE, AggregateOver)
- **DISTINCT** (remove duplicate rows)
- **HAVING** (filter groups after aggregation)
- **UNION** (combine queries, with UNION and UNION ALL)

❌ **Not Yet Supported:**
- Multiple ORDER BY fields
- CTEs (WITH clause)
- Raw SQL parsing

The structured query interface provides **most SQL-92 core features** while maintaining type safety and security through the query builder pattern.

