# SQL Query API Documentation

## Overview

The SpaceKit Storage Node provides a **SQL-like query interface** via HTTP API endpoints. While not a full SQL parser, it offers structured query capabilities with filters, sorting, pagination, and aggregate functions.

## API Endpoints

### 1. Query Files

**Endpoint:** `POST /query/files`

**Description:** Query file metadata with filters, sorting, and pagination.

**Request Body:**
```json
{
  "filters": [
    {
      "field": "owner_did",
      "op": "Equals",
      "value": "did:spacekit:user:alice"
    },
    {
      "field": "size",
      "op": "GreaterThan",
      "value": 1024
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

**Response:**
```json
{
  "files": [
    {
      "id": "uuid",
      "filename": "document.pdf",
      "size": 12345,
      "hash": "...",
      "owner_did": "did:spacekit:user:alice",
      "encryption_algorithm": "Kyber1024",
      "content_type": "application/pdf",
      "created_at": "2025-01-15T10:30:00Z",
      "last_accessed": null,
      "encryption_public_key": "...",
      "sharing_mode": "owner"
    }
  ],
  "total_count": 25,
  "execution_time_ms": 5,
  "window_results": []
}
```

**Note:** `window_results` is an array of window function results (one per file). Empty if no window functions are used.

### 2. Query Facts

**Endpoint:** `POST /query/facts`

**Description:** Query fact package metadata with filters, sorting, and pagination.

**Request Body:**
```json
{
  "filters": [
    {
      "field": "author",
      "op": "Equals",
      "value": "did:spacekit:user:bob"
    },
    {
      "field": "category",
      "op": "In",
      "value": ["science", "technology"]
    },
    {
      "field": "confidence_score",
      "op": "GreaterThanOrEqual",
      "value": 0.8
    }
  ],
  "sort_by": {
    "field": "created_at",
    "order": "Desc"
  },
  "limit": 20,
  "offset": 0
}
```

**Response:**
```json
{
  "facts": [
    {
      "fact_id": "uuid",
      "version": 1,
      "author": "did:spacekit:user:bob",
      "created_at": "2025-01-15T10:30:00Z",
      "content_size": 1024,
      "content_type": "application/json",
      "category": "science",
      "domain": "physics",
      "verification_level": "verified",
      "confidence_score": 0.95,
      "storage_tier": "hot",
      "compressed": true,
      "encrypted": true,
      "checksum": "...",
      "tags": ["quantum", "physics"]
    }
  ],
  "total_count": 150,
  "execution_time_ms": 12,
  "window_results": []
}
```

**Note:** `window_results` is an array of window function results (one per fact). Empty if no window functions are used.

### 3. Query Users

**Endpoint:** `POST /query/users`

**Description:** Query user metadata with filters, sorting, and pagination.

**Request Body:**
```json
{
  "filters": [
    {
      "field": "email",
      "op": "Contains",
      "value": "@example.com"
    },
    {
      "field": "network",
      "op": "Equals",
      "value": "ethereum"
    }
  ],
  "sort_by": {
    "field": "username",
    "order": "Asc"
  },
  "limit": 50,
  "offset": 0
}
```

**Response:**
```json
{
  "users": [
    {
      "username": "alice",
      "email": "alice@example.com",
      "address": "0x...",
      "network": "ethereum",
      "message": "...",
      "first_name": "Alice",
      "last_name": "Smith",
      "created_at": "2025-01-15T10:30:00Z"
    }
  ],
  "total_count": 200,
  "execution_time_ms": 8,
  "window_results": []
}
```

**Note:** `window_results` is an array of window function results (one per user). Empty if no window functions are used.

### 4. Aggregate Queries

**Endpoint:** `POST /query/aggregate`

**Description:** Perform aggregate functions (Count, Sum, Avg, Min, Max) on facts.

**Request Body:**
```json
{
  "function": "Avg",
  "field": "confidence_score",
  "filters": [
    {
      "field": "category",
      "op": "Equals",
      "value": "science"
    }
  ],
  "group_by": "domain"
}
```

**Response:**
```json
{
  "value": 0.87,
  "groups": {
    "physics": 0.92,
    "chemistry": 0.85,
    "biology": 0.83
  }
}
```

## Filter Operations

### Available Operations

| Operation | Description | Value Type |
|-----------|-------------|------------|
| `Equals` | Exact match | String, Number, Integer, Boolean |
| `NotEquals` | Not equal | String, Number, Integer, Boolean |
| `GreaterThan` | Greater than | Number, Integer |
| `LessThan` | Less than | Number, Integer |
| `GreaterThanOrEqual` | Greater than or equal | Number, Integer |
| `LessThanOrEqual` | Less than or equal | Number, Integer |
| `Contains` | String contains | String |
| `StartsWith` | String starts with | String |
| `EndsWith` | String ends with | String |
| `In` | Value in array | Array |
| `NotIn` | Value not in array | Array |

### Filter Value Types

```json
{
  "field": "size",
  "op": "GreaterThan",
  "value": 1024  // Integer
}
```

```json
{
  "field": "filename",
  "op": "Contains",
  "value": "document"  // String
}
```

```json
{
  "field": "category",
  "op": "In",
  "value": ["science", "technology", "math"]  // Array
}
```

## Sort Order

```json
{
  "field": "created_at",
  "order": "Desc"  // or "Asc"
}
```

## Pagination

```json
{
  "limit": 10,   // Maximum results (optional, default: unlimited)
  "offset": 0    // Skip N results (optional, default: 0)
}
```

## Aggregate Functions

| Function | Description | Field Type |
|----------|-------------|------------|
| `Count` | Count records | Any |
| `Sum` | Sum of values | Number, Integer |
| `Avg` | Average of values | Number, Integer |
| `Min` | Minimum value | Number, Integer |
| `Max` | Maximum value | Number, Integer |

## Example Queries

### Find Large Files by Owner

```bash
curl -X POST http://localhost:3030/query/files \
  -H "Content-Type: application/json" \
  -d '{
    "filters": [
      {
        "field": "owner_did",
        "op": "Equals",
        "value": "did:spacekit:user:alice"
      },
      {
        "field": "size",
        "op": "GreaterThan",
        "value": 1048576
      }
    ],
    "sort_by": {
      "field": "size",
      "order": "Desc"
    },
    "limit": 10
  }'
