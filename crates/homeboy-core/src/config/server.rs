use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_port() -> u16 {
    22
}

impl ServerConfig {
    pub fn keychain_service_name(&self) -> String {
        format!("com.extrachill.homeboy.ssh.{}", self.id)
    }

    pub fn is_valid(&self) -> bool {
        !self.host.is_empty() && !self.user.is_empty()
    }

    pub fn generate_id(host: &str) -> String {
        format!("server-{}", host.replace('.', "-"))
    }
}
