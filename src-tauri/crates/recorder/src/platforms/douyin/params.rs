use chrono::Utc;
use rand::Rng;

const MS_TOKEN_BASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-";

pub fn gen_verify_fp() -> String {
    let base_str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::rng();
    let now_ms = Utc::now().timestamp_millis() as u64;
    let mut base36 = String::new();
    let mut value = now_ms;
    while value > 0 {
        let remainder = (value % 36) as u8;
        let ch = if remainder < 10 {
            (b'0' + remainder) as char
        } else {
            (b'a' + remainder - 10) as char
        };
        base36.insert(0, ch);
        value /= 36;
    }

    let mut chars = vec!['\0'; 36];
    chars[8] = '_';
    chars[13] = '_';
    chars[18] = '_';
    chars[23] = '_';
    chars[14] = '4';

    let base_len = base_str.len();
    for i in 0..36 {
        if chars[i] != '\0' {
            continue;
        }
        let mut idx = rng.random_range(0..base_len);
        if i == 19 {
            idx = 3 & idx;
        }
        chars[i] = base_str.chars().nth(idx).unwrap_or('0');
    }

    format!(
        "verify_{}_{}",
        base36,
        chars.into_iter().collect::<String>()
    )
}

pub fn gen_false_ms_token() -> String {
    let mut rng = rand::rng();
    let mut output = String::with_capacity(128);
    for _ in 0..126 {
        let idx = rng.random_range(0..MS_TOKEN_BASE.len());
        output.push(MS_TOKEN_BASE.chars().nth(idx).unwrap_or('A'));
    }
    output.push_str("==");
    output
}
