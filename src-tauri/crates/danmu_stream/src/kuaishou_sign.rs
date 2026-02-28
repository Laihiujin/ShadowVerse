// Kuaishou signature generation for guest mode danmu support
// Based on reverse engineering of Kuaishou API
// Reference: https://github.com/HackAppSign/kuaishou-sign

use rand::Rng;

/// Generate Kuaishou guest signature for danmu API requests
pub struct KuaishouSign {
    did: String,
}

impl KuaishouSign {
    pub fn new(did: &str) -> Self {
        Self {
            did: did.to_string(),
        }
    }

    /// Generate sign parameter for danmu requests
    /// url_params: The query parameters without '?' 
    pub fn generate_sign(&self, url_params: &str) -> String {
        // Use md5::compute like other modules in this project
        let combined = format!("{}{}", url_params, self.did);
        format!("{:x}", md5::compute(combined.as_bytes()))
    }

    /// Add necessary parameters for guest mode danmu request
    pub fn enhance_params(&self, mut params: Vec<(String, String)>) -> Vec<(String, String)> {
        // Ensure did exists
        if !params.iter().any(|(k, _)| k == "did") {
            params.push(("did".to_string(), self.did.clone()));
        }
        
        // Add kpn if not exists
        if !params.iter().any(|(k, _)| k == "kpn") {
            params.push(("kpn".to_string(), "GAME_ZONE".to_string()));
        }
        
        params
    }
}

/// Generate Kuaishou web DID (Device ID) for guest mode  
/// Format: web_<32 hex characters>
#[allow(dead_code)]
pub fn gen_kuaishou_web_did() -> String {
    let mut rng = rand::rng();
    let hex: String = (0..32)
        .map(|_| format!("{:x}", rng.random::<u8>() % 16))
        .collect();
    format!("web_{}", hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_did_generation() {
        let did = gen_kuaishou_web_did();
        assert!(did.starts_with("web_"));
        assert_eq!(did.len(), 36); // "web_" + 32 hex chars
    }

    #[test]
    fn test_sign_generation() {
        let did = "web_test123";
        let signer = KuaishouSign::new(did);
        let sign = signer.generate_sign("test=123");
        assert!(!sign.is_empty());
        // MD5 should produce 32 hex characters
        assert_eq!(sign.len(), 32);
    }
}
