use landscape_common::{error::LdError, geo::GeoIpSourceConfig};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::DBId;

use super::entity::{
    Column, GeoIpSourceConfigActiveModel, GeoIpSourceConfigEntity, GeoIpSourceConfigModel,
};

#[derive(Clone)]
pub struct GeoIpSourceConfigRepository {
    db: DatabaseConnection,
}

impl GeoIpSourceConfigRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn query_by_name(
        &self,
        name: Option<String>,
    ) -> Result<Vec<GeoIpSourceConfig>, LdError> {
        let mut query = GeoIpSourceConfigEntity::find();
        if let Some(name) = name {
            query = query.filter(Column::Name.eq(name));
        }

        let result = query.order_by_desc(Column::UpdateAt).all(&self.db).await?;
        Ok(result.into_iter().map(From::from).collect())
    }
}

crate::impl_repository!(
    GeoIpSourceConfigRepository,
    GeoIpSourceConfigModel,
    GeoIpSourceConfigEntity,
    GeoIpSourceConfigActiveModel,
    GeoIpSourceConfig,
    DBId
);
