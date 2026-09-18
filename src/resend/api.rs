use super::throttler::Throttler;
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::Client;
use serde::{Serialize, Serializer};
use std::{collections::HashMap, sync::Arc};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Resend rejects batches larger than this.
pub const MAX_BATCH_SIZE: usize = 100;

lazy_static! {
    static ref RAW_EMAIL_REGEX: Regex =
        Regex::new(r"(?<mailbox>[^><\s@]+)@(?<domain>([^><\s@.,]+\.)+[^><\s@.,]{2,})").unwrap();
    static ref EMAIL_REGEX: Regex =
        Regex::new(r"(?<name>[^<]*)?<(?<mailbox>[^\s@]+)@(?<domain>([^\s@.,]+\.)+[^\s@.,]{2,})>",)
            .unwrap();
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmailAddress {
    name: Option<String>,
    email: String,
}

impl EmailAddress {
    pub fn from_parts(name: Option<String>, email: &str) -> Self {
        let name = name.map_or(String::new(), |x| x.trim().to_string());
        return EmailAddress {
            name: if name.len() == 0 { None } else { Some(name) },
            email: email.to_string(),
        };
    }

    pub fn from_string(input: &str) -> Result<Self> {
        match EMAIL_REGEX
            .captures(input)
            .or_else(|| RAW_EMAIL_REGEX.captures(input))
        {
            None => Err("Invalid email address".into()),
            Some(groups) => {
                let name = groups
                    .name("name")
                    .map_or(None, |x| Some(x.as_str().trim()))
                    .map(|x| x.trim_matches('"'))
                    .filter(|x| !x.is_empty())
                    .map(|x| x.to_string());
                let mailbox = groups.name("mailbox").unwrap().as_str();
                let domain = groups.name("domain").unwrap().as_str();
                Ok(EmailAddress {
                    name,
                    email: format!("{}@{}", mailbox, domain),
                })
            }
        }
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    /// Resend expects addresses as `email` or `Name <email>` strings.
    pub fn to_resend_string(&self) -> String {
        match &self.name {
            None => self.email.clone(),
            Some(name) => format!(
                "\"{}\" <{}>",
                name.replace('\\', "\\\\").replace('"', "\\\""),
                self.email
            ),
        }
    }
}

impl Serialize for EmailAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_resend_string())
    }
}

/// Resend tag. Name and value may only contain ASCII letters, numbers, `_` and `-`
/// (max 256 characters).
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Tag {
    pub name: String,
    pub value: String,
}

impl Tag {
    pub fn new(name: &str, value: &str) -> Self {
        Tag {
            name: Self::sanitize(name),
            value: Self::sanitize(value),
        }
    }

    fn sanitize(input: &str) -> String {
        input
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(256)
            .collect()
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Email {
    pub from: EmailAddress,
    pub to: Vec<EmailAddress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<EmailAddress>,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
}

#[derive(Clone)]
pub struct ResendAPI {
    http_client: Client,
    api_endpoint: String,
    api_key: String,
    throttler: Arc<Throttler>,
}

impl ResendAPI {
    pub fn new(api_endpoint: &str, api_key: &str, req_per_sec: u32) -> Self {
        ResendAPI {
            http_client: Client::new(),
            api_endpoint: api_endpoint.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            throttler: Arc::new(Throttler::new(req_per_sec)),
        }
    }

    pub async fn send_bulk(&self, emails: Vec<Email>, bulk_size: usize) -> Result<()> {
        let bulk_size = bulk_size.clamp(1, MAX_BATCH_SIZE);
        log::info!("Sending {} emails in bulk", emails.len());
        let chunks: Vec<&[Email]> = emails.chunks(bulk_size).collect();
        log::info!("Split emails list into {} chunks", chunks.len());
        let mut errors = String::new();
        for chunk in chunks {
            if let Err(err) = self.send_bulk_chunk(chunk).await {
                log::error!("{}", err);
                errors.push_str(&format!("{}\n", err));
            }
        }
        log::info!("All Resend API requests finished");
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.into())
        }
    }

    async fn send_bulk_chunk(&self, emails: &[Email]) -> Result<()> {
        self.throttler.wait().await;
        log::info!("Sending {} emails in chunk", emails.len());
        let res = self
            .http_client
            .post(format!("{}/emails/batch", self.api_endpoint))
            .bearer_auth(&self.api_key)
            .json(emails)
            .send()
            .await
            .map_err(|err| format!("Resend API request failed: {}", err))?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            return Err(format!("Resend API response: {} {}", status, body).into());
        }
        log::info!("Resend API response: {}", status);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_address_from_string() {
        let email = EmailAddress::from_string("John Doe <john_doe@mail.com>").unwrap();
        assert_eq!(email.name, Some("John Doe".to_string()));
        assert_eq!(email.email, "john_doe@mail.com".to_string());
    }

    #[test]
    fn test_email_address_from_string_no_name() {
        let email = EmailAddress::from_string("john_doe@mail.com").unwrap();
        assert_eq!(email.name, None);
        assert_eq!(email.email, "john_doe@mail.com".to_string());
    }

    #[test]
    fn test_email_address_from_parts() {
        let email = EmailAddress::from_parts(Some("John Doe".to_string()), "john_doe@mail.com");
        assert_eq!(email.name, Some("John Doe".to_string()));
        assert_eq!(email.email, "john_doe@mail.com".to_string());
    }

    #[test]
    fn test_email_address_from_parts_no_name() {
        let email = EmailAddress::from_parts(None, "john_doe@mail.com");
        assert_eq!(email.name, None);
        assert_eq!(email.email, "john_doe@mail.com".to_string());
    }

    #[test]
    fn test_invalid_email_address_from_string() {
        let error = EmailAddress::from_string("not-an-email").unwrap_err();
        assert_eq!(error.to_string(), "Invalid email address");
    }

    #[test]
    fn test_invalid_email_with_name() {
        let error = EmailAddress::from_string("John Doe <not-an-email>").unwrap_err();
        assert_eq!(error.to_string(), "Invalid email address");
    }

    #[test]
    fn test_email_address_to_resend_string() {
        let plain = EmailAddress::from_parts(None, "a@mail.com");
        assert_eq!(plain.to_resend_string(), "a@mail.com");
        let named = EmailAddress::from_parts(Some("John \"JD\" Doe".to_string()), "a@mail.com");
        assert_eq!(
            named.to_resend_string(),
            "\"John \\\"JD\\\" Doe\" <a@mail.com>"
        );
    }

    #[test]
    fn test_tag_sanitize() {
        let tag = Tag::new("campaign", "2e7e4b51-f31b-418a-a120-e41800cb689f");
        assert_eq!(tag.value, "2e7e4b51-f31b-418a-a120-e41800cb689f");
        assert_eq!(Tag::new("my tag:1", "x").name, "my_tag_1");
    }

    #[test]
    fn test_email_serialization() {
        let email = Email {
            from: EmailAddress::from_string("News <news@mail.com>").unwrap(),
            to: vec![EmailAddress::from_parts(None, "a@mail.com")],
            reply_to: None,
            subject: "Hi".to_string(),
            text: None,
            html: Some("<b>hi</b>".to_string()),
            headers: HashMap::new(),
            tags: vec![Tag::new("campaign", "abc")],
        };
        let json = serde_json::to_value(&email).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "from": "\"News\" <news@mail.com>",
                "to": ["a@mail.com"],
                "subject": "Hi",
                "html": "<b>hi</b>",
                "tags": [{"name": "campaign", "value": "abc"}]
            })
        );
    }
}
