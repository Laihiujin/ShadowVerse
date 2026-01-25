use reqwest::Client;

pub fn no_proxy_client() -> Client {
    Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|_| Client::new())
}
