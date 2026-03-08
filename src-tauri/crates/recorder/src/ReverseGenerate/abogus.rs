use chrono::Utc;
use rand::Rng;
use sm3::{Digest, Sm3};

const END_STRING: &str = "cus";
const UA_CODE: [u8; 32] = [
    76, 98, 15, 131, 97, 245, 224, 133, 122, 199, 241, 166, 79, 34, 90, 191, 128, 126, 122, 98, 66,
    11, 14, 40, 49, 110, 110, 173, 67, 96, 138, 252,
];

const STR_S4: &str = "Dkdpgh2ZmsQB80/MfvV36XI1R45-WUAlEixNLwoqYTOPuzKFjJnry79HbGcaStCe";

#[derive(Clone, Debug)]
pub struct ABogus {
    browser_len: usize,
    browser_code: Vec<u8>,
}

impl ABogus {
    pub fn new(platform: Option<&str>) -> Self {
        let browser = platform
            .map(Self::generate_browser_info)
            .unwrap_or_else(|| {
                "1536|742|1536|864|0|0|0|0|1536|864|1536|864|1536|742|24|24|MacIntel".to_string()
            });
        let browser_len = browser.len();
        let browser_code = browser.bytes().collect();
        Self {
            browser_len,
            browser_code,
        }
    }

    pub fn get_value(params: &str) -> String {
        let bogus = Self::new(None);
        bogus.generate_value(params)
    }

    fn generate_value(&self, params: &str) -> String {
        let string_1 = self.generate_string_1();
        let string_2 = self.generate_string_2(params);
        let mut merged = Vec::with_capacity(string_1.len() + string_2.len());
        merged.extend_from_slice(&string_1);
        merged.extend_from_slice(&string_2);
        Self::generate_result(&merged)
    }

    fn generate_string_1(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(12);
        output.extend_from_slice(&Self::list_1(None));
        output.extend_from_slice(&Self::list_2(None));
        output.extend_from_slice(&Self::list_3(None));
        output
    }

    fn generate_string_2(&self, params: &str) -> Vec<u8> {
        let list = self.generate_string_2_list(params, "GET", 0, 0);
        let end_check = Self::end_check_num(&list);
        let mut data = Vec::with_capacity(list.len() + self.browser_code.len() + 1);
        data.extend_from_slice(&list);
        data.extend_from_slice(&self.browser_code);
        data.push(end_check);
        Self::rc4_encrypt(&data, b"y")
    }

    fn generate_string_2_list(
        &self,
        params: &str,
        method: &str,
        start_time: u64,
        end_time: u64,
    ) -> Vec<u8> {
        let mut rng = rand::rng();
        let start_time = if start_time == 0 {
            Utc::now().timestamp_millis() as u64
        } else {
            start_time
        };
        let end_time = if end_time == 0 {
            start_time + rng.random_range(4..=8)
        } else {
            end_time
        };
        let params_array = self.generate_params_code(params);
        let method_array = self.generate_method_code(method);
        Self::list_4(
            ((end_time >> 24) & 255) as u8,
            params_array[21],
            UA_CODE[23],
            ((end_time >> 16) & 255) as u8,
            params_array[22],
            UA_CODE[24],
            ((end_time >> 8) & 255) as u8,
            ((end_time >> 0) & 255) as u8,
            ((start_time >> 24) & 255) as u8,
            ((start_time >> 16) & 255) as u8,
            ((start_time >> 8) & 255) as u8,
            ((start_time >> 0) & 255) as u8,
            method_array[21],
            method_array[22],
            ((end_time / 256 / 256 / 256 / 256) & 255) as u8,
            ((start_time / 256 / 256 / 256 / 256) & 255) as u8,
            self.browser_len as u8,
        )
    }

    fn generate_method_code(&self, method: &str) -> Vec<u8> {
        let mut joined = Vec::with_capacity(method.len() + END_STRING.len());
        joined.extend_from_slice(method.as_bytes());
        joined.extend_from_slice(END_STRING.as_bytes());
        let first = Self::sm3_to_array(&joined);
        Self::sm3_to_array(&first)
    }

    fn generate_params_code(&self, params: &str) -> Vec<u8> {
        let mut joined = Vec::with_capacity(params.len() + END_STRING.len());
        joined.extend_from_slice(params.as_bytes());
        joined.extend_from_slice(END_STRING.as_bytes());
        let first = Self::sm3_to_array(&joined);
        Self::sm3_to_array(&first)
    }

    fn sm3_to_array(data: &[u8]) -> Vec<u8> {
        let mut hasher = Sm3::new();
        hasher.update(data);
        let digest = hasher.finalize();
        digest.to_vec()
    }

