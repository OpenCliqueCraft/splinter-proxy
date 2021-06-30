use mcproto_rs::types::Chat;

pub trait ToChat {
    fn to_chat(&self) -> Chat;
}
impl ToChat for Chat {
    fn to_chat(&self) -> Chat {
        self.clone()
    }
}
impl ToChat for &str {
    fn to_chat(&self) -> Chat {
        Chat::from_text(self)
    }
}
impl ToChat for String {
    fn to_chat(&self) -> Chat {
        Chat::from_text(self.as_str())
    }
}
