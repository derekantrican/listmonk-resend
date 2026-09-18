use std::sync::Arc;

use futures::lock::Mutex;

use super::api::Email;

#[derive(Clone)]
pub struct Buffer {
    emails: Arc<Mutex<Vec<Email>>>,
}

impl Buffer {
    pub fn new() -> Self {
        Buffer {
            emails: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn push_all(&self, emails_vec: Vec<Email>) {
        let mut emails = self.emails.lock().await;
        emails.extend(emails_vec);
    }

    pub async fn pop_all(&self) -> Vec<Email> {
        let mut emails = self.emails.lock().await;
        let mut result = Vec::new();
        std::mem::swap(&mut result, &mut emails);
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::resend::api::{EmailAddress, Tag};
    use std::collections::HashMap;

    use super::*;

    fn email(to: &str) -> Email {
        Email {
            from: EmailAddress::from_parts(None, "testemail@email.com"),
            to: vec![EmailAddress::from_parts(None, to)],
            reply_to: None,
            subject: "Test subject".to_string(),
            text: None,
            html: Some("<h1>Test</h1>".to_string()),
            headers: HashMap::new(),
            tags: vec![Tag::new("test", "test")],
        }
    }

    #[actix_rt::test]
    async fn test_buffer() {
        let buffer = Buffer::new();
        buffer
            .push_all(vec![email("recipient@email.com"), email("recipient2@email.com")])
            .await;
        let popped_emails = buffer.pop_all().await;
        assert_eq!(popped_emails.len(), 2);
        assert!(buffer.pop_all().await.is_empty());
    }
}
