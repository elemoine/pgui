use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

const NULL_REPR: &str = "NULL";

/// Execute a SQL query against the database pool.
pub async fn execute_query(sql: &str, pool: PgPool) -> color_eyre::Result<Vec<PgRow>> {
    let q = sqlx::query(sql);
    let rows = q.fetch_all(&pool).await?;
    Ok(rows)
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

    match row.columns()[idx].type_info().name() {
        "INT2"                        => get!(i16),
        "INT4"                        => get!(i32),
        "INT8"                        => get!(i64),
        "FLOAT4"                      => get!(f32),
        "FLOAT8"                      => get!(f64),
        "BOOL"                        => get!(bool),
        "TEXT" | "VARCHAR" | "BPCHAR" => get!(String),
        _                             => "?".into(),
    }
}
