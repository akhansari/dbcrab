use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fmt,
    fs::{File as StdFile, OpenOptions as StdOpenOptions},
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use futures_util::{Stream, StreamExt};
use sqlparser::{ast::Statement, dialect::PostgreSqlDialect, parser::Parser};
use sqlx::{
    Connection, PgConnection, PgPool, Row, Transaction, pool::PoolConnection, postgres::Postgres,
};
use tokio::{
    fs::{self, File},
    io::{AsyncReadExt, AsyncWriteExt},
    time,
};

use crate::{
    catalog::quote_identifier,
    errors::{AppError, AppResult},
    paths,
};

const COPY_BUFFER_SIZE: usize = 64 * 1024;
const TTY_PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
const REDIRECTED_PROGRESS_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct ExportOptions {
    pub output: PathBuf,
    pub header: bool,
    pub force: bool,
    pub format_explicit: bool,
}

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub input: PathBuf,
    pub header: bool,
    pub format_explicit: bool,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TransferSummary {
    pub operation: TransferOperation,
    pub source: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub rows: Option<u64>,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TransferOperation {
    Export,
    Import,
}

impl fmt::Display for TransferOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Export => "export",
            Self::Import => "import",
        })
    }
}

#[derive(Debug)]
struct Relation {
    schema: String,
    name: String,
    kind: String,
    columns: Vec<RelationColumn>,
}

#[derive(Debug)]
struct RelationColumn {
    name: String,
    generated: bool,
}

#[derive(Debug)]
struct PreparedInput {
    file: File,
    prefix: Vec<u8>,
    headers: Option<Vec<String>>,
    size: u64,
}

struct CapturingReader<R> {
    inner: R,
    captured: Vec<u8>,
}

struct TempOutput {
    file: File,
    path: TempPath,
}

struct TempPath {
    path: PathBuf,
    armed: bool,
}

enum TransferFailure {
    Recoverable(AppError),
    ConnectionReset(AppError),
}

struct Progress {
    operation: &'static str,
    total: Option<u64>,
    terminal: bool,
    started: Instant,
}

pub async fn export_table(
    pool: &PgPool,
    target: &str,
    options: ExportOptions,
) -> AppResult<TransferSummary> {
    let started = Instant::now();
    let destination = prepare_destination(&options).await?;
    let mut temp = create_temp_output(&destination).await?;
    let mut connection = pool.acquire().await?;
    let (source, bytes) = export_table_to_file(
        pool,
        &mut connection,
        target,
        options.header,
        temp.file_mut(),
    )
    .await
    .map_err(|failure| handle_transfer_failure(&mut connection, failure))?;

    finish_export(temp, destination, options.force, source, started, bytes).await
}

pub async fn export_query(
    pool: &PgPool,
    query: &str,
    options: ExportOptions,
) -> AppResult<TransferSummary> {
    let started = Instant::now();
    let query = normalize_export_query(query)?;
    let destination = prepare_destination(&options).await?;
    let mut temp = create_temp_output(&destination).await?;
    let mut connection = pool.acquire().await?;
    let bytes = export_query_to_file(
        pool,
        &mut connection,
        &query,
        options.header,
        temp.file_mut(),
    )
    .await
    .map_err(|failure| handle_transfer_failure(&mut connection, failure))?;

    finish_export(
        temp,
        destination,
        options.force,
        "query".to_owned(),
        started,
        bytes,
    )
    .await
}

pub async fn import_table(
    pool: &PgPool,
    target: &str,
    options: ImportOptions,
) -> AppResult<TransferSummary> {
    let started = Instant::now();
    let path = prepare_input_path(&options)?;
    let mut input = prepare_input(path.clone(), options.header).await?;
    let mut connection = pool.acquire().await?;
    let (source, rows, bytes) =
        import_table_from_file(&mut connection, target, options.header, &mut input)
            .await
            .map_err(|failure| handle_transfer_failure(&mut connection, failure))?;

    Ok(TransferSummary {
        operation: TransferOperation::Import,
        source,
        path,
        bytes,
        rows: Some(rows),
        elapsed: started.elapsed(),
    })
}