    fn list_1(random_num: Option<f64>) -> [u8; 4] {
        Self::random_list(random_num, 170, 85, 1, 2, 5, 45 & 170)
    }

    fn list_2(random_num: Option<f64>) -> [u8; 4] {
        Self::random_list(random_num, 170, 85, 1, 0, 0, 0)
    }

    fn list_3(random_num: Option<f64>) -> [u8; 4] {
        Self::random_list(random_num, 170, 85, 1, 0, 5, 0)
    }

    fn random_list(random_num: Option<f64>, a: u8, b: u8, d: u8, e: u8, f: u8, g: u8) -> [u8; 4] {
        let r = random_num.unwrap_or_else(|| rand::random::<f64>() * 10000.0);
        let r_int = r as u32;
        let v1 = (r_int & 255) as u8;
        let v2 = ((r_int >> 8) & 255) as u8;
        let s1 = (v1 & a) | d;
        let s2 = (v1 & b) | e;
        let s3 = (v2 & a) | f;
        let s4 = (v2 & b) | g;
        [s1, s2, s3, s4]
    }

    fn list_4(
        a: u8,
        b: u8,
        c: u8,
        d: u8,
        e: u8,
        f: u8,
        g: u8,
        h: u8,
        i: u8,
        j: u8,
        k: u8,
        m: u8,
        n: u8,
        o: u8,
        p: u8,
        q: u8,
        r: u8,
    ) -> Vec<u8> {
        vec![
            44, a, 0, 0, 0, 0, 24, b, n, 0, c, d, 0, 0, 0, 1, 0, 239, e, o, f, g, 0, 0, 0, 0, h, 0,
            0, 14, i, j, 0, k, m, 3, p, 1, q, 1, r, 0, 0, 0,
        ]
    }

    fn end_check_num(values: &[u8]) -> u8 {
        values.iter().fold(0u8, |acc, item| acc ^ item)
    }

    fn generate_result(data: &[u8]) -> String {
        let alphabet = STR_S4.as_bytes();
        let mut output = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let b0 = data[i] as u32;
            let b1 = if i + 1 < data.len() {
                data[i + 1] as u32
            } else {
                0
            };
            let b2 = if i + 2 < data.len() {
                data[i + 2] as u32
            } else {
                0
            };
            let n = (b0 << 16) | (b1 << 8) | b2;

            for (shift, mask) in [(18, 0xFC0000), (12, 0x03F000), (6, 0x0FC0), (0, 0x3F)] {
                if shift == 6 && i + 1 >= data.len() {
                    break;
                }
                if shift == 0 && i + 2 >= data.len() {
                    break;
                }
                let idx = ((n & mask) >> shift) as usize;
                output.push(alphabet[idx]);
            }
            i += 3;
        }

        let pad_len = (4 - output.len() % 4) % 4;
        for _ in 0..pad_len {
            output.push(b'=');
        }
        String::from_utf8_lossy(&output).to_string()
    }

    fn rc4_encrypt(input: &[u8], key: &[u8]) -> Vec<u8> {
        let mut s: Vec<u8> = (0..=255u8).collect();
        let mut j: usize = 0;

        for i in 0..256 {
            j = (j + s[i] as usize + key[i % key.len()] as usize) % 256;
            s.swap(i, j);
        }

        let mut i: usize = 0;
        j = 0;
        let mut output = Vec::with_capacity(input.len());
        for byte in input {
            i = (i + 1) % 256;
            j = (j + s[i] as usize) % 256;
            s.swap(i, j);
            let t = (s[i] as usize + s[j] as usize) % 256;
            output.push(byte ^ s[t]);
        }
        output
    }

    fn generate_browser_info(platform: &str) -> String {
        let mut rng = rand::rng();
        let inner_width = rng.random_range(1280..=1920);
        let inner_height = rng.random_range(720..=1080);
        let outer_width = rng.random_range(inner_width..=1920);
        let outer_height = rng.random_range(inner_height..=1080);
        let screen_x = 0;
        let screen_y = if rng.random_bool(0.5) { 0 } else { 30 };
        let values = [
            inner_width,
            inner_height,
            outer_width,
            outer_height,
            screen_x,
            screen_y,
            0,
            0,
            outer_width,
            outer_height,
            outer_width,
            outer_height,
            inner_width,
            inner_height,
            24,
            24,
        ];
        let mut parts = values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        parts.push(platform.to_string());
        parts.join("|")
    }
}

pub fn encode_abogus(params: &str) -> String {
    let value = ABogus::get_value(params);
    urlencoding::encode(&value).into_owned()
}
