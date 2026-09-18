
use std::time::Duration;

const USER_AGENT: &str = concat!("Ports-Launcher/", env!("CARGO_PKG_VERSION"));

pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).user_agent(USER_AGENT).build().into()
}

pub fn api_agent() -> ureq::Agent {
    agent(Duration::from_secs(30))
}
