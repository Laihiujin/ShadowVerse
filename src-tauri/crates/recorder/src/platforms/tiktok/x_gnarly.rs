use md5;
use rand::Rng;
use std::time::{SystemTime, UNIX_EPOCH};

const AA: [u64; 93] = [
    4294967295,
    138,
    1498001188,
    211147047,
    253,
    0,
    203,
    288,
    9,
    1196819126,
    3212677781,
    135,
    263,
    193,
    58,
    18,
    244,
    2931180889,
    240,
    173,
    268,
    2157053261,
    261,
    175,
    14,
    5,
    171,
    270,
    156,
    258,
    13,
    15,
    3732962506,
    185,
    169,
    2,
    6,
    132,
    162,
    200,
    3,
    160,
    217618912,
    62,
    2517678443,
    44,
    164,
    4,
    96,
    183,
    2903579748,
    3863347763,
    119,
    181,
    10,
    190,
    8,
    2654435769,
    259,
    104,
    230,
    128,
    2633865432,
    225,
    1,
    257,
    143,
    179,
    16,
    600974999,
    185100057,
    32,
    188,
    53,
    2718276124,
    177,
    196,
    4294967296,
    147,
    117,
    17,
    49,
    7,
    28,
    12,
    266,
    216,
    11,
    0,
    45,
    166,
    247,
    1451689750,
];

const OT: [u32; 4] = [AA[9] as u32, AA[69] as u32, AA[51] as u32, AA[92] as u32];
const CUSTOM_B64_ALPHABET: &[u8; 64] =
    b"u09tbS3UvgDEe6r-ZVMXzLpsAohTn7mdINQlW412GqBjfYiyk8JORCF5/xKHwacP";

#[derive(Clone, Copy)]
struct GnarlyPrng {
    state: [u32; 16],
    pos: usize,
}

impl GnarlyPrng {
    fn new(now_ms: u64) -> Self {
        let mut rng = rand::rng();
        let rand_max = AA[77];
        let rand_u32 = |rng: &mut rand::rngs::ThreadRng| -> u32 {
            if rand_max >= (u32::MAX as u64 + 1) {
                rng.random::<u32>()
            } else {
                rng.random_range(0..rand_max as u32)
            }
        };
        let state = [
            AA[44] as u32,
            AA[74] as u32,
            AA[10] as u32,
            AA[62] as u32,
            AA[42] as u32,
            AA[17] as u32,
            AA[2] as u32,
            AA[21] as u32,
            AA[3] as u32,
            AA[70] as u32,
            AA[50] as u32,
            AA[32] as u32,
            (AA[0] as u32) & (now_ms as u32),
            rand_u32(&mut rng),
            rand_u32(&mut rng),
            rand_u32(&mut rng),
        ];
        Self {
            state,
            pos: AA[88] as usize,
        }
    }

    fn rand_f64(&mut self) -> f64 {
        let block = chacha_block(&self.state, 8);
        let t = block[self.pos] as u64;
        let r = ((block[self.pos + 8] & 0xFFFFFFF0) >> 11) as u64;
        if self.pos == 7 {
            let mut state = self.state;
            bump_counter(&mut state);
            self.state = state;
            self.pos = 0;
        } else {
            self.pos += 1;
        }
        (t as f64 + (r as f64) * 4294967296.0) / 2_f64.powi(53)
    }
}

fn u32_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

fn rotl(x: u32, n: u32) -> u32 {
    x.rotate_left(n)
}

fn quarter(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    state[a] = u32_add(state[a], state[b]);
    state[d] = rotl(state[d] ^ state[a], 16);
    state[c] = u32_add(state[c], state[d]);
    state[b] = rotl(state[b] ^ state[c], 12);
    state[a] = u32_add(state[a], state[b]);
    state[d] = rotl(state[d] ^ state[a], 8);
    state[c] = u32_add(state[c], state[d]);
    state[b] = rotl(state[b] ^ state[c], 7);
}

fn bump_counter(state: &mut [u32; 16]) {
    state[12] = state[12].wrapping_add(1);
}

