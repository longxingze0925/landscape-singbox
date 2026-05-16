use landscape_common::{error::LdError, geo::GeoSiteSourceConfig};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::DBId;

use super::entity::{Column, GeoSiteConfigActiveModel, GeoSiteConfigEntity, GeoSiteConfigModel};

#[derive(Clone)]
pub struct GeoSiteConfigRepository {
    db: DatabaseConnection,
}

impl GeoSiteConfigRepository {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    pub async fn query_by_name(
        &self,
        name: Option<String>,
    ) -> Result<Vec<GeoSiteSourceConfig>, LdError> {
        let mut query = GeoSiteConfigEntity::find();
        if let Some(name) = name {
            query = query.filter(Column::Name.eq(name));
        }

        let result = query.order_by_desc(Column::UpdateAt).all(&self.db).await?;
        Ok(result.into_iter().map(From::from).collect())
    }
}

crate::impl_repository!(
    GeoSiteConfigRepository,
    GeoSiteConfigModel,
    GeoSiteConfigEntity,
    GeoSiteConfigActiveModel,
    GeoSiteSourceConfig,
    DBId
);
