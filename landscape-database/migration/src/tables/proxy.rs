use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
pub enum ProxyNodeConfigs {
    #[sea_orm(iden = "proxy_node_configs")]
    Table,
    Id,
    Name,
    Enable,
    Server,
    Port,
    Protocol,
    Remark,
    UpdateAt,
}
