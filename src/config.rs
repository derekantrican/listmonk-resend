use clap::Parser;
use log;

#[derive(Parser, Debug, Clone)]
pub struct Configuration {
    #[arg(long, short = 'H', env, default_value_t = String::from("127.0.0.1"), help="Service bind IP address")]
    pub host: String,

    #[arg(
        long,
        short = 'p',
        env,
        default_value_t = 9000,
        help = "Service bind port"
    )]
    pub port: u16,

    #[arg(long, short = 'c', env, help = "Outgoing cron schedule", default_value_t = String::from("0 */1 * * * * *"))]
    pub outgoing_cron: String,

    #[arg(long, short = 'l', env, default_value_t = log::Level::Info, help="Log level")]
    pub log_level: log::Level,

    #[arg(long, short = 'e', env, help = "Resend API endpoint", default_value_t = String::from("https://api.resend.com"))]
    pub resend_api_endpoint: String,

    #[arg(long, short = 't', env, help = "Resend API key")]
    pub resend_api_key: String,

    #[arg(
        long,
        env,
        help = "Sender used when listmonk does not provide a from address"
    )]
    pub from_email: Option<String>,

    #[arg(long, short = 'm', env, help = "Listmonk API endpoint", default_value_t = String::from("http://localhost:9001"))]
    pub listmonk_api_endpoint: String,

    #[arg(long, short = 'u', env, help = "Listmonk API username")]
    pub listmonk_api_username: String,

    #[arg(long, short = 'w', env, help = "Listmonk API password")]
    pub listmonk_api_password: String,

    #[arg(
        long,
        short = 'b',
        env,
        help = "Resend API batch size (Resend accepts at most 100)",
        default_value_t = 100
    )]
    pub api_email_bulk_size: usize,

    #[arg(
        long,
        short = 'r',
        env,
        help = "Resend API requests per second",
        default_value_t = 2
    )]
    pub api_req_per_sec: u32,

    #[arg(
        long,
        short = 's',
        env,
        help = "Resend webhook signing secret (whsec_...); webhooks are not verified when unset"
    )]
    pub resend_webhook_secret: Option<String>,
}
