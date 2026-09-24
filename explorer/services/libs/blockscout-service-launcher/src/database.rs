use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::serde_as;
use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::interval;
use tracing::log::LevelFilter;

cfg_if::cfg_if! {
    if #[cfg(feature = "database-1")] {
        pub use sea_orm_1 as sea_orm;
        pub use sea_orm_migration_1::MigratorTrait;
    } else if #[cfg(feature = "database-0_11")] {
        pub use sea_orm_0_11 as sea_orm;
        pub use sea_orm_migration_0_11::MigratorTrait;
    } else if #[cfg(feature = "database-0_10")] {
        pub use sea_orm_0_10 as sea_orm;
        pub use sea_orm_migration_0_10::MigratorTrait;
    } else {
        compile_error!(
            "one of the features ['database-1', 'database-0_11', 'database-0_10'] \
             must be enabled"
        );
    }
}
pub use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr,
    FromQueryResult, Statement,
};

const DEFAULT_DB: &str = "postgres";

/// When `settings.create_database` is true, connect to the server database and create the
/// target database if it does not already exist.
pub async fn ensure_database_exists_if_configured(
    settings: &DatabaseSettings,
) -> anyhow::Result<()> {
    if !settings.create_database {
        return Ok(());
    }

    let db_url = settings.connect.clone().url();
    let (db_base_url, db_name) = {
        let mut db_url: url::Url = db_url.parse().context("invalid database url")?;
        let db_name = db_url
            .path_segments()
            .and_then(|mut segments| segments.next())
            .ok_or(anyhow::anyhow!("missing database name"))?
            .to_string();
        db_url.set_path("");
        if db_name.is_empty() {
            Err(anyhow::anyhow!("database name is empty"))?
        }
        (db_url, db_name)
    };
    tracing::info!("creating database '{db_name}'");
    let db_base_url = format!("{db_base_url}/{DEFAULT_DB}");

    let create_database_options = settings.connect_options.apply_to(db_base_url.into());
    let db = Database::connect(create_database_options).await?;

    let result = db
        .execute(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(r#"CREATE DATABASE "{db_name}""#),
        ))
        .await;
    match result {
        Ok(_) => {
            tracing::info!("database '{db_name}' created");
        }
        Err(e) => {
            if e.to_string().contains("already exists") {
                tracing::info!("database '{db_name}' already exists");
            } else {
                return Err(anyhow::anyhow!(e));
            }
        }
    };

    Ok(())
}

