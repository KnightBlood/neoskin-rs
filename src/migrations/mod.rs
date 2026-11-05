use sea_orm_migration::MigratorTrait;

mod m20261102_000001_create_core_tables;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
        vec![
            Box::new(m20261102_000001_create_core_tables::Migration),
            // Suprnova 的会话存储（额外表，不影响 17 张业务表）。
            Box::new(suprnova::session::migrations::CreateSessionsTable::new(
                suprnova::session::migrations::SessionUserKey::Integer,
            )),
        ]
    }
}
