use std::collections::HashMap;

use crate::config::Configuration;
use crate::resend::api::{Email, EmailAddress, Tag};
use crate::resend::buffer::Buffer;
use actix_web::{error, web, HttpResponse, Responder, Result};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Recipient {
    uuid: String,
    email: String,
    name: Option<String>,
    status: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Campaign {
    uuid: String,
    name: String,
    from_email: Option<String>,
    #[serde(default)]
    headers: Vec<HashMap<String, String>>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct MessengerRequest {
    subject: String,
    body: String,
    content_type: String,
    from_email: Option<String>,
    recipients: Vec<Recipient>,
    campaign: Campaign,
}

pub async fn messenger_handler(
    email_buffer: web::Data<Buffer>,
    config: web::Data<Configuration>,
    messenger_req: web::Json<MessengerRequest>,
) -> Result<impl Responder> {
    log::info!("Received messenger request: {:?}", messenger_req);
    let from = [
        messenger_req.campaign.from_email.as_ref(),
        messenger_req.from_email.as_ref(),
        config.from_email.as_ref(),
    ]
    .into_iter()
    .flatten()
    .find(|from| !from.trim().is_empty())
    .ok_or_else(|| error::ErrorBadRequest("Missing from email"))?;
    let from_address = EmailAddress::from_string(from)
        .map_err(|_| error::ErrorBadRequest("Invalid from email"))?;

    let mut tags: Vec<Tag> = messenger_req
        .campaign
        .tags
        .iter()
        .flatten()
        .map(|tag| Tag::new(tag, "true"))
        .collect();
    tags.push(Tag::new("campaign", &messenger_req.campaign.uuid));

    let headers: HashMap<String, String> = messenger_req
        .campaign
        .headers
        .iter()
        .flatten()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    // Listmonk renders markdown to HTML before handing it over, so only "plain" is text.
    let is_plain = messenger_req.content_type == "plain";
    let body = messenger_req.body.clone();

    let emails = messenger_req
        .recipients
        .iter()
        .filter(|recipient| {
            if recipient.status == "enabled" {
                return true;
            }
            log::info!(
                "Recipient {} is not enabled, skipping",
                recipient.email.clone()
            );
            false
        })
        .map(|recipient| Email {
            from: from_address.clone(),
            to: vec![EmailAddress::from_parts(
                recipient.name.clone(),
                &recipient.email,
            )],
            reply_to: None,
            subject: messenger_req.subject.clone(),
            text: is_plain.then(|| body.clone()),
            html: (!is_plain).then(|| body.clone()),
            headers: headers.clone(),
            tags: tags.clone(),
        })
        .collect();
    email_buffer.push_all(emails).await;
    Ok(HttpResponse::Ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn config(from_email: Option<&str>) -> web::Data<Configuration> {
        let mut config = Configuration::parse_from([
            "test",
            "--resend-api-key",
            "key",
            "--listmonk-api-username",
            "user",
            "--listmonk-api-password",
            "pass",
        ]);
        config.from_email = from_email.map(str::to_string);
        web::Data::new(config)
    }

    fn request(content_type: &str, from_email: Option<&str>) -> web::Json<MessengerRequest> {
        web::Json(MessengerRequest {
            subject: "Test subject".to_string(),
            body: "<h1>Test</h1>".to_string(),
            content_type: content_type.to_string(),
            from_email: None,
            recipients: vec![
                Recipient {
                    uuid: "123".to_string(),
                    email: "test@email.com".to_string(),
                    name: None,
                    status: "enabled".to_string(),
                },
                Recipient {
                    uuid: "456".to_string(),
                    email: "test2@email.com".to_string(),
                    name: Some("Test recipient".to_string()),
                    status: "enabled".to_string(),
                },
                Recipient {
                    uuid: "156".to_string(),
                    email: "test3@email.com".to_string(),
                    name: Some("Test recipient".to_string()),
                    status: "blocklisted".to_string(),
                },
            ],
            campaign: Campaign {
                uuid: "789".to_string(),
                name: "Test campaign".to_string(),
                from_email: from_email.map(str::to_string),
                headers: vec![HashMap::from([("X-Test".to_string(), "1".to_string())])],
                tags: Some(vec!["my tag".to_string()]),
            },
        })
    }

    #[actix_rt::test]
    async fn test_messenger_handler() {
        let email_buffer = web::Data::new(Buffer::new());
        messenger_handler(
            email_buffer.clone(),
            config(None),
            request("richtext", Some("from@email.com")),
        )
        .await
        .unwrap();
        let emails = email_buffer.pop_all().await;
        assert_eq!(emails.len(), 2);
        assert_eq!(
            emails[0].from,
            EmailAddress::from_string("from@email.com").expect("Invalid from email")
        );

        assert_eq!(emails[0].to.len(), 1);
        assert_eq!(
            emails[0].to[0],
            EmailAddress::from_parts(None, "test@email.com")
        );
        assert_eq!(emails[0].reply_to, None);
        assert_eq!(emails[0].subject, "Test subject".to_string());
        assert_eq!(emails[0].text, None);
        assert_eq!(emails[0].html, Some("<h1>Test</h1>".to_string()));
        assert_eq!(emails[0].headers.get("X-Test"), Some(&"1".to_string()));
        assert_eq!(
            emails[0].tags,
            vec![Tag::new("my_tag", "true"), Tag::new("campaign", "789")]
        );
    }

    #[actix_rt::test]
    async fn test_plain_content_is_sent_as_text() {
        let email_buffer = web::Data::new(Buffer::new());
        messenger_handler(
            email_buffer.clone(),
            config(None),
            request("plain", Some("from@email.com")),
        )
        .await
        .unwrap();
        let emails = email_buffer.pop_all().await;
        assert_eq!(emails[0].html, None);
        assert_eq!(emails[0].text, Some("<h1>Test</h1>".to_string()));
    }

    #[actix_rt::test]
    async fn test_from_falls_back_to_config() {
        let email_buffer = web::Data::new(Buffer::new());
        messenger_handler(
            email_buffer.clone(),
            config(Some("Default <default@email.com>")),
            request("richtext", None),
        )
        .await
        .unwrap();
        let emails = email_buffer.pop_all().await;
        assert_eq!(emails[0].from.email(), "default@email.com");
    }

    #[actix_rt::test]
    async fn test_missing_from_is_rejected() {
        let email_buffer = web::Data::new(Buffer::new());
        let result = messenger_handler(email_buffer.clone(), config(None), request("richtext", None)).await;
        assert!(result.is_err());
        assert!(email_buffer.pop_all().await.is_empty());
    }
}
