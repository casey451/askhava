//! The monthly test restore against a real empty cluster (T648; OPS-013, OPS-051, JOB-024;
//! TD-OPS-26, TD-JOB-06; D-492, D-3155, D-4835).
//!
//! CI's one PostgreSQL already has the roles, so each test makes a throwaway PostgreSQL 16
//! cluster with `initdb` and `pg_ctl` (found by name on `PATH` through `recovery`'s `pg_tool`,
//! PostgreSQL 16 only) in a temporary folder on a free local port, as an unprivileged user (one it
//! creates and reaches through `runuser` when the test runs as root, which `initdb` refuses),
//! points the restore's admin URL at it, and stops and removes it at the end. The source database
//! is the test's own, migrated as `askhava_migrator` the way `server migrate` does on the host.
//! Missing binaries fail the test with a message, never skip it. All data is fake.

use std::path::PathBuf;
use std::process::Command;

use askhava_jobs::api::{create, job_number};
use askhava_ops::api::Actor;
use askhava_recovery::api::{RestoreError, RestoreSettings, load_dump, pg_tool, test_restore};
use jiff::Timestamp;
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool};

/// The unprivileged user a root test run creates for `initdb` and `pg_ctl`.
const PG_USER: &str = "askhava_pgtest";

/// A throwaway PostgreSQL 16 cluster, stopped and removed when dropped.
struct Cluster {
    dir: PathBuf,
    port: u16,
    root: bool,
}

impl Cluster {
    /// Runs `initdb` and starts the server on a free port of 127.0.0.1 (no socket, trust logins).
    fn start(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("askhava_restore_{}_{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary cluster folder");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free local port")
            .port();
        let uid = Command::new("id").arg("-u").output().expect("id -u");
        let root = String::from_utf8_lossy(&uid.stdout).trim() == "0";
        let cluster = Self { dir, port, root };
        let data = cluster.dir.to_string_lossy().into_owned();
        if root {
            static USER: std::sync::Once = std::sync::Once::new();
            USER.call_once(|| {
                let made = Command::new("useradd").args(["--system", PG_USER]).output();
                made.expect("useradd");
            });
            let owned = Command::new("chown").args(["-R", PG_USER, &data]).status();
            assert!(
                owned.expect("chown").success(),
                "chown of the cluster folder"
            );
        }
        cluster.run(
            "initdb",
            &["-D", &data, "-U", "postgres", "-A", "trust", "--no-sync"],
        );
        let options =
            format!("-p {port} -c listen_addresses=127.0.0.1 -c unix_socket_directories=''");
        let log = format!("{data}/server.log");
        cluster.run(
            "pg_ctl",
            &["-D", &data, "-w", "-l", &log, "-o", &options, "start"],
        );
        cluster
    }

    /// Runs a PostgreSQL 16 `tool` as the unprivileged user and returns whether it succeeded.
    fn try_run(&self, tool: &'static str, args: &[&str]) -> bool {
        let found = pg_tool(tool);
        let path = found
            .unwrap_or_else(|e| panic!("TD-OPS-26 needs PostgreSQL 16 {tool}: {e}"))
            .path;
        let mut cmd = if self.root {
            let mut cmd = Command::new("runuser");
            cmd.args(["-u", PG_USER, "--"]).arg(path);
            cmd
        } else {
            Command::new(path)
        };
        cmd.args(args)
            .output()
            .is_ok_and(|out| out.status.success())
    }

    fn run(&self, tool: &'static str, args: &[&str]) {
        assert!(self.try_run(tool, args), "{tool} {args:?} failed");
    }

    /// The trust-login admin URL of `database` on this cluster (no password: fake, local).
    fn url(&self, database: &str) -> String {
        format!("postgres://postgres@127.0.0.1:{}/{database}", self.port)
    }

    async fn connect(&self, database: &str) -> PgConnection {
        PgConnection::connect(&self.url(database))
            .await
            .expect("cluster connection")
    }

