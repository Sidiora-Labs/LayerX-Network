pub use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{Statement, TransactionTrait};

mod m20220101_000001_create_table;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20220101_000001_create_table::Migration)]
    }
}

// https://discord.com/channels/873880840487206962/900758376164757555/1050378980181671936
fn split(s: &str) -> Vec<&str> {
    let mut data = vec![];
    let mut start = 0;
    let mut inside = false;
    let mut del_len = 0;
    for (i, c) in s.chars().enumerate() {
        if c == '$' {
            if del_len == 0 {
                del_len += 1;
            } else if del_len == 1 {
                del_len = 0;
            } else if del_len == 2 {
                inside = !inside;
                del_len = 0;
            }
        } else if c == '_' {
            if del_len == 1 {
                del_len += 1;
            } else {
                del_len = 0;
            }
        } else {
            del_len = 0;
            if c == ';' && !inside {
                data.push(&s[start..i + 1]);
                start = i + 1;
            }
        }
    }
    data
}

pub async fn from_sql(manager: &SchemaManager<'_>, content: &str) -> Result<(), DbErr> {
    let filtered = content
        .lines()
        .filter(|line| !line.starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    let stmnts = split(&filtered);
    let txn = manager.get_connection().begin().await?;
    for st in stmnts.into_iter() {
        txn.execute(Statement::from_string(
            manager.get_database_backend(),
            st.to_string(),
        ))
        .await
        .map_err(|e| DbErr::Migration(format!("{e}\nQuery: {st}")))?;
    }
    txn.commit().await
}
