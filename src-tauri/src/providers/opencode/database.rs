use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::Duration,
};

use chrono::{DateTime, Utc};
use rusqlite::{types::ValueRef, Connection, OpenFlags, Row};
use serde_json::Value;

use crate::pricing::ModelPricing;

use super::record::{
    parse_message, parse_part, provider_id, timestamp_from_number, timestamp_from_value,
    ParsedMessage, ParsedPart, UsageRecord, EPOCH_MILLISECONDS_THRESHOLD, GO_PROVIDER_ID,
};

const PROVIDER_JSON: &str = "COALESCE(\
    json_extract(m.data,'$.providerID'),\
    json_extract(m.data,'$.providerId'),\
    json_extract(m.data,'$.provider_id'),\
    json_extract(m.data,'$.model.providerID'),\
    json_extract(m.data,'$.model.providerId'))";
#[derive(Debug, Default)]
pub(crate) struct DatabaseUsage {
    pub(crate) records: Vec<UsageRecord>,
    pub(crate) go_anchor: Option<DateTime<Utc>>,
}

pub(crate) enum DatabaseRead {
    Missing,
    Usable(DatabaseUsage),
}

pub(crate) fn read_database(
    path: &Path,
    cutoff_ms: i64,
    pricing: &ModelPricing,
) -> Result<DatabaseRead, ()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DatabaseRead::Missing)
        }
        Err(_) => return Err(()),
    }

    let connection = open_read_only(path)?;
    let schema = inspect_schema(&connection)?;
    let mut messages = load_messages(&connection, &schema, cutoff_ms)?;
    let parts = load_parts(&connection, &schema, cutoff_ms, &messages)?;
    let go_anchor = load_go_anchor(&connection, &schema)?;

    let mut records = Vec::with_capacity(messages.len());
    for message in messages.drain(..) {
        let message_parts = parts
            .get(&message.message_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        records.push(message.into_usage(message_parts, pricing));
    }
    Ok(DatabaseRead::Usable(DatabaseUsage { records, go_anchor }))
}

pub(crate) fn has_hosted_usage(path: &Path) -> Result<bool, ()> {
    let connection = open_read_only(path)?;
    let schema = inspect_schema(&connection)?;
    let join = schema.session_join_sql();
    let cost = hosted_cost_sql(schema.part.is_some());
    let sql = format!(
        "SELECT EXISTS(\
            SELECT 1 FROM message m{join} \
            WHERE json_valid(m.data) \
              AND json_extract(m.data,'$.role') = 'assistant' \
              AND {PROVIDER_JSON} IN ('opencode-go','opencode') \
              AND {cost} \
            LIMIT 1)"
    );
    connection
        .query_row(&sql, [], |row| row.get::<_, bool>(0))
        .map_err(|_| ())
}

fn numeric_cost_sql(alias: &str) -> String {
    format!(
        "((json_type({alias}.data,'$.cost') IN ('integer','real') \
            AND json_extract({alias}.data,'$.cost') >= 0) OR \
          (json_type({alias}.data,'$.costUSD') IN ('integer','real') \
            AND json_extract({alias}.data,'$.costUSD') >= 0))"
    )
}

fn hosted_cost_sql(has_parts: bool) -> String {
    let message_cost = numeric_cost_sql("m");
    if !has_parts {
        return message_cost;
    }
    let part_cost = numeric_cost_sql("p");
    format!(
        "({message_cost} OR EXISTS(\
            SELECT 1 FROM part p \
            WHERE p.message_id = m.id \
              AND json_valid(p.data) \
              AND json_extract(p.data,'$.type') IN ('step-finish','step_finish') \
              AND {part_cost}))"
    )
}

fn open_read_only(path: &Path) -> Result<Connection, ()> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| ())?;
    connection
        .busy_timeout(Duration::from_millis(150))
        .map_err(|_| ())?;
    Ok(connection)
}