```

### Find High-Confidence Facts in Science Category

```bash
curl -X POST http://localhost:3030/query/facts \
  -H "Content-Type: application/json" \
  -d '{
    "filters": [
      {
        "field": "category",
        "op": "Equals",
        "value": "science"
      },
      {
        "field": "confidence_score",
        "op": "GreaterThanOrEqual",
        "value": 0.9
      }
    ],
    "sort_by": {
      "field": "confidence_score",
      "order": "Desc"
    },
    "limit": 20
  }'
```

### Count Files by Owner

```bash
curl -X POST http://localhost:3030/query/aggregate \
  -H "Content-Type: application/json" \
  -d '{
    "function": "Count",
    "field": "id",
    "filters": [
      {
        "field": "owner_did",
        "op": "Equals",
        "value": "did:spacekit:user:alice"
      }
    ]
  }'
```

### Average Confidence Score by Domain

```bash
curl -X POST http://localhost:3030/query/aggregate \
  -H "Content-Type: application/json" \
  -d '{
    "function": "Avg",
    "field": "confidence_score",
    "filters": [
      {
        "field": "category",
        "op": "Equals",
        "value": "science"
      }
    ],
    "group_by": "domain"
  }'
```

### Average Confidence Score by Domain with HAVING

```bash
curl -X POST http://localhost:3030/query/aggregate \
  -H "Content-Type: application/json" \
  -d '{
    "function": "Avg",
    "field": "confidence_score",
    "filters": [
      {
        "field": "category",
        "op": "Equals",
        "value": "science"
      }
    ],
    "group_by": "domain",
    "having": [
      {
        "field": "aggregate_value",
        "op": "GreaterThan",
        "value": 0.85
      }
    ]
  }'
