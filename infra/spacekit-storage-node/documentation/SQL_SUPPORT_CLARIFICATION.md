# SQL Support Clarification

## ⚠️ Important: SQL-92 vs SQL-Like Query Interface

**SpaceKit Storage Node does NOT support raw SQL parsing or SQL-92 DDL statements.**

The storage node provides a **SQL-like query interface** using structured query objects, not a full SQL database.

## What SpaceKit Storage Node Actually Supports

### ✅ SQL-Like Query Features (Structured Query Builder)

| Feature | Status | Implementation |
|---------|--------|----------------|
| **SELECT queries** | ✅ Supported | Via structured query objects |
| **Filtering (WHERE)** | ✅ Supported | `Filter` objects with operators |
| **JOINs** | ✅ **Fully Supported** | `JoinType` enum (Inner, Left, Right, FullOuter) |
| **Subqueries** | ✅ **Fully Supported** | `Subquery` objects (IN, NOT IN, EXISTS, NOT EXISTS) |
| **Sorting (ORDER BY)** | ✅ Supported | `SortBy` objects |
| **Pagination** | ✅ Supported | `limit` and `offset` parameters |
| **Aggregations** | ✅ Supported | COUNT, SUM, AVG, MIN, MAX |
| **GROUP BY** | ✅ Supported | Grouping with aggregations |
| **Window Functions** | ✅ **Fully Supported** | All types: ROW_NUMBER, RANK, DENSE_RANK, NTILE, LAG, LEAD, FIRST_VALUE, LAST_VALUE, AggregateOver |
| **DISTINCT** | ✅ **Fully Supported** | Remove duplicate rows |
| **HAVING** | ✅ **Fully Supported** | Filter groups after aggregation |
| **UNION** | ✅ **Fully Supported** | Combine queries (UNION & UNION ALL) |

### ❌ NOT Supported (SQL-92 Features)

| Feature | Status | Reason |
|---------|--------|--------|
| **Raw SQL parsing** | ❌ Not supported | No SQL parser - uses structured queries |
| **CREATE TABLE / DDL** | ❌ Not supported | Schema is fixed (files, facts, users) |
| **ALTER TABLE** | ❌ Not supported | Schema changes via migrations only |
| **DROP TABLE** | ❌ Not supported | Tables are system-defined |
| **PostgreSQL ENUMs** | ❌ Not supported | Not a PostgreSQL database |
| **PostgreSQL Extensions** | ❌ Not supported | No extension system |
| **FOREIGN KEY constraints** | ❌ Not supported | No constraint system |
| **CHECK constraints** | ❌ Not supported | No constraint validation |
| **Triggers** | ❌ Not supported | No trigger system |
| **Stored procedures** | ❌ Not supported | No procedure language |
| **JSONB/INET types** | ❌ Not supported | Basic types only |
| **User-defined schemas** | ❌ Not supported | Fixed schema structure |

## How It Works

### Query Interface Pattern

Instead of raw SQL like:
```sql
SELECT * FROM files WHERE owner_did = 'did:spacekit:user:alice' ORDER BY created_at DESC LIMIT 10;
```

SpaceKit uses structured JSON queries:
```json
{
  "table": "files",
  "filters": [
    {
      "field": "owner_did",
      "op": "Equals",
      "value": "did:spacekit:user:alice"
    }
  ],
  "sort_by": [
    {
      "field": "created_at",
      "order": "Desc"
    }
  ],
  "limit": 10
}
```

### Why This Design?

1. **Security**: Structured queries prevent SQL injection attacks
2. **Type Safety**: Query validation at compile time (Rust) and runtime
3. **API-First**: Designed for REST APIs, not SQL clients
4. **Simplified**: No need for complex SQL parser
5. **Document-Oriented**: Optimized for document storage, not relational tables

## SQLite Backend (Optional)

When the `sqlite` feature is enabled, SpaceKit can:
- ✅ Create internal SQLite tables for query optimization
- ✅ Sync in-memory data to SQLite for complex analytics
- ❌ **NOT** accept user SQL queries
- ❌ **NOT** allow user-defined schemas

The SQLite backend is **internal only** - it's used to optimize queries, not as a user-facing SQL interface.

## What This Means for Your Application

### ✅ You CAN:
- Query existing data (files, facts, users) using the query API
- Use JOINs, subqueries, aggregations
- Filter, sort, and paginate results
- Build complex queries programmatically

### ❌ You CANNOT:
- Execute raw SQL strings
- Create custom tables or schemas
- Use PostgreSQL-specific features
- Write DDL statements (CREATE, ALTER, DROP)
- Use SQL stored procedures or triggers

## Migration Path

If you need full SQL-92 support:

1. **Use PostgreSQL alongside SpaceKit**:
   - SpaceKit for encrypted storage
   - PostgreSQL for complex SQL queries
   - Sync data between them

2. **Use SpaceKit's document model**:
   - Store data as JSON documents
   - Use the query interface for filtering
   - Handle relationships in application code

3. **Wait for SQL parser** (future feature):
   - SQL-92 parser is planned (see `ENTERPRISE_GRADE_ROADMAP.md`)
   - Will add raw SQL support while maintaining security

## Documentation References

- **Query API**: See `documentation/api/sql-query-api.md`
- **PostgreSQL Comparison**: See `documentation/guides/postgresql-comparison.md`
- **Roadmap**: See `ENTERPRISE_GRADE_ROADMAP.md` (SQL parser is planned)

## Summary

**SpaceKit Storage Node = SQL-Like Query Interface (Structured Queries)**
- ✅ Query builder pattern
- ✅ Complex queries supported
- ❌ No raw SQL parsing
- ❌ No DDL support

**NOT a SQL-92 compliant database**
- ❌ Cannot parse SQL strings
- ❌ Cannot execute CREATE TABLE
- ❌ Cannot use PostgreSQL DDL

This is by design - SpaceKit prioritizes security, type safety, and API-first architecture over raw SQL compatibility.

