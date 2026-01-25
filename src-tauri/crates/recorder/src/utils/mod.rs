pub mod user_agent_generator;

pub fn no_proxy_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

pub fn jitter_interval_secs(base: u64, jitter: u64) -> u64 {
    if jitter == 0 {
        return base;
    }
    let min = std::cmp::max(1, base.saturating_sub(jitter));
    let max = base.saturating_add(jitter);
    if max <= min {
        return min;
    }
    min + rand::random::<u64>() % (max - min + 1)
}
