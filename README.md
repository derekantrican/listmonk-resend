# listmonk-resend
Listmonk Messenger for [Resend](https://resend.com) integration.

The service receives listmonk's [HTTP postback](https://listmonk.app/docs/messengers/) messages, buffers them and
sends them through Resend's [batch API](https://resend.com/docs/api-reference/emails/send-batch-emails). Resend
bounce and spam-complaint webhooks are translated back into listmonk bounces / blocklisting.

## Setup

1. Run the service (see `.env.example` for the configuration, every option can also be passed as a CLI flag):
   `docker run --env-file .env -p 9000:9000 ghcr.io/<owner>/listmonk-resend`
2. In listmonk, *Settings -> Messengers*, add a messenger with URL `http://<service>:9000/api/messenger`. Then select
   it as the messenger of a campaign (`email` keeps using SMTP).
3. In Resend, *Webhooks*, add `https://<service>/webhooks/service/resend` for the `email.bounced` and
   `email.complained` events. Copy the signing secret (`whsec_...`) into `RESEND_WEBHOOK_SECRET`; requests are
   rejected when the signature does not match. Without a secret the webhooks are not verified.
4. Enable bounce processing in listmonk (*Settings -> Bounces*, enable the bounce webhook) and give the
   `LISTMONK_API_USERNAME` user the `webhooks:post_bounce` and subscriber management permissions.

## Behaviour

- Messages are collected for `OUTGOING_CRON` (default: every minute), then sent in batches of at most 100 emails
  (`API_EMAIL_BULK_SIZE`), at most `API_REQ_PER_SEC` requests per second (Resend's default limit is 2).
- Sender: `campaign.from_email`, then listmonk's `from_email`, then `FROM_EMAIL`.
- `plain` campaigns are sent as text, everything else as HTML. Attachments are not supported (the Resend batch API
  does not accept them).
- Each email is tagged `campaign=<uuid>` (used to attribute bounces) and campaign tags are sent as `<tag>=true`,
  with characters other than letters, digits, `_` and `-` replaced by `_` as required by Resend.
- `Permanent` bounces are recorded as hard bounces, everything else as soft bounces.
- Failed batches are logged and dropped; they are not retried.

## Credits

This project is a modified version of [listmonk-mailersend](https://github.com/tokav/listmonk-mailersend), adapted to use Resend instead of MailerSend. The overall design (listmonk
postback receiver, email buffer with a scheduled batch job, bounce webhook handling) comes from that project, and it is
used under its MIT license (see `LICENSE`).
