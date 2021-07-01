use mcproto_rs::types::{
    BaseComponent,
    Chat,
    ColorCode,
    TextComponent,
};

use crate::commands::CommandSender;

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

pub fn format_chat_message(sender: &CommandSender, message: impl ToChat) -> Chat {
    Chat::Text(TextComponent {
        text: format!("{}", sender.name()),
        base: BaseComponent {
            bold: false,
            italic: false,
            underlined: false,
            strikethrough: false,
            obfuscated: false,
            color: Some(ColorCode::Blue),
            insertion: None,
            click_event: None,
            hover_event: None,
            extra: vec![Box::new(Chat::Text(TextComponent {
                text: ": ".into(),
                base: BaseComponent {
                    bold: false,
                    italic: false,
                    underlined: false,
                    strikethrough: false,
                    obfuscated: false,
                    color: None,
                    insertion: None,
                    click_event: None,
                    hover_event: None,
                    extra: vec![Box::new(message.to_chat())],
                },
            }))],
        },
    })
}