fn chacha_block(state: &[u32; 16], rounds: u32) -> [u32; 16] {
    let mut w = *state;
    let mut r = 0;
    while r < rounds {
        quarter(&mut w, 0, 4, 8, 12);
        quarter(&mut w, 1, 5, 9, 13);
        quarter(&mut w, 2, 6, 10, 14);
        quarter(&mut w, 3, 7, 11, 15);
        r += 1;
        if r >= rounds {
            break;
        }
        quarter(&mut w, 0, 5, 10, 15);
        quarter(&mut w, 1, 6, 11, 12);
        quarter(&mut w, 2, 7, 12, 13);
        quarter(&mut w, 3, 4, 13, 14);
        r += 1;
    }
    for i in 0..16 {
        w[i] = u32_add(w[i], state[i]);
    }
    w
}

fn num_to_bytes(val: u32) -> Vec<u8> {
    if val < 255 * 255 {
        vec![((val >> 8) & 0xFF) as u8, (val & 0xFF) as u8]
    } else {
        vec![
            ((val >> 24) & 0xFF) as u8,
            ((val >> 16) & 0xFF) as u8,
            ((val >> 8) & 0xFF) as u8,
            (val & 0xFF) as u8,
        ]
    }
}

fn be_int_from_str(value: &str) -> u32 {
    let mut acc: u32 = 0;
    for b in value.as_bytes().iter().take(4) {
        acc = (acc << 8) | (*b as u32);
    }
    acc
}

fn md5_hex(data: &str) -> String {
    format!("{:x}", md5::compute(data.as_bytes()))
}

fn encrypt_chacha(mut state: [u32; 16], rounds: u32, bytes: &[u8]) -> Vec<u8> {
    let n_full = bytes.len() / 4;
    let leftover = bytes.len() % 4;
    let mut words = vec![0u32; if leftover == 0 { n_full } else { n_full + 1 }];

    for i in 0..n_full {
        let j = i * 4;
        words[i] = (bytes[j] as u32)
            | ((bytes[j + 1] as u32) << 8)
            | ((bytes[j + 2] as u32) << 16)
            | ((bytes[j + 3] as u32) << 24);
    }
    if leftover != 0 {
        let base = n_full * 4;
        let mut v = 0u32;
        for c in 0..leftover {
            v |= (bytes[base + c] as u32) << (8 * c);
        }
        words[n_full] = v;
    }

    let mut offset = 0usize;
    while offset + 16 < words.len() {
        let stream = chacha_block(&state, rounds);
        bump_counter(&mut state);
        for k in 0..16 {
            words[offset + k] ^= stream[k];
        }
        offset += 16;
    }
    let remain = words.len() - offset;
    let stream = chacha_block(&state, rounds);
    for k in 0..remain {
        words[offset + k] ^= stream[k];
    }

    let mut out = vec![0u8; bytes.len()];
    for i in 0..n_full {
        let w = words[i];
        let j = i * 4;
        out[j] = (w & 0xFF) as u8;
        out[j + 1] = ((w >> 8) & 0xFF) as u8;
        out[j + 2] = ((w >> 16) & 0xFF) as u8;
        out[j + 3] = ((w >> 24) & 0xFF) as u8;
    }
    if leftover != 0 {
        let w = words[n_full];
        let base = n_full * 4;
        for c in 0..leftover {
            out[base + c] = ((w >> (8 * c)) & 0xFF) as u8;
        }
    }
    out
}

fn ab22(key_words: &[u32; 12], rounds: u32, bytes: &[u8]) -> Vec<u8> {
    let mut state = [0u32; 16];
    state[0] = OT[0];
    state[1] = OT[1];
    state[2] = OT[2];
    state[3] = OT[3];
    state[4..].copy_from_slice(key_words);
    encrypt_chacha(state, rounds, bytes)
}

fn custom_b64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    let full_len = (bytes.len() / 3) * 3;
    let mut i = 0usize;
    while i < full_len {
        let block =
            ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
        out.push(CUSTOM_B64_ALPHABET[((block >> 18) & 63) as usize] as char);
        out.push(CUSTOM_B64_ALPHABET[((block >> 12) & 63) as usize] as char);
        out.push(CUSTOM_B64_ALPHABET[((block >> 6) & 63) as usize] as char);
        out.push(CUSTOM_B64_ALPHABET[(block & 63) as usize] as char);
        i += 3;
    }
    out
}

