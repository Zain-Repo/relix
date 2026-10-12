use super::{
    CONNECT_TIMEOUT, CancelToken, ConnectionConfig, DbError, Engine, InterruptWatch, QueryResult,
    ResultBudget, ResultColumn,
};
use futures_util::TryStreamExt;
use mysql::prelude::Queryable;
use postgres_native_tls::MakeTlsConnector;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage, config::SslMode};

pub(super) struct PostgresSession {
    runtime: tokio::runtime::Runtime,
    client: Client,
    tls: Option<MakeTlsConnector>,
    driver: tokio::task::JoinHandle<()>,
    valid: bool,
}

impl PostgresSession {
    pub(super) fn connect(config: &ConnectionConfig) -> Result<Self, DbError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|error| DbError(error.to_string()))?;
        let tls = if config.tls {
            Some(MakeTlsConnector::new(
                native_tls::TlsConnector::new().map_err(|error| DbError(error.to_string()))?,
            ))
        } else {
            None
        };
        let mut options = tokio_postgres::Config::new();
        options
            .host(&config.host)
            .port(config.port)
            .dbname(&config.database)
            .user(&config.user)
            .password(&config.password)
            .application_name("Relix")
            .connect_timeout(CONNECT_TIMEOUT)
            .ssl_mode(if config.tls {
                SslMode::Require
            } else {
                SslMode::Disable
            });
        let (client, driver) = runtime.block_on(async {
            let connect = async {
                if let Some(tls) = tls.clone() {
                    let (client, connection) = options.connect(tls).await?;
                    // Client requests surface connection failures. The protocol
                    // driver must continue polling even while no query is active.
                    let driver = tokio::spawn(async move {
                        let _ = connection.await;
                    });
                    Ok::<_, tokio_postgres::Error>((client, driver))
                } else {
                    let (client, connection) = options.connect(NoTls).await?;
                    let driver = tokio::spawn(async move {
                        let _ = connection.await;
                    });
                    Ok((client, driver))
                }
            };
            tokio::time::timeout(CONNECT_TIMEOUT, connect)
                .await
                .map_err(|_| DbError("Connection timed out after 10 seconds.".into()))?
                .map_err(DbError::from)
        })?;
        runtime.block_on(async {
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                client.batch_execute("SET statement_timeout = 60000; SET lock_timeout = 5000;"),
            )
            .await
            .map_err(|_| DbError("Connection setup timed out after 10 seconds.".into()))?
            .map_err(DbError::from)
        })?;
        Ok(Self {
            runtime,
            client,
            tls,
            driver,
            valid: true,
        })
    }

    pub(super) fn query(
        &mut self,
        sql: &str,
        budget: &mut ResultBudget,
        cancel: &CancelToken,
        timeout: Duration,
        read_only: bool,
    ) -> Result<QueryResult, DbError> {
        if !self.valid {
            return Err(DbError(
                "Connection was closed after an interrupted query. Reconnect before continuing."
                    .into(),
            ));
        }
        if read_only {
            super::script::check_read_only_session(sql, Engine::Postgres)?;
        }
        let token = self.client.cancel_token();
        let tls = self.tls.clone();
        let _watch = InterruptWatch::start(cancel.clone(), timeout, move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async {
                    if let Some(tls) = tls {
                        let _ =
                            tokio::time::timeout(CONNECT_TIMEOUT, token.cancel_query(tls)).await;
                    } else {
                        let _ =
                            tokio::time::timeout(CONNECT_TIMEOUT, token.cancel_query(NoTls)).await;
                    }
                });
            }
        });
        let mut cleanup_failed = false;
        let timed_result = self.runtime.block_on(async {
            let execute = async {
                if read_only {
                    self.client.batch_execute("BEGIN READ ONLY;").await?;
                }
                let output = self.query_stream(sql, budget, cancel).await;
                if read_only {
                    // Rollback closes the protective transaction on success as
                    // well, since no writes should ever be committed from it.
                    let cleanup = self.client.batch_execute("ROLLBACK;").await;
                    if cleanup.is_err() {
                        cleanup_failed = true;
                    }
                    if output.is_ok() {
                        cleanup?;
                    }
                }
                output
            };
            tokio::time::timeout(timeout, execute).await
        });
        let result = match timed_result {
            Ok(result) => result,
            Err(_) => {
                // Dropping a request future does not cancel PostgreSQL's server
                // command. Explicitly cancel, close its protocol driver, and
                // prohibit reuse until a new connection is established.
                self.valid = false;
                self.cancel_active();
                self.driver.abort();
                return Err(DbError(
                    "Query timed out; connection closed. Reconnect before continuing.".into(),
                ));
            }
        };
        if cleanup_failed {
            self.valid = false;
            self.cancel_active();
            self.driver.abort();
            return Err(DbError(format!(
                "{} Connection closed because read-only transaction cleanup failed; reconnect before continuing.",
                result
                    .err()
                    .map_or_else(|| "Query failed.".into(), |error| error.to_string())
            )));
        }
        if cancel.is_cancelled() {
            self.cancel_active();
            // An empty request is a protocol barrier: the prior command has
            // finished draining before this connection can be reused.
            let drained = self.runtime.block_on(async {
                tokio::time::timeout(CONNECT_TIMEOUT, self.client.simple_query("")).await
            });
            if !matches!(drained, Ok(Ok(_))) {
                self.valid = false;
                self.driver.abort();
                return Err(DbError("Query cancelled; connection closed because its response could not be drained. Reconnect before continuing.".into()));
            }
            Err(DbError("Query cancelled.".into()))
        } else {
            result
        }
    }

    fn cancel_active(&self) {
        let token = self.client.cancel_token();
        self.runtime.block_on(async {
            if let Some(tls) = self.tls.clone() {
                let _ = tokio::time::timeout(CONNECT_TIMEOUT, token.cancel_query(tls)).await;
            } else {
                let _ = tokio::time::timeout(CONNECT_TIMEOUT, token.cancel_query(NoTls)).await;
            }
        });
    }

    async fn query_stream(
        &self,
        sql: &str,
        budget: &mut ResultBudget,
        cancel: &CancelToken,
    ) -> Result<QueryResult, DbError> {
        let metadata = self.client.prepare(sql).await?;
        let stream = self.client.simple_query_raw(sql).await?;
        futures_util::pin_mut!(stream);
        let mut result = QueryResult::default();
        while let Some(message) = stream.try_next().await? {
            cancel.check()?;
            match message {
                SimpleQueryMessage::RowDescription(columns) => {
                    result.columns = columns
                        .iter()
                        .enumerate()
                        .map(|(index, column)| ResultColumn {
                            name: column.name().to_owned(),
                            data_type: metadata
                                .columns()
                                .get(index)
                                .map_or("text", |column| column.type_().name())
                                .to_owned(),
                        })
                        .collect();
                    budget.columns(&result.columns)?;
                }
                SimpleQueryMessage::Row(row) => {
                    let estimated = (0..row.len()).fold(0_usize, |bytes, index| {
                        bytes.saturating_add(row.get(index).map_or(0, str::len))
                    });
                    if !budget.can_materialize(&mut result, estimated, row.len()) {
                        continue;
                    }
                    let values = (0..row.len())
                        .map(|index| row.get(index).map(str::to_owned))
                        .collect();
                    budget.push(&mut result, values);
                    // Continue draining the bounded stream so the server can
                    // complete side effects (including INSERT ... RETURNING).
                }
                SimpleQueryMessage::CommandComplete(count) if result.columns.is_empty() => {
                    result.affected_rows = count
                }
                _ => {}
            }
        }
        Ok(result)
    }
}