```

### Query Files with JOIN

```bash
curl -X POST http://localhost:3030/query/files \
  -H "Content-Type: application/json" \
  -d '{
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
    ],
    "limit": 10
  }'
```

### Query Files with Window Functions

```bash
curl -X POST http://localhost:3030/query/files \
  -H "Content-Type: application/json" \
  -d '{
    "filters": [
      {
        "field": "owner_did",
        "op": "Equals",
        "value": "did:spacekit:user:alice"
      }
    ],
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
      }
    ],
    "sort_by": {
      "field": "created_at",
      "order": "Desc"
    },
    "limit": 10
  }'
```

### Query Files with DISTINCT

```bash
curl -X POST http://localhost:3030/query/files \
  -H "Content-Type: application/json" \
  -d '{
    "filters": [],
    "distinct": true,
    "sort_by": {
      "field": "created_at",
      "order": "Desc"
    },
    "limit": 20
  }'
```

### UNION Query

```bash
curl -X POST http://localhost:3030/query/union \
  -H "Content-Type: application/json" \
  -d '{
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
  }'
```

## Error Responses

### 400 Bad Request
```json
{
  "error": "Query failed: Invalid filter operation 'InvalidOp'"
}
```

### 500 Internal Server Error
```json
{
  "error": "Query failed: Database error"
}
```

### 503 Service Unavailable
```json
{
  "error": "Query interface not available"
}
```

## Performance Considerations

- **In-Memory Queries**: All queries run against in-memory data structures for maximum performance
- **Filtering**: Applied in-memory after loading all records (for small to medium datasets)
- **JOINs**: Executed in-memory with O(n*m) complexity (can be optimized with indexes)
- **Subqueries**: Each subquery executes independently
- **Window Functions**: Computed per partition with efficient in-memory algorithms
- **DISTINCT**: Uses HashSet for O(1) duplicate detection
- **Sorting**: In-memory sorting using Rust's efficient sort algorithms
- **Pagination**: Applied after filtering and sorting
- **Execution Time**: Included in response for performance monitoring

## Advanced Query Features

### JOIN Operations

**Status**: ✅ **Fully Supported**

Join files with users or facts using INNER, LEFT, RIGHT, or FULL OUTER joins.

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
  ],
  "limit": 10
}
```

**Supported Join Types:**
- `Inner` - Returns matching rows from both tables
- `Left` - Returns all rows from left table + matching rows from right
- `Right` - Returns all rows from right table + matching rows from left
- `FullOuter` - Returns all rows from both tables

### Subqueries

**Status**: ✅ **Fully Supported**

Use subqueries in filters for complex conditions.

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

**Supported Subquery Types:**
- `In` - `field IN (SELECT ...)`
- `NotIn` - `field NOT IN (SELECT ...)`
- `Exists` - `EXISTS (SELECT ...)`
- `NotExists` - `NOT EXISTS (SELECT ...)`

### Window Functions

**Status**: ✅ **Fully Supported**

Perform analytical queries with window functions.

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
  ],
  "limit": 10
}
```

**Response includes window results:**
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

**Supported Window Functions:**
- `RowNumber` - Sequential row number within partition
- `Rank` - Rank with gaps for ties
- `DenseRank` - Rank without gaps for ties
- `Ntile(n)` - Divide rows into n buckets
- `Lag(field, offset)` - Previous row value
- `Lead(field, offset)` - Next row value
- `FirstValue(field)` - First value in partition
- `LastValue(field)` - Last value in partition
- `AggregateOver(function, field)` - Aggregate with OVER clause (Sum, Avg, Min, Max, Count)

### DISTINCT

**Status**: ✅ **Fully Supported**

Remove duplicate rows from query results.

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

### HAVING Clause

**Status**: ✅ **Fully Supported**

Filter groups after aggregation.

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

### UNION Operations

**Status**: ✅ **Fully Supported**

Combine multiple file queries.

**Endpoint:** `POST /query/union`

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

**Union Types:**
- `Union` - Removes duplicates
- `UnionAll` - Keeps all rows (including duplicates)

## Limitations

### Current Limitations
- ⚠️ **In-Memory Only**: All data must fit in memory
- ⚠️ **Single ORDER BY Field**: Cannot sort by multiple fields
- ⚠️ **No CTEs**: Cannot use common table expressions (WITH clause)
- ⚠️ **No Raw SQL**: Must use structured JSON queries (not a SQL parser)

### Future Enhancements
- ✅ **SQL Parser**: Support for standard SQL syntax (planned)
- ✅ **Multiple ORDER BY Fields**: Support sorting by multiple fields
- ✅ **CTEs (WITH clause)**: Common table expressions support

## Comparison with SQL

### SQL Equivalent
```sql
SELECT * FROM files 
WHERE owner_did = 'did:swtch:user:alice' 
  AND size > 1024
