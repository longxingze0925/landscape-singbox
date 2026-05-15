use crate::repository::UpdateActiveModel;
use landscape_common::proxy::ProxyNodeConfig;
use sea_orm::{entity::prelude::*, ActiveValue::Set};
use serde::{Deserialize, Serialize};

use crate::{DBId, DBJson, DBTimestamp};

pub type ProxyNodeConfigModel = Model;
pub type ProxyNodeConfigEntity = Entity;
pub type ProxyNodeConfigActiveModel = ActiveModel;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "proxy_node_configs")]
#[cfg_attr(feature = "postgres", sea_orm(schema_name = "public"))]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: DBId,
    pub name: String,
    pub enable: bool,
    pub server: String,
    pub port: u16,
    pub protocol: DBJson,
    pub remark: String,
    pub update_at: DBTimestamp,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if insert && self.id.is_not_set() {
            self.id = Set(Uuid::new_v4());
        }
        Ok(self)
    }
}

impl From<Model> for ProxyNodeConfig {
    fn from(entity: Model) -> Self {
        ProxyNodeConfig {
            id: entity.id,
            name: entity.name,
            enable: entity.enable,
            server: entity.server,
            port: entity.port,
            protocol: serde_json::from_value(entity.protocol).unwrap(),
            remark: entity.remark,
            update_at: entity.update_at,
        }
    }
}

impl Into<ActiveModel> for ProxyNodeConfig {
    fn into(self) -> ActiveModel {
        let mut active = ActiveModel { id: Set(self.id), ..Default::default() };
        self.update(&mut active);
        active
    }
}

impl UpdateActiveModel<ActiveModel> for ProxyNodeConfig {
    fn update(self, active: &mut ActiveModel) {
        active.name = Set(self.name);
        active.enable = Set(self.enable);
        active.server = Set(self.server);
        active.port = Set(self.port);
        active.protocol = Set(serde_json::to_value(self.protocol).unwrap().into());
        active.remark = Set(self.remark);
        active.update_at = Set(self.update_at);
    }
}
