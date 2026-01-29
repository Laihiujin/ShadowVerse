use rand::Rng;

pub fn gen_random_string(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..62);
            if idx < 10 {
                (b'0' + idx) as char
            } else if idx < 36 {
                (b'a' + idx - 10) as char
            } else {
                (b'A' + idx - 36) as char
            }
        })
        .collect()
}

pub fn gen_random_numeric(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| (b'0' + rng.random_range(0..10)) as char)
        .collect()
}

pub fn gen_tiktok_verify_fp() -> String {
    let part1 = gen_random_string(32); // Simplified version of verify_fp generation
    format!("verify_{}", part1)
}