    /// The databases the restore made and has not dropped.
    async fn leftovers(&self) -> i64 {
        let sql = "SELECT count(*) FROM pg_database WHERE datname LIKE 'askhava_restore_%'";
        let mut conn = self.connect("postgres").await;
        sqlx::query_scalar(sql)
            .fetch_one(&mut conn)
            .await
            .expect("databases")
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        let data = self.dir.to_string_lossy().into_owned();
        self.try_run("pg_ctl", &["-D", &data, "-m", "immediate", "-w", "stop"]);
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

async fn exec(pool: &PgPool, sql: String) {
    sqlx::raw_sql(AssertSqlSafe(sql))
        .execute(pool)
        .await
        .expect("statement");
}

/// Migrates the test's empty database as `askhava_migrator`, as on the host (D-092), and returns
/// the restore settings: this database as the dump's source, `cluster` as the throwaway service.
async fn source(pool: &PgPool, cluster: &Cluster) -> RestoreSettings {
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(pool)
        .await
        .expect("name");
    exec(
        pool,
        format!("GRANT CREATE ON DATABASE \"{db}\" TO askhava_migrator"),
    )
    .await;
    exec(
        pool,
        "GRANT USAGE, CREATE ON SCHEMA public TO askhava_migrator".to_owned(),
    )
    .await;
    let mut conn = pool.acquire().await.expect("connection");
    sqlx::raw_sql("SET ROLE askhava_migrator")
        .execute(&mut *conn)
        .await
        .expect("SET ROLE");
    askhava_core::api::MIGRATOR
        .run(&mut *conn)
        .await
        .expect("migrations as the migrator");
    sqlx::raw_sql("RESET ROLE")
        .execute(&mut *conn)
        .await
        .expect("RESET ROLE");
    let base = std::env::var("DATABASE_URL").expect("DATABASE_URL for #[sqlx::test]");
    let (server, _) = base.rsplit_once('/').expect("a database URL");
    RestoreSettings::new(
        format!("{server}/{db}").into(),
        cluster.url("postgres").into(),
    )
}

/// TD-OPS-26 (OPS-051, D-492): the `pg_dump -Fc` stream restored into an empty cluster without
/// `scripts/bootstrap_roles.sql` fails and leaves no object; with the bootstrap first it passes,
/// every role `NOLOGIN`; an object owned by another role, or an `UPDATE` grant to the app role on
/// an append-only table, fails the check; the restore's database is dropped every time.
#[sqlx::test]
async fn td_ops_26_a_restore_needs_the_roles_first_and_checks_owners_and_grants(pool: PgPool) {
    let cluster = Cluster::start("ops26");
    let urls = source(&pool, &cluster).await;
    let mut admin = cluster.connect("postgres").await;
    sqlx::raw_sql("CREATE DATABASE bare")
        .execute(&mut admin)
        .await
        .expect("bare database");
    assert_eq!(
        load_dump(&urls, "bare").await,
        Err(RestoreError::Restore),
        "no roles"
    );
    let objects = "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                   WHERE n.nspname !~ '^(pg_|information_schema$)'";
    let left: i64 = sqlx::query_scalar(objects)
        .fetch_one(&mut cluster.connect("bare").await)
        .await
        .expect("count");
    assert_eq!(left, 0, "--single-transaction leaves no object behind");

    assert_eq!(test_restore(&urls).await, Ok(()), "bootstrap first");
    let roles = "SELECT count(*), count(*) FILTER (WHERE rolcanlogin) FROM pg_roles \
                 WHERE rolname IN ('askhava_migrator', 'askhava_app', 'askhava_backup')";
    let roles: (i64, i64) = sqlx::query_as(roles)
        .fetch_one(&mut admin)
        .await
        .expect("roles");
    assert_eq!(
        roles,
        (3, 0),
        "the bootstrap made every role, none with a login"
    );

    let plant = "CREATE TABLE recovery.planted_fake (id int); \
                 ALTER TABLE recovery.planted_fake OWNER TO askhava_app";
    exec(&pool, plant.to_owned()).await;
    assert_eq!(test_restore(&urls).await, Err(RestoreError::Owner));
    exec(&pool, "DROP TABLE recovery.planted_fake".to_owned()).await;
    exec(
        &pool,
        "GRANT UPDATE ON ops.audit_log TO askhava_app".to_owned(),
    )
    .await;
    assert_eq!(test_restore(&urls).await, Err(RestoreError::Grant));
    assert_eq!(
        cluster.leftovers().await,
        0,
        "each restore dropped its database"
    );
}

/// TD-JOB-06 (JOB-024): the restored copy carries the job number counter, so a number texted out
/// before the export is never issued again after a restore.
#[sqlx::test]
async fn td_job_06_a_texted_job_number_is_not_reused_after_a_restore(pool: PgPool) {
    let cluster = Cluster::start("job06");
    let urls = source(&pool, &cluster).await;
    let mut conn = pool.acquire().await.expect("connection");
    let mut texted = Vec::new();
    for _ in 0..2 {
        let job = create(&mut conn, Timestamp::now(), false, Actor::System)
            .await
            .expect("job");
        texted.push(job_number(&mut conn, job).await.expect("number"));
    }
    assert_eq!(
        test_restore(&urls).await,
        Ok(()),
        "the bootstrap, then a clean restore"
    );
    let mut admin = cluster.connect("postgres").await;
    sqlx::raw_sql("CREATE DATABASE restored")
        .execute(&mut admin)
        .await
        .expect("database");
    assert_eq!(load_dump(&urls, "restored").await, Ok(()));
    let mut restored = cluster.connect("restored").await;
    let job = create(&mut restored, Timestamp::now(), false, Actor::System)
        .await
        .expect("job");
    let next = job_number(&mut restored, job).await.expect("number");
    assert_eq!(texted, [1001, 1002]);
    assert_eq!(next, 1003, "the next number is past every texted one");
}
