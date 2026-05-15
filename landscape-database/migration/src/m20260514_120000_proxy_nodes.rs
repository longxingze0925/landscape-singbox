use sea_orm_migration::prelude::*;

use crate::tables::proxy::ProxyNodeConfigs;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ProxyNodeConfigs::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ProxyNodeConfigs::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(ProxyNodeConfigs::Name).string().not_null())
                    .col(
                        ColumnDef::new(ProxyNodeConfigs::Enable).boolean().not_null().default(true),
                    )
                    .col(ColumnDef::new(ProxyNodeConfigs::Server).string().not_null())
                    .col(ColumnDef::new(ProxyNodeConfigs::Port).unsigned().not_null())
                    .col(ColumnDef::new(ProxyNodeConfigs::Protocol).json().not_null())
                    .col(ColumnDef::new(ProxyNodeConfigs::Remark).string().not_null())
                    .col(
                        ColumnDef::new(ProxyNodeConfigs::UpdateAt).double().not_null().default(0.0),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ProxyNodeConfigs::Table).to_owned()).await?;
        Ok(())
    }
}