ORDER BY created_at DESC
LIMIT 10;
```

### SpaceKit Query API
```json
{
  "filters": [
    {"field": "owner_did", "op": "Equals", "value": "did:swtch:user:alice"},
    {"field": "size", "op": "GreaterThan", "value": 1024}
  ],
  "sort_by": {"field": "created_at", "order": "Desc"},
  "limit": 10
}
```

## Best Practices

1. ✅ **Use Filters**: Always filter before sorting/pagination
2. ✅ **Limit Results**: Always set a reasonable limit
3. ✅ **Use DISTINCT Sparingly**: DISTINCT adds overhead, only use when needed
4. ✅ **Optimize JOINs**: JOINs can be expensive - filter before joining when possible
5. ✅ **Window Functions**: Use PARTITION BY to limit window function computation scope
6. ✅ **HAVING vs WHERE**: Use WHERE for pre-aggregation filtering, HAVING for post-aggregation
7. ✅ **Monitor Performance**: Check `execution_time_ms` in responses
8. ✅ **Cache Results**: Cache frequently accessed queries
9. ✅ **Batch Queries**: Combine multiple filters in one query
10. ✅ **Subquery Performance**: Keep subqueries simple and well-filtered

## Integration Examples

### JavaScript/TypeScript
```typescript
async function queryFiles(ownerDid: string, minSize: number) {
  const response = await fetch('http://localhost:3030/query/files', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      filters: [
        { field: 'owner_did', op: 'Equals', value: ownerDid },
        { field: 'size', op: 'GreaterThan', value: minSize }
      ],
      sort_by: { field: 'created_at', order: 'Desc' },
      limit: 10
    })
  });
  
  const result = await response.json();
  return result.files;
}
```

### Python
```python
import requests

def query_files(owner_did, min_size):
    response = requests.post(
        'http://localhost:3030/query/files',
        json={
            'filters': [
                {'field': 'owner_did', 'op': 'Equals', 'value': owner_did},
                {'field': 'size', 'op': 'GreaterThan', 'value': min_size}
            ],
            'sort_by': {'field': 'created_at', 'order': 'Desc'},
            'limit': 10
        }
    )
    return response.json()['files']
```

### Rust
```rust
use serde_json::json;

let query = json!({
    "filters": [
        {"field": "owner_did", "op": "Equals", "value": "did:swtch:user:alice"},
        {"field": "size", "op": "GreaterThan", "value": 1024}
    ],
    "sort_by": {"field": "created_at", "order": "Desc"},
    "limit": 10
});

let client = reqwest::Client::new();
let response = client
    .post("http://localhost:3030/query/files")
    .json(&query)
    .send()
    .await?;

let result: FileQueryResult = response.json().await?;
```

## References

- **[Structured Query Features](../STRUCTURED_QUERY_FEATURES.md)** - Complete feature list and examples
- **[SQL Support Clarification](../SQL_SUPPORT_CLARIFICATION.md)** - What SQL features are actually supported
- **[PostgreSQL Comparison](../guides/postgresql-comparison.md)** - Comparison with PostgreSQL
- **[README.md](../../README.md)** - General storage node documentation

