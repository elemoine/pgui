use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

const NULL_REPR: &str = "NULL";

/// Execute a SQL query against the database pool.
pub async fn execute_query(sql: &str, pool: PgPool) -> color_eyre::Result<Vec<PgRow>> {
    let q = sqlx::query(sql);
    let rows = q.fetch_all(&pool).await?;
    Ok(rows)
}

pub async fn list_tables(pool: PgPool) -> color_eyre::Result<Vec<String>> {
    let sql = "SELECT table_name FROM information_schema.tables WHERE table_schema='public' ORDER BY table_name";
    let rows = execute_query(sql, pool).await?;
    let tables = rows
        .into_iter()
        .map(|row| row.try_get::<String, _>(0).unwrap_or_default())
        .collect();
    Ok(tables)
}

/// Information about a single column of a table.
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

/// List the columns of a table in the `public` schema.
pub async fn list_columns(table: &str, pool: PgPool) -> color_eyre::Result<Vec<ColumnInfo>> {
    let sql = "SELECT column_name, data_type, is_nullable \
               FROM information_schema.columns \
               WHERE table_schema = 'public' AND table_name = $1 \
               ORDER BY ordinal_position";
    let rows = sqlx::query(sql).bind(table).fetch_all(&pool).await?;
    let columns = rows
        .into_iter()
        .map(|row| ColumnInfo {
            name: row.try_get::<String, _>("column_name").unwrap_or_default(),
            data_type: row.try_get::<String, _>("data_type").unwrap_or_default(),
            nullable: row.try_get::<String, _>("is_nullable").unwrap_or_default() == "YES",
        })
        .collect();
    Ok(columns)
}

/// Format a cell value from a row.
pub fn format_column_value(row: &PgRow, idx: usize) -> String {
    use sqlx::{Column as _, TypeInfo as _};

    macro_rules! get {
        ($ty:ty) => {
            row.try_get::<Option<$ty>, _>(idx)
                .ok()
                .flatten()
                .map_or(NULL_REPR.into(), |v| v.to_string())
        };
    }

    use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

    match row.columns()[idx].type_info().name() {
        "INT2" => get!(i16),
        "INT4" => get!(i32),
        "INT8" => get!(i64),
        "FLOAT4" => get!(f32),
        "FLOAT8" => get!(f64),
        "BOOL" => get!(bool),
        "TEXT" | "VARCHAR" | "BPCHAR" => get!(String),
        "DATE" => get!(NaiveDate),
        "TIME" => get!(NaiveTime),
        "TIMESTAMP" => get!(NaiveDateTime),
        "TIMESTAMPTZ" => get!(DateTime<Utc>),
        _ => "?".into(),
    }
}