pub fn sign(query: &str, body: &str, user_agent: &str) -> String {
    sign_with(query, body, user_agent, 0, "5.1.1", None)
}

pub fn sign_with(
    query: &str,
    body: &str,
    user_agent: &str,
    envcode: u32,
    version: &str,
    timestamp_ms: Option<u64>,
) -> String {
    let now_ms = timestamp_ms.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    });
    let now_sec = (now_ms / 1000) as u32;
    let mut entries: Vec<(u8, Value)> = Vec::new();
    entries.push((1, Value::Number(1)));
    entries.push((2, Value::Number(envcode)));
    entries.push((3, Value::String(md5_hex(query))));
    entries.push((4, Value::String(md5_hex(body))));
    entries.push((5, Value::String(md5_hex(user_agent))));
    entries.push((6, Value::Number(now_sec)));
    entries.push((7, Value::Number(1508145731)));
    entries.push((8, Value::Number(((now_ms * 1000) % 2147483648) as u32)));
    entries.push((9, Value::String(version.to_string())));

    if version == "5.1.1" {
        entries.push((10, Value::String("1.0.0.314".to_string())));
        entries.push((11, Value::Number(1)));
        let mut v12 = 0u32;
        for key in 1u8..=11u8 {
            if let Some(value) = find_value(&entries, key) {
                let to_xor = match value {
                    Value::Number(num) => *num,
                    Value::String(text) => be_int_from_str(text),
                };
                v12 ^= to_xor;
            }
        }
        entries.push((12, Value::Number(v12)));
    }

    let mut v0 = 0u32;
    let size = entries.len() as u8;
    for key in 1u8..=size {
        if let Some(Value::Number(num)) = find_value(&entries, key) {
            v0 ^= num;
        }
    }
    entries.push((0, Value::Number(v0)));

    let mut payload: Vec<u8> = Vec::new();
    payload.push(entries.len() as u8);
    for (key, value) in &entries {
        payload.push(*key);
        let val_bytes = match value {
            Value::Number(num) => num_to_bytes(*num),
            Value::String(text) => text.as_bytes().to_vec(),
        };
        let len_bytes = num_to_bytes(val_bytes.len() as u32);
        payload.extend_from_slice(&len_bytes);
        payload.extend_from_slice(&val_bytes);
    }

    let mut prng = GnarlyPrng::new(now_ms);
    let mut key_words = [0u32; 12];
    let mut key_bytes: Vec<u8> = Vec::with_capacity(48);
    let mut round_accum = 0u32;
    for i in 0..12 {
        let rnd = prng.rand_f64();
        let word = (rnd * 4294967296.0).floor() as u32;
        key_words[i] = word;
        round_accum = (round_accum + (word & 15)) & 15;
        key_bytes.push((word & 0xFF) as u8);
        key_bytes.push(((word >> 8) & 0xFF) as u8);
        key_bytes.push(((word >> 16) & 0xFF) as u8);
        key_bytes.push(((word >> 24) & 0xFF) as u8);
    }
    let rounds = round_accum + 5;

    let enc_bytes = ab22(&key_words, rounds, &payload);

    let mut insert_pos = 0usize;
    let enc_len = enc_bytes.len();
    for b in &key_bytes {
        insert_pos = (insert_pos + *b as usize) % (enc_len + 1);
    }
    for b in &enc_bytes {
        insert_pos = (insert_pos + *b as usize) % (enc_len + 1);
    }

    let mut final_bytes = Vec::with_capacity(1 + enc_len + key_bytes.len());
    final_bytes.push(((1u8 << 6) ^ (1u8 << 3) ^ 3u8) & 0xFF);
    final_bytes.extend_from_slice(&enc_bytes[..insert_pos]);
    final_bytes.extend_from_slice(&key_bytes);
    final_bytes.extend_from_slice(&enc_bytes[insert_pos..]);

    custom_b64_encode(&final_bytes)
}

fn find_value<'a>(entries: &'a [(u8, Value)], key: u8) -> Option<&'a Value> {
    entries.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

enum Value {
    Number(u32),
    String(String),
}
