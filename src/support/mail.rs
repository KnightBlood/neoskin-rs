//! 邮件发送（复用 Suprnova `Mail`，读取 `MAIL_DRIVER` 等环境变量）。

use suprnova::FrameworkError;

/// PHP `config('mail.default') != ''`：`MAIL_DRIVER` 为空时视为未启用。
pub fn enabled() -> bool {
    std::env::var("MAIL_DRIVER")
        .map(|driver| !driver.trim().is_empty())
        .unwrap_or(true)
}

/// 发送 HTML 邮件。`MAIL_DRIVER=log` 时由框架写入日志。
pub async fn send_html(
    to: &str,
    subject: &str,
    html: impl Into<String>,
) -> Result<(), FrameworkError> {
    let to = to.to_string();
    let subject = subject.to_string();
    suprnova::Mail::html(html, move |builder| builder.to(to).subject(subject)).await
}