async fn export_table_to_file(
    pool: &PgPool,
    connection: &mut PgConnection,
    target: &str,
    header: bool,
    file: &mut File,
) -> Result<(String, u64), TransferFailure> {
    let mut transaction = begin_transfer(connection, true).await?;
    let relation = match resolve_relation(transaction.as_mut(), target).await {
        Ok(relation) => relation,
        Err(err) => {
            return Err(rollback_transfer(transaction, TransferFailure::Recoverable(err)).await);
        }
    };
    if !is_export_relation(&relation.kind) {
        let error = AppError::message(format!(
            "`{target}` is not a selectable table, view, or materialized view"
        ));
        return Err(rollback_transfer(transaction, TransferFailure::Recoverable(error)).await);
    }

    let columns = relation
        .columns
        .iter()
        .filter(|column| !column.generated)
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>();
    if columns.is_empty() {
        let error = AppError::message(format!("relation `{target}` has no exportable columns"));
        return Err(rollback_transfer(transaction, TransferFailure::Recoverable(error)).await);
    }

    let source = relation.qualified_name();
    let query = format!("select {} from {source}", columns.join(", "));
    let copy = copy_out_statement(&query, header);
    let copy_result = copy_to_file(pool, transaction.as_mut(), &copy, file).await;
    let bytes = complete_transfer(transaction, copy_result).await?;
    Ok((source, bytes))
}

async fn export_query_to_file(
    pool: &PgPool,
    connection: &mut PgConnection,
    query: &str,
    header: bool,
    file: &mut File,
) -> Result<u64, TransferFailure> {
    let mut transaction = begin_transfer(connection, true).await?;
    let copy = copy_out_statement(query, header);
    let copy_result = copy_to_file(pool, transaction.as_mut(), &copy, file).await;
    complete_transfer(transaction, copy_result).await
}

async fn import_table_from_file(
    connection: &mut PgConnection,
    target: &str,
    header: bool,
    input: &mut PreparedInput,
) -> Result<(String, u64, u64), TransferFailure> {
    let mut transaction = begin_transfer(connection, false).await?;
    let relation = match resolve_relation(transaction.as_mut(), target).await {
        Ok(relation) => relation,
        Err(err) => {
            return Err(rollback_transfer(transaction, TransferFailure::Recoverable(err)).await);
        }
    };
    if !is_import_relation(&relation.kind) {
        let error = AppError::message(format!(
            "`{target}` is not an ordinary, partitioned, or foreign table"
        ));
        return Err(rollback_transfer(transaction, TransferFailure::Recoverable(error)).await);
    }

    let column_names = match input.headers.as_deref() {
        Some(headers) => match map_headers(headers, &relation.columns) {
            Ok(columns) => Some(columns),
            Err(err) => {
                return Err(
                    rollback_transfer(transaction, TransferFailure::Recoverable(err)).await,
                );
            }
        },
        None => None,
    };
    let source = relation.qualified_name();
    let copy = copy_in_statement(&source, column_names.as_deref(), header);
    let copy_result = copy_from_file(
        transaction.as_mut(),
        &copy,
        &mut input.file,
        &input.prefix,
        input.size,
    )
    .await;
    let (rows, bytes) = complete_transfer(transaction, copy_result).await?;
    Ok((source, rows, bytes))
}

async fn begin_transfer(
    connection: &mut PgConnection,
    read_only: bool,
) -> Result<Transaction<'_, Postgres>, TransferFailure> {
    let transaction = if read_only {
        connection.begin_with("begin read only").await
    } else {
        connection.begin().await
    };
    let mut transaction = transaction.map_err(classify_transaction_error)?;
    let settings = async {
        sqlx::query("set local datestyle = 'ISO'")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("set local intervalstyle = 'postgres'")
            .execute(&mut *transaction)
            .await?;
        Ok::<_, sqlx::Error>(())
    }
    .await;

    match settings {
        Ok(()) => Ok(transaction),
        Err(err) => {
            Err(rollback_transfer(transaction, TransferFailure::Recoverable(err.into())).await)
        }
    }
}

async fn complete_transfer<T>(
    transaction: Transaction<'_, Postgres>,
    result: Result<T, TransferFailure>,
) -> Result<T, TransferFailure> {
    match result {
        Ok(value) => transaction
            .commit()
            .await
            .map(|()| value)
            .map_err(classify_transaction_error),
        Err(failure) => Err(rollback_transfer(transaction, failure).await),
    }
}

fn classify_transaction_error(err: sqlx::Error) -> TransferFailure {
    if matches!(&err, sqlx::Error::Database(_)) {
        TransferFailure::Recoverable(err.into())
    } else {
        TransferFailure::ConnectionReset(err.into())
    }
}

async fn rollback_transfer(
    transaction: Transaction<'_, Postgres>,
    failure: TransferFailure,
) -> TransferFailure {
    match failure {
        TransferFailure::ConnectionReset(err) => {
            drop(transaction);
            TransferFailure::ConnectionReset(err)
        }
        TransferFailure::Recoverable(err) => match transaction.rollback().await {
            Ok(()) => TransferFailure::Recoverable(err),
            Err(_) => TransferFailure::ConnectionReset(err),
        },
    }
}

fn handle_transfer_failure(
    connection: &mut PoolConnection<Postgres>,
    failure: TransferFailure,
) -> AppError {
    match failure {
        TransferFailure::Recoverable(err) => err,
        TransferFailure::ConnectionReset(err) => {
            connection.close_on_drop();
            err
        }
    }
}

