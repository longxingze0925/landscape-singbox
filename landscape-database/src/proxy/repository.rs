use landscape_common::proxy::ProxyNodeConfig;
use sea_orm::DatabaseConnection;

use super::entity::{ProxyNodeConfigActiveModel, ProxyNodeConfigEntity, ProxyNodeConfigModel};
use crate::DBId;

#[derive(Clone)]
pub struct ProxyNodeRepository {
    db: DatabaseConnection,
}

impl ProxyNodeRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

crate::impl_repository!(
    ProxyNodeRepository,
    ProxyNodeConfigModel,
    ProxyNodeConfigEntity,
    ProxyNodeConfigActiveModel,
    ProxyNodeConfig,
    DBId
);
