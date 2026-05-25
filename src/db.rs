use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row};

/// Execute a SQL query against the database pool.
pub async fn execute_query(sql: &str, pool: PgPool) -> color_eyre::Result<Vec<PgRow>> {
    eprintln!("  Envoi de la requête...");
    let start = std::time::Instant::now();
    let q = sqlx::query(sql);
    let rows = q.fetch_all(&pool).await?;
    let elapsed = start.elapsed();
    eprintln!("✓ Requête terminée en {:.2}s ({} lignes)", elapsed.as_secs_f64(), rows.len());
    Ok(rows)
}

/// Format a cell value from a row.
pub fn cell_to_string(row: &PgRow, idx: usize) -> String {
    use sqlx::{Column as _, TypeInfo as _};

    let ty = row.columns()[idx].type_info().name();
    match ty {
        "INT2" => row
            .try_get::<Option<i16>, _>(idx)
            .ok()
            .flatten()
            .map_or("NULL".into(), |v| v.to_string()),
        "INT4" => row
            .try_get::<Option<i32>, _>(idx)
            .ok()
            .flatten()
            .map_or("NULL".into(), |v| v.to_string()),
        "INT8" => row
            .try_get::<Option<i64>, _>(idx)
            .ok()
            .flatten()
            .map_or("NULL".into(), |v| v.to_string()),
        "TEXT" | "VARCHAR" | "BPCHAR" => row
            .try_get::<Option<String>, _>(idx)
            .ok()
            .flatten()
            .unwrap_or_else(|| "NULL".into()),
        "BOOL" => row
            .try_get::<Option<bool>, _>(idx)
            .ok()
            .flatten()
            .map_or("NULL".into(), |v| v.to_string()),
        _ => "?".into(),
    }
}
