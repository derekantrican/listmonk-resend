use crate::{
    config::Configuration,
    listmonk::api::{BounceType, ListmonkAPI, ListmonkBounce},
    resend::{api::EmailAddress, signature},
};

use actix_web::{web, HttpRequest, HttpResponse, Responder, Result};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
pub struct Bounce {
    #[serde(rename = "type")]
    bounce_type: Option<String>,
    #[serde(rename = "subType")]
    sub_type: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct EmailData {
    email_id: String,
    #[serde(default)]
    to: Vec<String>,
    /// Object map (`{"name": "value"}`); an array of `{name, value}` is accepted as well.
    tags: Option<Value>,
    bounce: Option<Bounce>,
}

#[derive(Deserialize, Debug)]
pub struct WebhookRequest {
    #[serde(rename = "type")]
    request_type: String,
    data: EmailData,
}

impl EmailData {
    fn tag(&self, name: &str) -> Option<String> {
        match self.tags.as_ref()? {
            Value::Object(map) => map.get(name)?.as_str().map(str::to_string),
            Value::Array(items) => items
                .iter()
                .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
                .and_then(|item| item.get("value")?.as_str().map(str::to_string)),
            _ => None,
        }
    }
}

fn header<'a>(req: &'a HttpRequest, name: &str) -> &'a str {
    req.headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

fn verify_signature(req: &HttpRequest, secret: &str, body: &[u8]) -> bool {
    let now = chrono::Utc::now().timestamp();
    match signature::verify(
        secret,
        header(req, "svix-id"),
        header(req, "svix-timestamp"),
        header(req, "svix-signature"),
        body,
        now,
    ) {
        Ok(_) => true,
        Err(err) => {
            log::warn!("Rejecting webhook request, invalid signature: {}", err);
            false
        }
    }
}

pub async fn webhook_handler(
    req: HttpRequest,
    listmonk_api: web::Data<ListmonkAPI>,
    config: web::Data<Configuration>,
    body: web::Bytes,
) -> Result<impl Responder> {
    if let Some(secret) = &config.resend_webhook_secret {
        if !verify_signature(&req, secret, &body) {
            return Ok(HttpResponse::Unauthorized().body("Invalid signature"));
        }
    }
    let payload: WebhookRequest = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(err) => {
            log::warn!("Ignoring unparsable webhook request: {}", err);
            return Ok(HttpResponse::BadRequest().body("Invalid payload"));
        }
    };
    log::info!("Received webhook request: {:?}", payload);
    match payload.request_type.as_str() {
        "email.bounced" => handle_bounce(listmonk_api, payload).await,
        "email.complained" => handle_spam_complaint(listmonk_api, payload).await,
        _ => {
            log::info!("Ignoring webhook request");
            Ok(HttpResponse::Ok().body("OK"))
        }
    }
}

async fn handle_spam_complaint(
    listmonk_api: web::Data<ListmonkAPI>,
    payload: WebhookRequest,
) -> Result<HttpResponse> {
    let mut failed = false;
    for recipient in &payload.data.to {
        let address = match EmailAddress::from_string(recipient) {
            Ok(address) => address,
            Err(_) => {
                log::warn!("Skipping invalid recipient address {}", recipient);
                continue;
            }
        };
        match listmonk_api.blocklist_by_email(address).await {
            Ok(_) => log::info!("Successfully blocklisted recipient"),
            Err(e) => {
                log::error!("Failed to blocklist recipient: {}", e);
                failed = true;
            }
        }
    }
    Ok(response(failed))
}

/// Permanent bounces are hard; transient (and undetermined) ones are soft.
fn bounce_type(bounce: Option<&Bounce>) -> BounceType {
    match bounce
        .and_then(|b| b.bounce_type.as_deref())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("permanent") => BounceType::Hard,
        _ => BounceType::Soft,
    }
}

async fn handle_bounce(
    listmonk_api: web::Data<ListmonkAPI>,
    payload: WebhookRequest,
) -> Result<HttpResponse> {
    let data = &payload.data;
    let campaign_uuid = data.tag("campaign");
    let mut failed = false;
    for recipient in &data.to {
        let address = match EmailAddress::from_string(recipient) {
            Ok(address) => address,
            Err(_) => {
                log::warn!("Skipping invalid recipient address {}", recipient);
                continue;
            }
        };
        let mut bounce = ListmonkBounce::new(address.email(), bounce_type(data.bounce.as_ref()))
            .with_meta(&data.email_id);
        if let Some(campaign_uuid) = &campaign_uuid {
            bounce = bounce.with_campaign_uuid(campaign_uuid);
        }
        match listmonk_api.record_bounce(bounce).await {
            Ok(_) => log::info!("Successfully recorded bounce event"),
            Err(e) => {
                log::error!("Failed to record bounce: {}", e);
                failed = true;
            }
        }
    }
    Ok(response(failed))
}

fn response(failed: bool) -> HttpResponse {
    if failed {
        HttpResponse::InternalServerError().body("Internal Server Error")
    } else {
        HttpResponse::Ok().body("OK")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNCE: &str = include_str!("../../test/req_bounce.json");

    #[test]
    fn test_parse_bounce() {
        let payload: WebhookRequest = serde_json::from_str(BOUNCE).unwrap();
        assert_eq!(payload.request_type, "email.bounced");
        assert_eq!(payload.data.to, vec!["delivered@resend.dev".to_string()]);
        assert_eq!(
            payload.data.tag("campaign"),
            Some("2e7e4b51-f31b-418a-a120-e41800cb689f".to_string())
        );
        assert!(matches!(
            bounce_type(payload.data.bounce.as_ref()),
            BounceType::Hard
        ));
    }

    #[test]
    fn test_tags_as_array() {
        let data: EmailData = serde_json::from_str(
            r#"{"email_id":"1","to":["a@b.co"],"tags":[{"name":"campaign","value":"abc"}]}"#,
        )
        .unwrap();
        assert_eq!(data.tag("campaign"), Some("abc".to_string()));
        assert_eq!(data.tag("other"), None);
    }

    #[test]
    fn test_transient_is_soft() {
        let bounce = Bounce {
            bounce_type: Some("Transient".to_string()),
            sub_type: None,
            message: None,
        };
        assert!(matches!(bounce_type(Some(&bounce)), BounceType::Soft));
        assert!(matches!(bounce_type(None), BounceType::Soft));
    }
}