struct DatabaseSchema {
    message_time_created: bool,
    join_sessions: bool,
    part: Option<PartSchema>,
}

struct PartSchema {
    time_created: bool,
}

impl DatabaseSchema {
    fn session_join_sql(&self) -> &'static str {
        if self.join_sessions {
            " INNER JOIN session s ON s.id = m.session_id"
        } else {
            ""
        }
    }
}

fn inspect_schema(connection: &Connection) -> Result<DatabaseSchema, ()> {
    let message = table_columns(connection, "message")?
        .filter(|columns| {
            ["id", "session_id", "data"]
                .iter()
                .all(|column| columns.contains(*column))
        })
        .ok_or(())?;
    let join_sessions =
        table_columns(connection, "session")?.is_some_and(|columns| columns.contains("id"));
    let part = table_columns(connection, "part")?.and_then(|columns| {
        (columns.contains("message_id") && columns.contains("data")).then(|| PartSchema {
            time_created: columns.contains("time_created"),
        })
    });
    Ok(DatabaseSchema {
        message_time_created: message.contains("time_created"),
        join_sessions,
        part,
    })
}

fn table_columns(connection: &Connection, table: &str) -> Result<Option<HashSet<String>>, ()> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| ())?;
    if !exists {
        return Ok(None);
    }

    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|_| ())?;
    let mut rows = statement.query([]).map_err(|_| ())?;
    let mut columns = HashSet::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        if let Some(name) = row_text(row, 1) {
            columns.insert(name);
        }
    }
    Ok(Some(columns))
}

fn load_messages(
    connection: &Connection,
    schema: &DatabaseSchema,
    cutoff_ms: i64,
) -> Result<Vec<ParsedMessage>, ()> {
    let time = if schema.message_time_created {
        "m.time_created"
    } else {
        "NULL"
    };
    let join = schema.session_join_sql();
    let filter = if schema.message_time_created {
        timestamp_filter("m.time_created")
    } else {
        String::new()
    };
    let sql = format!("SELECT m.id, m.session_id, {time}, m.data FROM message m{join}{filter}");
    let mut statement = connection.prepare(&sql).map_err(|_| ())?;
    let mut rows = if schema.message_time_created {
        statement
            .query(rusqlite::params![
                cutoff_ms,
                cutoff_ms.div_euclid(1_000),
                EPOCH_MILLISECONDS_THRESHOLD
            ])
            .map_err(|_| ())?
    } else {
        statement.query([]).map_err(|_| ())?
    };
    let mut messages = Vec::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        let Some(message_id) = row_text(row, 0).and_then(non_empty) else {
            continue;
        };
        let Some(session_id) = row_text(row, 1).and_then(non_empty) else {
            continue;
        };
        let column_timestamp = row_i64(row, 2);
        let Some(data) = row_text(row, 3) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let Some(message) = parse_message(session_id, message_id, column_timestamp, &value) else {
            continue;
        };
        if message.timestamp.timestamp_millis() >= cutoff_ms {
            messages.push(message);
        }
    }
    Ok(messages)
}

fn load_parts(
    connection: &Connection,
    schema: &DatabaseSchema,
    cutoff_ms: i64,
    messages: &[ParsedMessage],
) -> Result<HashMap<String, Vec<ParsedPart>>, ()> {
    let Some(part_schema) = &schema.part else {
        return Ok(HashMap::new());
    };
    let message_ids = messages
        .iter()
        .map(|message| message.message_id.as_str())
        .collect::<HashSet<_>>();
    if message_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let filter = if schema.message_time_created {
        timestamp_filter("m.time_created")
    } else if part_schema.time_created {
        timestamp_filter("p.time_created")
    } else {
        String::new()
    };
    let sql = format!(
        "SELECT p.message_id, p.data FROM part p \
         INNER JOIN message m ON m.id = p.message_id{filter}"
    );
    let mut statement = connection.prepare(&sql).map_err(|_| ())?;
    let mut rows = if schema.message_time_created || part_schema.time_created {
        statement
            .query(rusqlite::params![
                cutoff_ms,
                cutoff_ms.div_euclid(1_000),
                EPOCH_MILLISECONDS_THRESHOLD
            ])
            .map_err(|_| ())?
    } else {
        statement.query([]).map_err(|_| ())?
    };
    let mut parts = HashMap::<String, Vec<ParsedPart>>::new();
    while let Some(row) = rows.next().map_err(|_| ())? {
        let Some(message_id) = row_text(row, 0) else {
            continue;
        };
        if !message_ids.contains(message_id.as_str()) {
            continue;
        }
        let Some(data) = row_text(row, 1) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        let Some(part) = parse_part(&value) else {
            continue;
        };
        parts.entry(message_id).or_default().push(part);
    }
    Ok(parts)
}

