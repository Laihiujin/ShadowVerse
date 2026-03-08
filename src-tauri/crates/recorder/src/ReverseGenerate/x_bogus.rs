use md5;

pub struct XBogus {
    user_agent: String,
}

impl XBogus {
    pub fn new(user_agent: &str) -> Self {
        Self {
            user_agent: user_agent.to_string(),
        }
    }

    fn md5_str_to_array(&self, md5_str: &str) -> Vec<u8> {
        if md5_str.len() > 32 {
            return md5_str.as_bytes().to_vec();
        }
        let mut array = Vec::new();
        let mut idx = 0;
        while idx + 1 < md5_str.len() {
            let high = md5_str.as_bytes()[idx] as char;
            let low = md5_str.as_bytes()[idx + 1] as char;
            let hi = high.to_digit(16).unwrap_or(0) as u8;
            let lo = low.to_digit(16).unwrap_or(0) as u8;
            array.push((hi << 4) | lo);
            idx += 2;
        }
        array
    }

    fn md5_bytes(&self, input: &[u8]) -> String {
        format!("{:x}", md5::compute(input))
    }

    fn md5(&self, input: &str) -> String {
        let array = self.md5_str_to_array(input);
        self.md5_bytes(&array)
    }

    fn md5_encrypt(&self, url_path: &str) -> Vec<u8> {
        let first = self.md5(url_path);
        let second = self.md5_str_to_array(&first);
        let third = self.md5_str_to_array(&self.md5_bytes(&second));
        third
    }

    fn rc4_encrypt(&self, key: &[u8], data: &[u8]) -> Vec<u8> {
        let mut s: Vec<u8> = (0..=255).collect();
        let mut j = 0u8;
        for i in 0..256usize {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
        let mut i = 0u8;
        let mut j2 = 0u8;
        let mut output = Vec::with_capacity(data.len());
        for byte in data {
            i = i.wrapping_add(1);
            j2 = j2.wrapping_add(s[i as usize]);
            s.swap(i as usize, j2 as usize);
            let idx = s[i as usize].wrapping_add(s[j2 as usize]);
            let k = s[idx as usize];
            output.push(byte ^ k);
        }
        output
    }

    fn calculation(&self, a1: u8, a2: u8, a3: u8) -> String {
        let x3 = ((a1 as u32) << 16) | ((a2 as u32) << 8) | (a3 as u32);
        let table = b"Dkdpgh4ZKsQB80/Mfvw36XI1R25-WUAlEi7NLboqYTOPuzmFjJnryx9HVGcaStCe=";
        let mut out = String::with_capacity(4);
        out.push(table[((x3 & 16515072) >> 18) as usize] as char);
        out.push(table[((x3 & 258048) >> 12) as usize] as char);
        out.push(table[((x3 & 4032) >> 6) as usize] as char);
        out.push(table[(x3 & 63) as usize] as char);
        out
    }

    fn encoding_conversion(values: &[u8]) -> Vec<u8> {
        let a = values[0];
        let b = values[1];
        let c = values[2];
        let e = values[3];
        let d = values[4];
        let t = values[5];
        let f = values[6];
        let r = values[7];
        let n = values[8];
        let o = values[9];
        let i = values[10];
        let underscore = values[11];
        let x = values[12];
        let u = values[13];
        let s = values[14];
        let l = values[15];
        let v = values[16];
        let h = values[17];
        let p = values[18];

        vec![
            a, i, b, underscore, c, x, e, u, d, s, t, l, f, v, r, h, n, p, o,
        ]
    }

    pub fn get_x_bogus(&self, url_path: &str) -> String {
        let params = url_path.replace("?", "");
        let md5 = self.md5_encrypt(&params);
        let mut data = vec![64, 0, 1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        data.append(&mut md5.clone());
        data.append(&mut md5.clone());
        data.push(1);

        let rc4_key = self.md5_encrypt(&self.user_agent);
        let encrypted = self.rc4_encrypt(&rc4_key, &data);
        let converted = Self::encoding_conversion(&encrypted);
        let mut encoded = String::new();
        let mut idx = 0;
        while idx + 2 < converted.len() {
            let a1 = converted[idx];
            let a2 = converted[idx + 1];
            let a3 = converted[idx + 2];
            encoded.push_str(&self.calculation(a1, a2, a3));
            idx += 3;
        }
        encoded
    }
}
