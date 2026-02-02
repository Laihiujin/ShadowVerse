#[derive(Debug, Clone, Default)]
pub struct Account {
    pub platform: String,
    pub id: String,
    pub name: String,
    pub avatar: String,
    pub csrf: String,
    pub cookies: String,
}

impl Account {
    pub fn is_guest(&self) -> bool {
        self.id.starts_with("guest:") || self.id.starts_with("cookie_")
    }
}
