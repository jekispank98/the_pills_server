use crate::config::SmtpConfig;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{SinglePart, header},
    transport::smtp::authentication::Credentials,
};
use std::fs;

pub async fn send_email(
    smtp: &SmtpConfig,
    to_email: &str,
    subject: &str,
    template_path: &str,
    placeholders: &[(String, String)],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut html_template = fs::read_to_string(template_path)?;

    for (key, value) in placeholders {
        html_template = html_template.replace(key, value);
    }

    let email = Message::builder()
        .from(smtp.from_address.parse()?)
        .to(to_email.parse()?)
        .subject(subject)
        .singlepart(
            SinglePart::builder()
                .header(header::ContentType::TEXT_HTML)
                .body(html_template),
        )?;

    let creds = Credentials::new(smtp.username.clone(), smtp.password.clone());
    let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.server)?
        .credentials(creds)
        .port(smtp.port)
        .build();

    mailer.send(email).await?;

    Ok(())
}