fn timestamp_filter(column: &str) -> String {
    format!(" WHERE ({column} >= ?1 OR ({column} >= ?2 AND {column} < ?3))")
}

fn load_go_anchor(
    connection: &Connection,
    schema: &DatabaseSchema,
) -> Result<Option<DateTime<Utc>>, ()> {
    let join = schema.session_join_sql();
    let cost = hosted_cost_sql(schema.part.is_some());
    if schema.message_time_created {
        let sql = format!(
            "SELECT m.time_created FROM message m{join} \
             WHERE json_valid(m.data) \
               AND json_extract(m.data,'$.role') = 'assistant' \
               AND {PROVIDER_JSON} = 'opencode-go' \
               AND {cost}"
        );
        let mut statement = connection.prepare(&sql).map_err(|_| ())?;
        let mut rows = statement.query([]).map_err(|_| ())?;
        let mut anchor: Option<DateTime<Utc>> = None;
        while let Some(row) = rows.next().map_err(|_| ())? {
            if let Some(timestamp) = row_i64(row, 0).and_then(timestamp_from_number) {
                anchor = Some(anchor.map_or(timestamp, |current| current.min(timestamp)));
            }
        }
        return Ok(anchor);
    }

    let sql = format!("SELECT NULL, m.data, {cost} FROM message m{join} WHERE json_valid(m.data)");
    let mut statement = connection.prepare(&sql).map_err(|_| ())?;
    let mut rows = statement.query([]).map_err(|_| ())?;
    let mut anchor: Option<DateTime<Utc>> = None;
    while let Some(row) = rows.next().map_err(|_| ())? {
        let column_timestamp = row_i64(row, 0);
        let Some(data) = row_text(row, 1) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if value.get("role").and_then(Value::as_str) != Some("assistant")
            || provider_id(&value).as_deref() != Some(GO_PROVIDER_ID)
            || !row.get::<_, bool>(2).unwrap_or(false)
        {
            continue;
        }
        let timestamp = column_timestamp
            .and_then(timestamp_from_number)
            .or_else(|| {
                value
                    .pointer("/time/created")
                    .and_then(timestamp_from_value)
            });
        if let Some(timestamp) = timestamp {
            anchor = Some(anchor.map_or(timestamp, |current| current.min(timestamp)));
        }
    }
    Ok(anchor)
}

fn row_text(row: &Row<'_>, index: usize) -> Option<String> {
    match row.get_ref(index).ok()? {
        ValueRef::Text(value) | ValueRef::Blob(value) => {
            std::str::from_utf8(value).ok().map(str::to_owned)
        }
        _ => None,
    }
}

fn row_i64(row: &Row<'_>, index: usize) -> Option<i64> {
    match row.get_ref(index).ok()? {
        ValueRef::Integer(value) => Some(value),
        ValueRef::Real(value) if value.is_finite() => Some(value.trunc() as i64),
        ValueRef::Text(value) => std::str::from_utf8(value).ok()?.parse().ok(),
        _ => None,
    }
}

fn non_empty(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| value.to_owned())
}
