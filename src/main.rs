mod config;
mod listmonk;
mod resend;

use actix_jobs::{run_forever, Scheduler};
use actix_web::{web, App, HttpServer};
use clap::Parser;
use config::Configuration;
use dotenv;
use listmonk::api::ListmonkAPI;
use resend::{api::ResendAPI, buffer::Buffer, job::OutgoingEmailsJob};
use std::io;

#[actix_web::main]
async fn main() -> io::Result<()> {
    dotenv::dotenv().ok();
    let config = Configuration::parse();
    simple_logger::init_with_level(config.log_level).unwrap();

    let shared_email_buffer = Buffer::new();
    let resend_api = ResendAPI::new(
        &config.resend_api_endpoint,
        &config.resend_api_key,
        config.api_req_per_sec,
    );
    if config.resend_webhook_secret.is_none() {
        log::warn!("RESEND_WEBHOOK_SECRET is not set, webhook requests will not be verified");
    }

    let mut scheduler = Scheduler::new();
    scheduler.add(Box::new(OutgoingEmailsJob::new(
        &config.outgoing_cron,
        resend_api,
        shared_email_buffer.clone(),
        config.api_email_bulk_size,
    )));
    log::info!("Starting scheduler");
    run_forever(scheduler);

    let listmonk_api = ListmonkAPI::new(
        &config.listmonk_api_endpoint,
        &config.listmonk_api_username,
        &config.listmonk_api_password,
    );
    let host = config.host.clone();
    let port = config.port;
    log::info!("Starting server on {}:{}", host, port);
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(shared_email_buffer.clone()))
            .app_data(web::Data::new(listmonk_api.clone()))
            .app_data(web::Data::new(config.clone()))
            .route(
                "/api/messenger",
                web::post().to(listmonk::rest::messenger_handler),
            )
            .route(
                "/webhooks/service/resend",
                web::post().to(resend::rest::webhook_handler),
            )
    })
    .bind((host, port))?
    .run()
    .await
    .unwrap();
    Ok(())
}