async fn finish_export(
    temp: TempOutput,
    destination: PathBuf,
    force: bool,
    source: String,
    started: Instant,
    bytes: u64,
) -> AppResult<TransferSummary> {
    temp.publish(destination.clone(), force).await?;

    Ok(TransferSummary {
        operation: TransferOperation::Export,
        source,
        path: destination,
        bytes,
        rows: None,
        elapsed: started.elapsed(),
    })
}

async fn copy_to_file(
    pool: &PgPool,
    connection: &mut PgConnection,
    statement: &str,
    file: &mut File,
) -> Result<u64, TransferFailure> {
    let backend_pid: i32 = sqlx::query_scalar("select pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .map_err(|err| TransferFailure::Recoverable(err.into()))?;
    let mut stream = connection
        .copy_out_raw(statement)
        .await
        .map_err(|err| TransferFailure::Recoverable(err.into()))?;
    let mut cancel = Box::pin(tokio::signal::ctrl_c());
    let mut interval = progress_interval();
    let progress = Progress::new("Exported", None);
    let mut bytes = 0_u64;

    loop {
        tokio::select! {
            signal = &mut cancel => {
                let signal_error = signal.err().map(AppError::from);
                let recovered = cancel_copy_out(pool, backend_pid, &mut stream).await;
                progress.finish(bytes);
                if !recovered {
                    drop(stream);
                    return Err(TransferFailure::ConnectionReset(AppError::message(
                        "export cancelled; PostgreSQL cancellation failed, so the session connection was reset",
                    )));
                }
                return Err(TransferFailure::Recoverable(
                    signal_error.unwrap_or_else(|| AppError::message("export cancelled")),
                ));
            }
            _ = interval.tick() => progress.report(bytes),
            item = stream.next() => match item {
                Some(Ok(chunk)) => {
                    if let Err(err) = file.write_all(&chunk).await {
                        let recovered = cancel_copy_out(pool, backend_pid, &mut stream).await;
                        progress.finish(bytes);
                        if !recovered {
                            drop(stream);
                            return Err(TransferFailure::ConnectionReset(err.into()));
                        }
                        return Err(TransferFailure::Recoverable(err.into()));
                    }
                    bytes = bytes.saturating_add(chunk.len() as u64);
                }
                Some(Err(err)) => {
                    progress.finish(bytes);
                    return Err(TransferFailure::Recoverable(err.into()));
                }
                None => {
                    progress.finish(bytes);
                    return Ok(bytes);
                }
            },
        }
    }
}

async fn cancel_copy_out<S, B>(pool: &PgPool, backend_pid: i32, stream: &mut S) -> bool
where
    S: Stream<Item = Result<B, sqlx::Error>> + Unpin,
{
    match cancel_backend(pool, backend_pid).await {
        Ok(true) => {
            while stream.next().await.is_some() {}
            true
        }
        Ok(false) | Err(_) => false,
    }
}

async fn cancel_backend(pool: &PgPool, backend_pid: i32) -> AppResult<bool> {
    let options = pool.connect_options();
    let mut connection = PgConnection::connect_with(options.as_ref()).await?;
    let cancelled = sqlx::query_scalar("select pg_catalog.pg_cancel_backend($1)")
        .bind(backend_pid)
        .fetch_one(&mut connection)
        .await?;
    let _ = connection.close().await;
    Ok(cancelled)
}

async fn copy_from_file(
    connection: &mut PgConnection,
    statement: &str,
    file: &mut File,
    prefix: &[u8],
    total: u64,
) -> Result<(u64, u64), TransferFailure> {
    let mut copy = connection
        .copy_in_raw(statement)
        .await
        .map_err(|err| TransferFailure::Recoverable(err.into()))?;
    let mut cancel = Box::pin(tokio::signal::ctrl_c());
    let mut interval = progress_interval();
    let progress = Progress::new("Imported", Some(total));
    let mut bytes = 0_u64;

    if !prefix.is_empty() {
        let sent = tokio::select! {
            signal = &mut cancel => {
                progress.finish(bytes);
                let error = signal.err().map(AppError::from).unwrap_or_else(|| AppError::message("import cancelled"));
                return Err(abort_copy(copy, "DBCrab import cancelled", error).await);
            }
            result = copy.send(prefix) => result,
        };
        if let Err(err) = sent {
            progress.finish(bytes);
            return Err(abort_copy(copy, "DBCrab import failed", err.into()).await);
        }
        bytes = bytes.saturating_add(prefix.len() as u64);
    }

    let mut buffer = vec![0_u8; COPY_BUFFER_SIZE];
    loop {
        let read = tokio::select! {
            signal = &mut cancel => {
                progress.finish(bytes);
                let error = signal.err().map(AppError::from).unwrap_or_else(|| AppError::message("import cancelled"));
                return Err(abort_copy(copy, "DBCrab import cancelled", error).await);
            }
            _ = interval.tick() => {
                progress.report(bytes);
                continue;
            }
            result = file.read(&mut buffer) => result,
        };

        let read = match read {
            Ok(read) => read,
            Err(err) => {
                progress.finish(bytes);
                return Err(
                    abort_copy(copy, "DBCrab could not read the import file", err.into()).await,
                );
            }
        };
        if read == 0 {
            break;
        }

        let sent = tokio::select! {
            signal = &mut cancel => {
                progress.finish(bytes);
                let error = signal.err().map(AppError::from).unwrap_or_else(|| AppError::message("import cancelled"));
                return Err(abort_copy(copy, "DBCrab import cancelled", error).await);
            }
            result = copy.send(&buffer[..read]) => result,
        };
        if let Err(err) = sent {
            progress.finish(bytes);
            return Err(abort_copy(copy, "DBCrab import failed", err.into()).await);
        }
        bytes = bytes.saturating_add(read as u64);
    }

    match copy.finish().await {
        Ok(rows) => {
            progress.finish(bytes);
            Ok((rows, bytes))
        }
        Err(err) => {
            progress.finish(bytes);
            Err(TransferFailure::Recoverable(err.into()))
        }
    }
}

async fn abort_copy(
    copy: sqlx::postgres::PgCopyIn<&mut PgConnection>,
    reason: &'static str,
    error: AppError,
) -> TransferFailure {
    match copy.abort(reason).await {
        Ok(()) => TransferFailure::Recoverable(error),
        Err(_) => TransferFailure::ConnectionReset(error),
    }
}

async fn resolve_relation(connection: &mut PgConnection, target: &str) -> AppResult<Relation> {
    let row = sqlx::query(
        r#"
        select n.nspname as schema,
               c.relname as name,
               c.relkind::text as kind,
               c.oid::bigint as oid
        from pg_catalog.pg_class c
        join pg_catalog.pg_namespace n on n.oid = c.relnamespace
        where c.oid = pg_catalog.to_regclass($1)
        "#,
    )
    .bind(target)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or_else(|| AppError::message(format!("relation `{target}` was not found")))?;

    let oid: i64 = row.try_get("oid")?;
    let column_rows = sqlx::query(
        r#"
        select a.attname as name,
               a.attgenerated <> '' as generated
        from pg_catalog.pg_attribute a
        where a.attrelid::bigint = $1
          and a.attnum > 0
          and not a.attisdropped
        order by a.attnum
        "#,
    )
    .bind(oid)
    .fetch_all(&mut *connection)
    .await?;
    let columns = column_rows
        .into_iter()
        .map(|row| {
            Ok(RelationColumn {
                name: row.try_get("name")?,
                generated: row.try_get("generated")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(Relation {
        schema: row.try_get("schema")?,
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        columns,
    })
}

impl Relation {
    fn qualified_name(&self) -> String {
        format!(
            "{}.{}",
            quote_identifier(&self.schema),
            quote_identifier(&self.name)
        )
    }
}

fn is_export_relation(kind: &str) -> bool {
    matches!(kind, "r" | "p" | "f" | "v" | "m")
}

fn is_import_relation(kind: &str) -> bool {
    matches!(kind, "r" | "p" | "f")
}

fn map_headers(headers: &[String], columns: &[RelationColumn]) -> AppResult<Vec<String>> {
    let available = columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();

    headers
        .iter()
        .map(|header| {
            if !seen.insert(header.as_str()) {
                return Err(AppError::message(format!(
                    "CSV header contains duplicate column `{header}`"
                )));
            }
            let column = available.get(header.as_str()).ok_or_else(|| {
                AppError::message(format!(
                    "CSV header column `{header}` does not exist in the target table"
                ))
            })?;
            if column.generated {
                return Err(AppError::message(format!(
                    "CSV header column `{header}` is generated and cannot be imported"
                )));
            }
            Ok(header.clone())
        })
        .collect()
}

fn copy_out_statement(query: &str, header: bool) -> String {
    format!(
        "copy ({query}) to stdout with (format csv, header {}, encoding 'UTF8')",
        sql_bool(header)
    )
}

fn copy_in_statement(relation: &str, columns: Option<&[String]>, header: bool) -> String {
    let columns = columns.map_or_else(String::new, |columns| {
        format!(
            " ({})",
            columns
                .iter()
                .map(|column| quote_identifier(column))
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    format!(
        "copy {relation}{columns} from stdin with (format csv, header {}, encoding 'UTF8')",
        sql_bool(header)
    )
}

fn sql_bool(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

fn normalize_export_query(input: &str) -> AppResult<String> {
    let statements = Parser::parse_sql(&PostgreSqlDialect {}, input)
        .map_err(|err| AppError::message(format!("invalid export query: {err}")))?;
    match statements.as_slice() {
        [Statement::Query(query)] => Ok(query.to_string()),
        [] => Err(AppError::message("export query requires one SQL statement")),
        [_] => Err(AppError::message(
            "export query only accepts SELECT, WITH, VALUES, or TABLE",
        )),
        _ => Err(AppError::message(
            "export query accepts exactly one SQL statement",
        )),
    }
}

fn prepare_input_path(options: &ImportOptions) -> AppResult<PathBuf> {
    let path = paths::expand_home(&options.input);
    validate_csv_path(&path, options.format_explicit)?;
    Ok(path)
}

async fn prepare_destination(options: &ExportOptions) -> AppResult<PathBuf> {
    let path = paths::expand_home(&options.output);
    validate_csv_path(&path, options.format_explicit)?;
    if cfg!(windows) && options.force {
        return Err(AppError::message(
            "--force export is not supported on Windows",
        ));
    }
    if !options.force && fs::try_exists(&path).await? {
        return Err(AppError::message(format!(
            "export destination `{}` already exists; pass --force to replace it",
            path.display()
        )));
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).await?;
    }
    Ok(path)
}

fn validate_csv_path(path: &Path, format_explicit: bool) -> AppResult<()> {
    if format_explicit
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
    {
        return Ok(());
    }

    Err(AppError::message(format!(
        "cannot infer CSV format from `{}`; use a .csv extension or pass --format csv",
        path.display()
    )))
}

async fn prepare_input(path: PathBuf, header: bool) -> AppResult<PreparedInput> {
    if !header {
        let file = File::open(&path).await?;
        let size = file.metadata().await?.len();
        return Ok(PreparedInput {
            file,
            prefix: Vec::new(),
            headers: None,
            size,
        });
    }

    tokio::task::spawn_blocking(move || prepare_header_input(&path))
        .await
        .map_err(|err| AppError::message(format!("CSV header task failed: {err}")))?
}

fn prepare_header_input(path: &Path) -> AppResult<PreparedInput> {
    let file = StdFile::open(path)?;
    let size = file.metadata()?.len();
    let capture = CapturingReader {
        inner: file,
        captured: Vec::new(),
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(capture);
    let record = reader
        .byte_records()
        .next()
        .transpose()
        .map_err(|err| AppError::message(format!("invalid CSV header: {err}")))?
        .ok_or_else(|| AppError::message("CSV import file is empty and has no header"))?;
    let headers = record
        .iter()
        .map(|header| {
            std::str::from_utf8(header)
                .map(str::to_owned)
                .map_err(|_| AppError::message("CSV header is not valid UTF-8"))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let capture = reader.into_inner();

    Ok(PreparedInput {
        file: File::from_std(capture.inner),
        prefix: capture.captured,
        headers: Some(headers),
        size,
    })
}

impl<R: Read> Read for CapturingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.captured.extend_from_slice(&buffer[..read]);
        Ok(read)
    }
}

impl TempOutput {
    fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    async fn publish(mut self, destination: PathBuf, force: bool) -> AppResult<()> {
        self.file.flush().await?;
        self.file.sync_all().await?;
        drop(self.file);
        publish_temp_file(self.path, destination, force).await
    }
}

impl TempPath {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn as_path(&self) -> &Path {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

async fn create_temp_output(destination: &Path) -> AppResult<TempOutput> {
    let destination = destination.to_owned();
    tokio::task::spawn_blocking(move || create_temp_output_blocking(&destination))
        .await
        .map_err(|err| AppError::message(format!("temporary export creation task failed: {err}")))?
}

fn create_temp_output_blocking(destination: &Path) -> AppResult<TempOutput> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = destination
        .file_name()
        .ok_or_else(|| AppError::message("export destination must include a file name"))?;
    let process_id = std::process::id();

    for attempt in 0..1000_u16 {
        let mut temp_name = OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(".dbcrab-{process_id}-{attempt}.tmp"));
        let path = parent.join(temp_name);
        match StdOpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                return Ok(TempOutput {
                    file: File::from_std(file),
                    path: TempPath::new(path),
                });
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }

    Err(AppError::message(
        "could not create a unique temporary export file",
    ))
}

async fn publish_temp_file(temp: TempPath, destination: PathBuf, force: bool) -> AppResult<()> {
    tokio::task::spawn_blocking(move || publish_temp_file_blocking(temp, &destination, force))
        .await
        .map_err(|err| AppError::message(format!("export publication task failed: {err}")))?
}

fn publish_temp_file_blocking(
    mut temp: TempPath,
    destination: &Path,
    force: bool,
) -> AppResult<()> {
    if force {
        replace_temp_file(temp.as_path(), destination)?;
        temp.disarm();
    } else {
        // Unlike rename, hard-linking cannot replace a concurrently created destination.
        std::fs::hard_link(temp.as_path(), destination)?;
        match std::fs::remove_file(temp.as_path()) {
            Ok(()) => temp.disarm(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => temp.disarm(),
            Err(err) => {
                eprintln!(
                    "warning: export succeeded but temporary link `{}` could not be removed: {err}",
                    temp.as_path().display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn replace_temp_file(temp: &Path, destination: &Path) -> AppResult<()> {
    std::fs::rename(temp, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_temp_file(_temp: &Path, _destination: &Path) -> AppResult<()> {
    Err(AppError::message(
        "--force export is not supported on Windows",
    ))
}

#[cfg(not(any(unix, windows)))]
fn replace_temp_file(_temp: &Path, _destination: &Path) -> AppResult<()> {
    Err(AppError::message(
        "--force export is not supported on this platform",
    ))
}

fn progress_interval() -> time::Interval {
    let duration = if io::stderr().is_terminal() {
        TTY_PROGRESS_INTERVAL
    } else {
        REDIRECTED_PROGRESS_INTERVAL
    };
    let mut interval = time::interval_at(time::Instant::now() + duration, duration);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
    interval
}

impl Progress {
    fn new(operation: &'static str, total: Option<u64>) -> Self {
        Self {
            operation,
            total,
            terminal: io::stderr().is_terminal(),
            started: Instant::now(),
        }
    }

    fn report(&self, bytes: u64) {
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let rate = bytes as f64 / elapsed;
        let message = match self.total {
            Some(total) if total > 0 => format!(
                "{} {} / {} ({:.1}%) at {}/s",
                self.operation,
                human_bytes(bytes),
                human_bytes(total),
                bytes as f64 * 100.0 / total as f64,
                human_bytes(rate as u64),
            ),
            _ => format!(
                "{} {} at {}/s",
                self.operation,
                human_bytes(bytes),
                human_bytes(rate as u64),
            ),
        };

        if self.terminal {
            eprint!("\r{message}");
            let _ = io::stderr().flush();
        } else {
            eprintln!("{message}");
        }
    }

    fn finish(&self, bytes: u64) {
        if self.terminal {
            eprint!("\r\x1b[2K");
            let _ = io::stderr().flush();
        } else if self.started.elapsed() >= REDIRECTED_PROGRESS_INTERVAL {
            eprintln!("{} {}", self.operation, human_bytes(bytes));
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::postgres::PgPoolOptions;

    static TEST_FILE_ID: AtomicU64 = AtomicU64::new(0);

    fn columns() -> Vec<RelationColumn> {
        vec![
            RelationColumn {
                name: "id".to_owned(),
                generated: false,
            },
            RelationColumn {
                name: "name".to_owned(),
                generated: false,
            },
            RelationColumn {
                name: "slug".to_owned(),
                generated: true,
            },
        ]
    }

    #[test]
    fn headers_map_in_file_order() {
        // Given
        let headers = vec!["name".to_owned(), "id".to_owned()];

        // When
        let mapped = map_headers(&headers, &columns()).expect("headers should map");

        // Then
        assert_eq!(mapped, ["name", "id"]);
    }

    #[test]
    fn duplicate_header_is_rejected() {
        // Given
        let headers = vec!["id".to_owned(), "id".to_owned()];

        // When
        let error = map_headers(&headers, &columns()).expect_err("duplicate should fail");

        // Then
        assert!(error.to_string().contains("duplicate column `id`"));
    }

    #[test]
    fn generated_header_is_rejected() {
        // Given
        let headers = vec!["slug".to_owned()];

        // When
        let error = map_headers(&headers, &columns()).expect_err("generated should fail");

        // Then
        assert!(error.to_string().contains("generated"));
    }

    #[test]
    fn unknown_header_is_rejected() {
        // Given
        let headers = vec!["missing".to_owned()];

        // When
        let error = map_headers(&headers, &columns()).expect_err("unknown should fail");

        // Then
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn export_query_accepts_one_trailing_semicolon() {
        // Given
        let query = "select ';' as separator;";

        // When
        let normalized = normalize_export_query(query).expect("query should be accepted");

        // Then
        assert_eq!(normalized, "SELECT ';' AS separator");
    }

    #[test]
    fn export_query_rejects_multiple_statements() {
        // Given
        let query = "select 1; select 2";

        // When
        let error = normalize_export_query(query).expect_err("multiple statements should fail");

        // Then
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn export_query_rejects_mutation() {
        // Given
        let query = "delete from users returning id";

        // When
        let error = normalize_export_query(query).expect_err("mutation should fail");

        // Then
        assert!(error.to_string().contains("only accepts SELECT"));
    }

    #[test]
    fn export_query_rejects_copy_wrapper_escape() {
        // Given
        let query = "select 1) to program 'command' --";

        // When
        let error = normalize_export_query(query).expect_err("wrapper escape should fail");

        // Then
        assert!(error.to_string().contains("invalid export query"));
    }

    #[test]
    fn csv_extension_is_inferred_case_insensitively() {
        // Given
        let path = Path::new("users.CSV");

        // When
        let result = validate_csv_path(path, false);

        // Then
        assert!(result.is_ok());
    }

    #[test]
    fn unknown_extension_requires_explicit_format() {
        // Given
        let path = Path::new("users.data");

        // When
        let error = validate_csv_path(path, false).expect_err("format should be required");

        // Then
        assert!(error.to_string().contains("--format csv"));
    }

    #[test]
    fn copy_in_quotes_mixed_case_columns() {
        // Given
        let columns = vec!["id".to_owned(), "Display Name".to_owned()];

        // When
        let statement = copy_in_statement("public.users", Some(&columns), true);

        // Then
        assert_eq!(
            statement,
            "copy public.users (id, \"Display Name\") from stdin with (format csv, header true, encoding 'UTF8')"
        );
    }

    #[test]
    fn headerless_copy_uses_physical_column_order() {
        // Given
        let relation = "public.users";

        // When
        let statement = copy_in_statement(relation, None, false);

        // Then
        assert_eq!(
            statement,
            "copy public.users from stdin with (format csv, header false, encoding 'UTF8')"
        );
    }

    #[tokio::test]
    async fn publish_without_force_does_not_replace_existing_file() {
        // Given
        let destination = test_csv_path("existing");
        fs::write(&destination, b"original")
            .await
            .expect("destination fixture should be written");
        let mut temp = create_temp_output(&destination)
            .await
            .expect("temporary output should be created");
        temp.file
            .write_all(b"replacement")
            .await
            .expect("temporary output should be written");
        let temp_path = temp.path.as_path().to_owned();

        // When
        let result = temp.publish(destination.clone(), false).await;

        // Then
        let contents = fs::read(&destination)
            .await
            .expect("destination should be readable");
        assert!(result.is_err());
        assert_eq!(contents, b"original");
        assert!(
            !fs::try_exists(&temp_path)
                .await
                .expect("temporary path should be checked")
        );
        remove_test_file(&destination).await;
    }

    #[tokio::test]
    async fn dropped_temp_output_removes_temporary_file() {
        // Given
        let destination = test_csv_path("dropped-temp");
        let temp = create_temp_output(&destination)
            .await
            .expect("temporary output should be created");
        let temp_path = temp.path.as_path().to_owned();

        // When
        drop(temp);

        // Then
        assert!(
            !fs::try_exists(temp_path)
                .await
                .expect("temporary path should be checked")
        );
    }

    #[tokio::test]
    async fn published_temp_output_preserves_destination() {
        // Given
        let destination = test_csv_path("published-temp");
        let mut temp = create_temp_output(&destination)
            .await
            .expect("temporary output should be created");
        temp.file
            .write_all(b"published")
            .await
            .expect("temporary output should be written");
        let temp_path = temp.path.as_path().to_owned();

        // When
        temp.publish(destination.clone(), false)
            .await
            .expect("temporary output should publish");

        // Then
        let contents = fs::read(&destination)
            .await
            .expect("destination should be readable");
        assert_eq!(contents, b"published");
        assert!(
            !fs::try_exists(temp_path)
                .await
                .expect("temporary path should be checked")
        );
        remove_test_file(&destination).await;
    }

    #[test]
    fn byte_formatter_uses_binary_units() {
        // Given
        let bytes = 1536;

        // When
        let formatted = human_bytes(bytes);

        // Then
        assert_eq!(formatted, "1.5 KiB");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_table_export_import_round_trips_csv() {
        // Given
        let pool = live_pool().await;
        sqlx::query(
            r#"
            create temporary table export_source (
                id integer generated always as identity,
                name text not null,
                note text,
                slug text generated always as (lower(name)) stored
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("source table should be created");
        sqlx::query(
            "insert into export_source (name, note) values ('Alice, A', null), (E'Line\\nBreak', '')",
        )
        .execute(&pool)
        .await
        .expect("source rows should be inserted");
        sqlx::query(
            r#"
            create temporary table import_target (
                id integer generated always as identity,
                name text not null,
                note text,
                slug text generated always as (lower(name)) stored
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("target table should be created");
        let path = test_csv_path("round-trip");

        // When
        export_table(&pool, "export_source", export_options(path.clone()))
            .await
            .expect("table should export");
        let summary = import_table(&pool, "import_target", import_options(path.clone()))
            .await
            .expect("table should import");

        // Then
        let rows = sqlx::query_as::<_, (i32, String, Option<String>, String)>(
            "select id, name, note, slug from import_target order by id",
        )
        .fetch_all(&pool)
        .await
        .expect("imported rows should load");
        assert_eq!(summary.rows, Some(2));
        assert_eq!(
            rows,
            [
                (1, "Alice, A".to_owned(), None, "alice, a".to_owned()),
                (
                    2,
                    "Line\nBreak".to_owned(),
                    Some(String::new()),
                    "line\nbreak".to_owned(),
                ),
            ]
        );
        remove_test_file(&path).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_import_maps_reordered_headers() {
        // Given
        let pool = live_pool().await;
        sqlx::query(
            "create temporary table reordered_target (id integer primary key, name text, active boolean default true)",
        )
        .execute(&pool)
        .await
        .expect("target table should be created");
        let path = test_csv_path("reordered");
        fs::write(&path, b"name,id\nAlice,7\n")
            .await
            .expect("fixture should be written");

        // When
        import_table(&pool, "reordered_target", import_options(path.clone()))
            .await
            .expect("CSV should import");

        // Then
        let row = sqlx::query_as::<_, (i32, String, bool)>(
            "select id, name, active from reordered_target",
        )
        .fetch_one(&pool)
        .await
        .expect("imported row should load");
        assert_eq!(row, (7, "Alice".to_owned(), true));
        remove_test_file(&path).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_import_rolls_back_all_rows_after_bad_value() {
        // Given
        let pool = live_pool().await;
        sqlx::query("create temporary table rollback_target (id integer, name text)")
            .execute(&pool)
            .await
            .expect("target table should be created");
        let path = test_csv_path("rollback");
        fs::write(&path, b"id,name\n1,valid\ninvalid,bad\n")
            .await
            .expect("fixture should be written");

        // When
        let result = import_table(&pool, "rollback_target", import_options(path.clone())).await;

        // Then
        let count: i64 = sqlx::query_scalar("select count(*) from rollback_target")
            .fetch_one(&pool)
            .await
            .expect("row count should load");
        assert!(result.is_err());
        assert_eq!(count, 0);
        remove_test_file(&path).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_query_export_writes_csv() {
        // Given
        let pool = live_pool().await;
        let path = test_csv_path("query");

        // When
        export_query(
            &pool,
            "select 'value,with,commas'::text as value",
            export_options(path.clone()),
        )
        .await
        .expect("query should export");

        // Then
        let contents = fs::read_to_string(&path)
            .await
            .expect("export should be readable");
        assert_eq!(contents, "value\n\"value,with,commas\"\n");
        remove_test_file(&path).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires DBCRAB_TEST_DATABASE_URL"]
    async fn live_backend_cancellation_preserves_session_state() {
        // Given
        let pool = live_pool().await;
        let mut connection = pool.acquire().await.expect("connection should be acquired");
        sqlx::query("create temporary table cancellation_marker (id integer)")
            .execute(&mut *connection)
            .await
            .expect("temporary table should be created");
        let backend_pid: i32 = sqlx::query_scalar("select pg_backend_pid()")
            .fetch_one(&mut *connection)
            .await
            .expect("backend pid should load");

        // When
        let query = sqlx::query("select pg_sleep(30)").execute(&mut *connection);
        let cancel = async {
            time::sleep(Duration::from_millis(100)).await;
            cancel_backend(&pool, backend_pid).await
        };
        let (query_result, cancel_result) = tokio::join!(query, cancel);

        // Then
        let marker_exists: bool = sqlx::query_scalar(
            "select pg_catalog.to_regclass('pg_temp.cancellation_marker') is not null",
        )
        .fetch_one(&mut *connection)
        .await
        .expect("session should remain usable");
        assert!(cancel_result.expect("cancellation request should run"));
        assert!(query_result.is_err());
        assert!(marker_exists);
    }

    async fn live_pool() -> PgPool {
        let url = std::env::var("DBCRAB_TEST_DATABASE_URL")
            .expect("DBCRAB_TEST_DATABASE_URL must be set for ignored live tests");
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("live PostgreSQL connection should succeed")
    }

    fn test_csv_path(label: &str) -> PathBuf {
        let id = TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("dbcrab-{label}-{}-{id}.csv", std::process::id()))
    }

    fn export_options(path: PathBuf) -> ExportOptions {
        ExportOptions {
            output: path,
            header: true,
            force: false,
            format_explicit: false,
        }
    }

    fn import_options(path: PathBuf) -> ImportOptions {
        ImportOptions {
            input: path,
            header: true,
            format_explicit: false,
        }
    }

    async fn remove_test_file(path: &Path) {
        fs::remove_file(path)
            .await
            .expect("test CSV should be removed");
    }
}