pub(super) struct MySqlSession {
    connection: Option<mysql::Conn>,
    control: Arc<Mutex<mysql::Conn>>,
}

impl MySqlSession {
    pub(super) fn connect(config: &ConnectionConfig) -> Result<Self, DbError> {
        let mut options = mysql::OptsBuilder::new()
            .ip_or_hostname(Some(config.host.clone()))
            .tcp_port(config.port)
            .db_name(Some(config.database.clone()))
            .user(Some(config.user.clone()))
            .pass(Some(config.password.clone()))
            .tcp_connect_timeout(Some(CONNECT_TIMEOUT))
            // This also bounds authentication and TLS handshake socket reads.
            // A separate watcher enforces the total query deadline.
            .read_timeout(Some(CONNECT_TIMEOUT))
            .write_timeout(Some(CONNECT_TIMEOUT));
        if config.tls {
            options = options.ssl_opts(mysql::SslOpts::default());
        }
        let mut connection = mysql::Conn::new(options.clone())?;
        connection.query_drop("SET SESSION MAX_EXECUTION_TIME=60000")?;
        let control = mysql::Conn::new(options)?;
        Ok(Self {
            connection: Some(connection),
            control: Arc::new(Mutex::new(control)),
        })
    }

    pub(super) fn query(
        &mut self,
        sql: &str,
        budget: &mut ResultBudget,
        cancel: &CancelToken,
        timeout: Duration,
        read_only: bool,
    ) -> Result<Vec<QueryResult>, DbError> {
        let connection_id = self.connection.as_ref().ok_or_else(|| DbError("Connection was closed after an interrupted query. Reconnect before continuing.".into()))?.connection_id();
        if read_only {
            super::script::check_read_only_session(sql, Engine::MySql)?;
        }
        let control = self.control.clone();
        let _watch = InterruptWatch::start(cancel.clone(), timeout, move || {
            if let Ok(mut connection) = control.lock() {
                // The ID comes from the protocol, not user text. KILL QUERY
                // stops this session's command without disconnecting it.
                let _ = connection.query_drop(format!("KILL QUERY {connection_id}"));
            }
        });
        if read_only {
            let begin = self
                .connection
                .as_mut()
                .ok_or_else(|| DbError("Connection closed.".into()))?
                .query_drop("START TRANSACTION READ ONLY");
            if let Err(error) = begin {
                if error.is_connectivity_error() {
                    self.invalidate(connection_id);
                }
                return Err(error.into());
            }
        }
        let result = self.query_stream(sql, budget, cancel);
        if result.as_ref().err().is_some_and(|error| error.broken) {
            self.invalidate(connection_id);
            return Err(DbError(format!(
                "{} Connection closed after a protocol failure; reconnect before continuing.",
                result
                    .err()
                    .map_or_else(|| "Query failed.".into(), |error| error.message.to_string())
            )));
        }
        if cancel.is_cancelled() || result.is_err() {
            self.kill_query(connection_id);
            // This harmless protocol barrier verifies that a cancelled or
            // failed command has fully drained before the session is reused.
            let drained = self
                .connection
                .as_mut()
                .ok_or_else(|| DbError("Connection closed.".into()))?
                .query_drop("DO 0");
            if drained.is_err() {
                self.invalidate(connection_id);
                return Err(DbError("Query interrupted; connection closed because its response could not be drained. Reconnect before continuing.".into()));
            }
        }
        if read_only {
            let cleanup = self
                .connection
                .as_mut()
                .ok_or_else(|| DbError("Connection closed.".into()))?
                .query_drop("ROLLBACK");
            if let Err(error) = cleanup {
                self.invalidate(connection_id);
                return Err(DbError(format!(
                    "{error} Connection closed because transaction cleanup failed; reconnect before continuing."
                )));
            }
        }
        if cancel.is_cancelled() {
            Err(DbError("Query cancelled.".into()))
        } else {
            result.map_err(|error| error.message)
        }
    }

