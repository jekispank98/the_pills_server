use super::sendmail::send_email;
use crate::config::SmtpConfig;

type MailResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;


pub async fn send_verification_email(
    smtp: &SmtpConfig,
    verification_link: &str,
    to_email: &str,
    username: &str,
) -> MailResult {
    let subject = "Email Verification";
    let template_path = "src/mail/templates/Verification-email.html";
    let placeholders = vec![
        ("{{username}}".to_string(), username.to_string()),
        ("{{verification_link}}".to_string(), verification_link.to_string()),
    ];

    send_email(smtp, to_email, subject, template_path, &placeholders).await
}

pub async fn send_welcome_email(smtp: &SmtpConfig, to_email: &str, username: &str) -> MailResult {
    let subject = "Welcome to Application";
    let template_path = "src/mail/templates/Welcome-email.html";
    let placeholders = vec![("{{username}}".to_string(), username.to_string())];

    send_email(smtp, to_email, subject, template_path, &placeholders).await
}

pub async fn send_forgot_password_email(
    smtp: &SmtpConfig,
    to_email: &str,
    reset_link: &str,
    username: &str,
) -> MailResult {
    let subject = "Reset your Password";
    let template_path = "src/mail/templates/RestPassword-email.html";
    let placeholders = vec![
        ("{{username}}".to_string(), username.to_string()),
        ("{{rest_link}}".to_string(), reset_link.to_string()),
    ];

    send_email(smtp, to_email, subject, template_path, &placeholders).await
}
