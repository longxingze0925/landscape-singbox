use landscape_common::{proxy::ProxyNodeConfig, service::controller::ConfigController};
use landscape_database::{
    provider::LandscapeDBServiceProvider, proxy::repository::ProxyNodeRepository,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct ProxyNodeService {
    store: ProxyNodeRepository,
}

impl ProxyNodeService {
    pub async fn new(store: LandscapeDBServiceProvider) -> Self {
        let store = store.proxy_node_store();
        Self { store }
    }
}

#[async_trait::async_trait]
impl ConfigController for ProxyNodeService {
    type Id = Uuid;
    type Config = ProxyNodeConfig;
    type DatabseAction = ProxyNodeRepository;

    fn get_repository(&self) -> &Self::DatabseAction {
        &self.store
    }
}