/// Connect to Postgres using `settings` (including optional create-database), without running
/// SeaORM migrations.
pub async fn connect_postgres(settings: &DatabaseSettings) -> anyhow::Result<DatabaseConnection> {
    ensure_database_exists_if_configured(settings).await?;
    let db_url = settings.connect.clone().url();
    let connect_options = settings.connect_options.apply_to(db_url.into());
    Database::connect(connect_options)
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

/// Run SeaORM migrations when `settings.run_migrations` is true.
pub async fn run_sea_orm_migrations<Migrator: MigratorTrait>(
    db: &DatabaseConnection,
    settings: &DatabaseSettings,
) -> anyhow::Result<()> {
    if settings.run_migrations {
        Migrator::up(db, None).await?;
    }
    Ok(())
}

pub async fn initialize_postgres<Migrator: MigratorTrait>(
    settings: &DatabaseSettings,
) -> anyhow::Result<DatabaseConnection> {
    let db = connect_postgres(settings).await?;
    run_sea_orm_migrations::<Migrator>(&db, settings).await?;
    Ok(db)
}

pub struct ReadWriteRepo {
    main_db: DatabaseConnection,
    replica_db: Option<ReplicaRepo>,
}

async fn connect_replica_repo(settings: &ReplicaDatabaseSettings) -> anyhow::Result<ReplicaRepo> {
    let db_url = settings.connect.clone().url();
    let connect_options = settings.connect_options.apply_to(db_url.into());
    let db = Database::connect(connect_options)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let replica_db = ReplicaRepo::new(db, settings.max_lag, settings.health_check_interval);
    replica_db.spawn_health_check();
    Ok(replica_db)
}

async fn maybe_connect_replica_repo(
    settings: Option<&ReplicaDatabaseSettings>,
) -> anyhow::Result<Option<ReplicaRepo>> {
    let Some(settings) = settings else {
        return Ok(None);
    };
    let repo = connect_replica_repo(settings)
        .await
        .context("failed to connect to read replica")?;
    Ok(Some(repo))
}

impl ReadWriteRepo {
    pub async fn new<Migrator: MigratorTrait>(
        main_db_settings: &DatabaseSettings,
        replica_db_settings: Option<&ReplicaDatabaseSettings>,
    ) -> anyhow::Result<Self> {
        let main_db = initialize_postgres::<Migrator>(main_db_settings).await?;
        let replica_db = maybe_connect_replica_repo(replica_db_settings).await?;
        Ok(Self {
            main_db,
            replica_db,
        })
    }

    /// Primary plus optional read replica, same as [`Self::new`], but **no** SeaORM migrations on
    /// the primary (only [`connect_postgres`]). Use when the service runs migrations elsewhere
    /// (for example `sqlx` against the primary pool).
    pub async fn new_no_migrations(
        main_db_settings: &DatabaseSettings,
        replica_db_settings: Option<&ReplicaDatabaseSettings>,
    ) -> anyhow::Result<Self> {
        let main_db = connect_postgres(main_db_settings).await?;
        let replica_db = maybe_connect_replica_repo(replica_db_settings).await?;
        Ok(Self {
            main_db,
            replica_db,
        })
    }

    /// The main database connection supporting read and write operations.
    pub fn main_db(&self) -> &DatabaseConnection {
        &self.main_db
    }

    /// The read-only database connection, will fallback to the main database if the replica is unhealthy.
    pub fn read_db(&self) -> &DatabaseConnection {
        match &self.replica_db {
            Some(replica_db) if replica_db.is_healthy() => &replica_db.db,
            _ => &self.main_db,
        }
    }
}

#[derive(Clone)]
pub struct ReplicaRepo {
    db: DatabaseConnection,
    is_healthy: Arc<AtomicBool>,
    max_lag: Duration,
    health_check_interval: Duration,
}

impl ReplicaRepo {
    pub fn new(db: DatabaseConnection, max_lag: Duration, health_check_interval: Duration) -> Self {
        Self {
            db,
            is_healthy: Arc::new(AtomicBool::new(true)),
            max_lag,
            health_check_interval,
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::Relaxed)
    }

    pub fn spawn_health_check(&self) {
        let repo = self.clone();

        tracing::info!("starting replica db health check");
        tokio::spawn(async move {
            let mut interval = interval(repo.health_check_interval);

            loop {
                interval.tick().await;
                let is_healthy = repo.check_health().await;
                let prev_is_healthy = repo.is_healthy.swap(is_healthy, Ordering::Relaxed);
                if prev_is_healthy && !is_healthy {
                    tracing::warn!("replica db is unhealthy, fallback to write db");
                }
                if !prev_is_healthy && is_healthy {
                    tracing::info!("replica db became healthy");
                }
            }
        });
    }

    async fn check_health(&self) -> bool {
        #[derive(FromQueryResult)]
        struct HealthResult {
            is_in_recovery: bool,
            lag: i32,
        }

        let query = r#"
        SELECT pg_is_in_recovery() AS is_in_recovery,
        (EXTRACT(EPOCH FROM now()) - EXTRACT(EPOCH FROM pg_last_xact_replay_timestamp()))::int AS lag;
        "#;

        let res = HealthResult::find_by_statement(Statement::from_string(
            self.db.get_database_backend(),
            query,
        ))
        .one(&self.db)
        .await;

        // replica is considered unhealthy when :
        // 1. recovery is still in progress
        // 2. sync delay is greater than max_lag
        let is_unhealthy = match res {
            Ok(Some(r)) => r.is_in_recovery && Duration::from_secs(r.lag as u64) > self.max_lag,
            _ => true,
        };

        !is_unhealthy
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    pub connect: DatabaseConnectSettings,
    #[serde(default)]
    pub connect_options: DatabaseConnectOptionsSettings,
    #[serde(default)]
    pub create_database: bool,
    #[serde(default)]
    pub run_migrations: bool,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReplicaDatabaseSettings {
    pub connect: DatabaseConnectSettings,
    #[serde(default)]
    pub connect_options: DatabaseConnectOptionsSettings,
    #[serde(default = "default_max_lag")]
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub max_lag: Duration,
    #[serde(default = "default_health_check_interval")]
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    pub health_check_interval: Duration,
}

fn default_max_lag() -> Duration {
    Duration::from_secs(5 * 60)
}

fn default_health_check_interval() -> Duration {
    Duration::from_secs(10)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub enum DatabaseConnectSettings {
    Url(String),
    Kv(DatabaseKvConnection),
}

impl DatabaseConnectSettings {
    pub fn url(self) -> String {
        match self {
            DatabaseConnectSettings::Url(s) => s,
            DatabaseConnectSettings::Kv(kv) => kv.url(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatabaseKvConnection {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    #[serde(default)]
    pub dbname: Option<String>,
    #[serde(default)]
    pub options: Option<String>,
}

impl DatabaseKvConnection {
    pub fn url(self) -> String {
        let dbname = self
            .dbname
            .map(|dbname| format!("/{dbname}"))
            .unwrap_or_default();
        let options = self
            .options
            .map(|options| format!("?{options}"))
            .unwrap_or_default();
        format!(
            "postgresql://{}:{}@{}:{}{}{}",
            self.user, self.password, self.host, self.port, dbname, options
        )
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConnectOptionsSettings {
    /// Maximum number of connections for a pool
    pub max_connections: Option<u32>,
    /// Minimum number of connections for a pool
    pub min_connections: Option<u32>,
    #[serde_as(as = "Option<serde_with::DurationSeconds<u64>>")]
    /// The connection timeout for a packet connection
    pub connect_timeout: Option<Duration>,
    #[serde_as(as = "Option<serde_with::DurationSeconds<u64>>")]
    /// Maximum idle time for a particular connection to prevent
    /// network resource exhaustion
    pub idle_timeout: Option<Duration>,
    #[serde_as(as = "Option<serde_with::DurationSeconds<u64>>")]
    /// Set the maximum amount of time to spend waiting for acquiring a connection
    pub acquire_timeout: Option<Duration>,
    #[serde_as(as = "Option<serde_with::DurationSeconds<u64>>")]
    /// Set the maximum lifetime of individual connections
    pub max_lifetime: Option<Duration>,
    /// Enable SQLx statement logging
    pub sqlx_logging: bool,
    #[serde(
        deserialize_with = "string_to_level_filter",
        serialize_with = "level_filter_to_string"
    )]
    /// SQLx statement logging level (ignored if `sqlx_logging` is false)
    pub sqlx_logging_level: LevelFilter,
    #[cfg(feature = "database-1")]
    #[serde(
        deserialize_with = "string_to_level_filter",
        serialize_with = "level_filter_to_string"
    )]
    /// SQLx slow statements logging level (ignored if `sqlx_logging` is false)
    pub sqlx_slow_statements_logging_level: LevelFilter,
    #[cfg(feature = "database-1")]
    #[serde_as(as = "serde_with::DurationSeconds<u64>")]
    /// SQLx slow statements duration threshold (ignored if `sqlx_logging` is false)
    pub sqlx_slow_statements_logging_threshold: Duration,
    #[cfg(feature = "database-1")]
    /// Only establish connections to the DB as needed. If set to `true`, the db connection will
    /// be created using SQLx's [connect_lazy](https://docs.rs/sqlx/latest/sqlx/struct.Pool.html#method.connect_lazy)
    /// method.
    pub connect_lazy: bool,
    #[cfg(feature = "database-1")]
    /// PostgreSQL [`application_name`](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-APPLICATION-NAME)
    /// for connections in this pool (via `PgConnectOptions`).
    #[serde(default)]
    pub postgres_application_name: Option<String>,
    #[cfg(feature = "database-1")]
    /// PostgreSQL `statement_timeout` startup option (e.g. `"60s"`, `"60000"` ms). See
    /// [`PgConnectOptions::options`](https://docs.rs/sqlx/latest/sqlx/postgres/struct.PgConnectOptions.html#method.options).
    #[serde(default)]
    pub postgres_statement_timeout: Option<String>,
}

impl Default for DatabaseConnectOptionsSettings {
    fn default() -> Self {
        Self {
            max_connections: None,
            min_connections: None,
            connect_timeout: None,
            idle_timeout: None,
            acquire_timeout: None,
            max_lifetime: None,
            sqlx_logging: true,
            sqlx_logging_level: LevelFilter::Debug,
            #[cfg(feature = "database-1")]
            sqlx_slow_statements_logging_level: LevelFilter::Off,
            #[cfg(feature = "database-1")]
            sqlx_slow_statements_logging_threshold: Duration::from_secs(1),
            #[cfg(feature = "database-1")]
            connect_lazy: false,
            #[cfg(feature = "database-1")]
            postgres_application_name: None,
            #[cfg(feature = "database-1")]
            postgres_statement_timeout: None,
        }
    }
}

impl DatabaseConnectOptionsSettings {
    fn apply_to(&self, mut options: ConnectOptions) -> ConnectOptions {
        if let Some(value) = self.max_connections {
            options.max_connections(value);
        }
        if let Some(value) = self.min_connections {
            options.min_connections(value);
        }
        if let Some(value) = self.connect_timeout {
            options.connect_timeout(value);
        }
        if let Some(value) = self.idle_timeout {
            options.idle_timeout(value);
        }
        if let Some(value) = self.acquire_timeout {
            options.acquire_timeout(value);
        }
        if let Some(value) = self.max_lifetime {
            options.max_lifetime(value);
        }
        options.sqlx_logging(self.sqlx_logging);
        options.sqlx_logging_level(self.sqlx_logging_level);
        #[cfg(feature = "database-1")]
        options.sqlx_slow_statements_logging_settings(
            self.sqlx_slow_statements_logging_level,
            self.sqlx_slow_statements_logging_threshold,
        );
        #[cfg(feature = "database-1")]
        options.connect_lazy(self.connect_lazy);
        #[cfg(feature = "database-1")]
        {
            let app = self.postgres_application_name.clone();
            let st = self.postgres_statement_timeout.clone();
            if app.is_some() || st.is_some() {
                options.map_sqlx_postgres_opts(move |mut pg| {
                    if let Some(ref name) = app {
                        pg = pg.application_name(name.as_str());
                    }
                    if let Some(ref timeout) = st {
                        pg = pg.options([("statement_timeout", timeout.as_str())]);
                    }
                    pg
                });
            }
        }
        options
    }
}

fn string_to_level_filter<'de, D>(deserializer: D) -> Result<LevelFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let string = String::deserialize(deserializer)?;
    LevelFilter::from_str(&string).map_err(<D::Error as serde::de::Error>::custom)
}

pub fn level_filter_to_string<S>(x: &LevelFilter, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(x.as_str().to_lowercase().as_str())
}

#[cfg(all(test, feature = "database-1"))]
mod postgres_connect_options_tests {
    use super::{ConnectOptions, DatabaseConnectOptionsSettings};

    #[test]
    fn apply_to_accepts_postgres_session_options() {
        let settings = DatabaseConnectOptionsSettings {
            postgres_application_name: Some("launcher_test".into()),
            postgres_statement_timeout: Some("45s".into()),
            ..Default::default()
        };
        let _opts = settings.apply_to(ConnectOptions::new("postgres://localhost:5432/postgres"));
    }
}
