pub trait Authenticator {
    fn authenticate(&self, token: &str) -> bool;
}
pub struct TokenAuth {
    pub secret: String,
}
impl Authenticator for TokenAuth {
    fn authenticate(&self, token: &str) -> bool {
        token == self.secret
    }
}
impl TokenAuth {
    pub fn new(secret: impl Into<String>) -> Self {
        Self { secret: secret.into() }
    }
}
pub fn extra_helper() -> &'static str { "added-by-incremental" }