    fn kill_query(&self, connection_id: u32) {
        if let Ok(mut control) = self.control.try_lock() {
            let _ = control.query_drop(format!("KILL QUERY {connection_id}"));
        }
    }

    fn invalidate(&mut self, connection_id: u32) {
        self.kill_query(connection_id);
        // Dropping the protocol connection closes its socket. A failed session
        // cannot silently carry a still-running command into the next request.
        drop(self.connection.take());
    }

    fn query_stream(
        &mut self,
        sql: &str,
        budget: &mut ResultBudget,
        cancel: &CancelToken,
    ) -> Result<Vec<QueryResult>, MySqlFailure> {
        let mut stream = self
            .connection
            .as_mut()
            .ok_or_else(|| DbError("Connection closed.".into()))?
            .query_iter(sql)?;
        let mut results = Vec::new();
        while let Some(mut set) = stream.iter() {
            cancel.check()?;
            if results.len() >= 1000 {
                return Err(
                    DbError("A command can return at most 1,000 result sets.".into()).into(),
                );
            }
            let mut result = QueryResult {
                columns: set
                    .columns()
                    .as_ref()
                    .iter()
                    .map(|column| ResultColumn {
                        name: column.name_str().into_owned(),
                        data_type: format!("{:?}", column.column_type()),
                    })
                    .collect(),
                affected_rows: set.affected_rows(),
                ..Default::default()
            };
            budget.columns(&result.columns)?;
            for row in set.by_ref() {
                cancel.check()?;
                let row = row?;
                let estimated = (0..row.len()).fold(0_usize, |bytes, index| {
                    bytes.saturating_add(row.as_ref(index).map_or(0, mysql_size))
                });
                if !budget.can_materialize(&mut result, estimated, row.len()) {
                    continue;
                }
                let values = (0..row.len())
                    .map(|index| row.as_ref(index).and_then(mysql_value))
                    .collect();
                budget.push(&mut result, values);
            }
            results.push(result);
        }
        Ok(results)
    }
}

struct MySqlFailure {
    message: DbError,
    broken: bool,
}
impl From<DbError> for MySqlFailure {
    fn from(message: DbError) -> Self {
        Self {
            message,
            broken: false,
        }
    }
}
impl From<mysql::Error> for MySqlFailure {
    fn from(error: mysql::Error) -> Self {
        let broken = error.is_connectivity_error();
        Self {
            message: error.into(),
            broken,
        }
    }
}

fn mysql_size(value: &mysql::Value) -> usize {
    match value {
        mysql::Value::NULL => 0,
        mysql::Value::Bytes(value) => {
            if std::str::from_utf8(value).is_ok() {
                value.len()
            } else {
                value.len().saturating_mul(2).saturating_add(2)
            }
        }
        _ => 128,
    }
}

fn mysql_value(value: &mysql::Value) -> Option<String> {
    match value {
        mysql::Value::NULL => None,
        mysql::Value::Bytes(value) => Some(
            String::from_utf8(value.clone()).unwrap_or_else(|_| super::embedded::hex_blob(value)),
        ),
        mysql::Value::Int(value) => Some(value.to_string()),
        mysql::Value::UInt(value) => Some(value.to_string()),
        mysql::Value::Float(value) => Some(value.to_string()),
        mysql::Value::Double(value) => Some(value.to_string()),
        mysql::Value::Date(year, month, day, hour, minute, second, micros) => Some(format!(
            "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"
        )),
        mysql::Value::Time(negative, days, hours, minutes, seconds, micros) => Some(format!(
            "{}{days}d {hours:02}:{minutes:02}:{seconds:02}.{micros:06}",
            if *negative { "-" } else { "" }
        )),
    }
}
