use std::fmt::Write as _;

use super::class::{DetailLevel, NotificationClass};
use super::event::{AgentId, Event, JourneyOutcome, Money, NotificationId};

const MINIMAL_TITLE: &str = "notification.minimal.title";
const MINIMAL_BODY: &str = "notification.minimal.body";
const MINIMAL_LOCK_SCREEN: &str = "notification.minimal.lock-screen";

pub(crate) struct Content {
    pub class: NotificationClass,
    pub lock_screen_copy_key: String,
    pub payload_title_copy_key: String,
    pub payload_body_copy_key: String,
    pub action_copy_key: Option<String>,
    pub deep_link: String,
    pub payload_agent: Option<AgentId>,
    pub payload_money: Option<Money>,
    pub email_subject: String,
    pub email_lines: Vec<String>,
}

pub(crate) fn build(event: &Event, detail: DetailLevel) -> Content {
    let class = event.class();
    let title_copy_key = format!("notification.{class}.title");
    let body_copy_key = body_copy_key(event);
    let lock_screen_copy_key = format!("notification.{class}.lock-screen");
    let (payload_title_copy_key, payload_body_copy_key, payload_lock_screen) = match detail {
        DetailLevel::Minimal => (
            MINIMAL_TITLE.to_owned(),
            MINIMAL_BODY.to_owned(),
            MINIMAL_LOCK_SCREEN.to_owned(),
        ),
        DetailLevel::Full | DetailLevel::Summary => (
            title_copy_key.clone(),
            body_copy_key.clone(),
            lock_screen_copy_key.clone(),
        ),
    };
    let money = event_money(event);
    let agent = event_agent(event);
    let payload_agent = matches!(detail, DetailLevel::Full).then(|| agent).flatten();
    let payload_money = match detail {
        DetailLevel::Full => money.clone(),
        DetailLevel::Summary | DetailLevel::Minimal => None,
    };
    Content {
        class,
        lock_screen_copy_key: payload_lock_screen,
        payload_title_copy_key,
        payload_body_copy_key,
        action_copy_key: action_copy_key(class),
        deep_link: deep_link(event),
        payload_agent: payload_agent.clone(),
        payload_money: payload_money.clone(),
        email_subject: email_subject(event, detail),
        email_lines: email_lines(
            event,
            detail,
            payload_agent.as_ref(),
            payload_money.as_ref(),
        ),
    }
}

fn event_agent(event: &Event) -> Option<AgentId> {
    match event {
        Event::ApprovalWaiting { agent_id, .. } => Some(agent_id.clone()),
        Event::MoneyArrived { .. }
        | Event::JourneyFinished { .. }
        | Event::ClaimReady { .. }
        | Event::SecurityNewDevice { .. }
        | Event::SecurityRecovery { .. }
        | Event::SecurityWalletRebinding { .. }
        | Event::SecurityKeyRotation { .. }
        | Event::ServiceStatus { .. } => None,
    }
}

fn body_copy_key(event: &Event) -> String {
    match event {
        Event::JourneyFinished { outcome, .. } => {
            format!("notification.journey-finished.{}.body", outcome.as_str())
        }
        Event::ServiceStatus { component, .. } => {
            format!("notification.service-status.{}.body", component.as_str())
        }
        _ => format!("notification.{}.body", event.class()),
    }
}

fn action_copy_key(class: NotificationClass) -> Option<String> {
    match class {
        NotificationClass::ApprovalWaiting => Some("notification.action.open-approval".to_owned()),
        NotificationClass::SecurityNewDevice
        | NotificationClass::SecurityRecovery
        | NotificationClass::SecurityKeyRotation => {
            Some("notification.action.review-devices".to_owned())
        }
        NotificationClass::SecurityWalletRebinding => {
            Some("notification.action.review-wallet".to_owned())
        }
        NotificationClass::MoneyArrived
        | NotificationClass::JourneyFinished
        | NotificationClass::ClaimReady
        | NotificationClass::ServiceStatus => None,
    }
}

fn deep_link(event: &Event) -> String {
    match event {
        Event::ApprovalWaiting { approval_id, .. } => {
            format!("/app/approvals/{}", approval_id.as_str())
        }
        Event::MoneyArrived { entry_id, .. } => format!("/app/activity/{}", entry_id.as_str()),
        Event::JourneyFinished { journey_id, .. } => {
            format!("/app/journeys/{}", journey_id.as_str())
        }
        Event::ClaimReady { journey_id, .. } => {
            format!("/app/journeys/{}/claim", journey_id.as_str())
        }
        Event::SecurityNewDevice { .. } => "/app/security/devices".to_owned(),
        Event::SecurityRecovery { .. } => "/app/security/recovery".to_owned(),
        Event::SecurityWalletRebinding { .. } => "/app/security/wallet".to_owned(),
        Event::SecurityKeyRotation { .. } => "/app/security/keys".to_owned(),
        Event::ServiceStatus { .. } => "/app/status".to_owned(),
    }
}

fn event_money(event: &Event) -> Option<Money> {
    match event {
        Event::ApprovalWaiting { money, .. } | Event::JourneyFinished { money, .. } => {
            money.clone()
        }
        Event::MoneyArrived { money, .. } | Event::ClaimReady { money, .. } => Some(money.clone()),
        Event::SecurityNewDevice { .. }
        | Event::SecurityRecovery { .. }
        | Event::SecurityWalletRebinding { .. }
        | Event::SecurityKeyRotation { .. }
        | Event::ServiceStatus { .. } => None,
    }
}

fn email_subject(event: &Event, detail: DetailLevel) -> String {
    if matches!(detail, DetailLevel::Minimal) {
        return "New notification".to_owned();
    }
    match event {
        Event::ApprovalWaiting { .. } => "Approval waiting",
        Event::MoneyArrived { .. } => "Money arrived",
        Event::JourneyFinished { .. } => "Journey finished",
        Event::ClaimReady { .. } => "Claim ready",
        Event::SecurityNewDevice { .. } => "New device signed in",
        Event::SecurityRecovery { .. } => "Account recovery activity",
        Event::SecurityWalletRebinding { .. } => "Payout wallet change requested",
        Event::SecurityKeyRotation { .. } => "Account key rotation activity",
        Event::ServiceStatus { .. } => "Service status",
    }
    .to_owned()
}

fn email_sentence(event: &Event) -> &'static str {
    match event {
        Event::ApprovalWaiting { .. } => "An agent is waiting for your approval.",
        Event::MoneyArrived { .. } => "Money arrived in your account.",
        Event::JourneyFinished { outcome, .. } => match outcome {
            JourneyOutcome::Completed => "A money journey completed.",
            JourneyOutcome::Failed => "A money journey did not go through.",
        },
        Event::ClaimReady { .. } => "A withdrawal is ready to claim.",
        Event::SecurityNewDevice { .. } => "A new device signed in to your account.",
        Event::SecurityRecovery { .. } => "An account recovery event occurred.",
        Event::SecurityWalletRebinding { .. } => "A payout wallet change was requested.",
        Event::SecurityKeyRotation { .. } => "An account key rotation event occurred.",
        Event::ServiceStatus { .. } => "Part of the service is degraded.",
    }
}

fn email_lines(
    event: &Event,
    detail: DetailLevel,
    payload_agent: Option<&AgentId>,
    payload_money: Option<&Money>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if matches!(detail, DetailLevel::Minimal) {
        lines.push("Open the app to see what happened.".to_owned());
        return lines;
    }
    lines.push(email_sentence(event).to_owned());
    if let Some(agent) = payload_agent {
        lines.push(format!("Agent: {}", agent.as_str()));
    }
    if let Some(money) = payload_money {
        lines.push(format!("Amount: {}", money.render()));
    }
    lines
}

pub(crate) fn push_payload(content: &Content, id: &NotificationId, created_at: u64) -> String {
    let mut members = vec![
        text_member("notification_id", id.as_str()),
        text_member("class", content.class.as_str()),
        text_member("title_copy_key", &content.payload_title_copy_key),
        text_member("body_copy_key", &content.payload_body_copy_key),
        text_member("lock_screen_copy_key", &content.lock_screen_copy_key),
        text_member("deep_link", &content.deep_link),
    ];
    if let Some(action) = &content.action_copy_key {
        members.push(text_member("action_copy_key", action));
    }
    if let Some(agent) = &content.payload_agent {
        members.push(text_member("agent_id", agent.as_str()));
    }
    if let Some(money) = &content.payload_money {
        members.push(format!(
            "\"money\":{{\"amount\":\"{}\",\"currency\":\"{}\"}}",
            money.amount(),
            json_escape(money.currency())
        ));
    }
    members.push(text_member("created_at", &rfc3339(created_at)));
    format!("{{{}}}", members.join(","))
}

pub(crate) fn in_app_payload(content: &Content, id: &NotificationId, created_at: u64) -> String {
    let mut members = vec![
        text_member("notification_id", id.as_str()),
        text_member("class", content.class.as_str()),
        text_member("title_copy_key", &content.payload_title_copy_key),
        text_member("body_copy_key", &content.payload_body_copy_key),
        text_member("deep_link", &content.deep_link),
        text_member("created_at", &rfc3339(created_at)),
        "\"read\":false".to_owned(),
    ];
    if let Some(action) = &content.action_copy_key {
        members.push(text_member("action_copy_key", action));
    }
    if let Some(agent) = &content.payload_agent {
        members.push(text_member("agent_id", agent.as_str()));
    }
    if let Some(money) = &content.payload_money {
        members.push(format!(
            "\"money\":{{\"amount\":\"{}\",\"currency\":\"{}\"}}",
            money.amount(),
            json_escape(money.currency())
        ));
    }
    format!("{{{}}}", members.join(","))
}

pub(crate) fn email_payload(content: &Content, id: &NotificationId, created_at: u64) -> String {
    let mut lines = vec![
        format!("Date: {}", rfc2822(created_at)),
        format!("Message-ID: <{}@notifications.layerx>", id.as_str()),
        format!("Subject: {}", content.email_subject),
        "MIME-Version: 1.0".to_owned(),
        "Content-Type: text/plain; charset=UTF-8".to_owned(),
        String::new(),
    ];
    lines.extend(content.email_lines.clone());
    lines.push(format!("Open: {}", content.deep_link));
    if let Some(action) = &content.action_copy_key {
        lines.push(format!("Action: {action}"));
    }
    lines.join("\r\n")
}

fn text_member(name: &str, value: &str) -> String {
    format!("\"{name}\":\"{}\"", json_escape(value))
}

pub(crate) fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            control if (control as u32) < 0x20 => {
                let _ = write!(escaped, "\\u{:04x}", control as u32);
            }
            other => escaped.push(other),
        }
    }
    escaped
}

const SECONDS_PER_DAY: u64 = 86_400;
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

use crate::time::rfc3339;

fn civil_from_days(days: u64) -> (u64, usize, u64) {
    let shifted = days.saturating_add(719_468);
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = day_of_era
        .saturating_sub(day_of_era / 1460)
        .saturating_add(day_of_era / 36_524)
        .saturating_sub(day_of_era / 146_096)
        / 365;
    let year = year_of_era.saturating_add(era.saturating_mul(400));
    let day_of_year = day_of_era
        .saturating_sub(year_of_era.saturating_mul(365))
        .saturating_sub(year_of_era / 4)
        .saturating_add(year_of_era / 100);
    let month_period = day_of_year.saturating_mul(5).saturating_add(2) / 153;
    let day = day_of_year
        .saturating_sub(month_period.saturating_mul(153).saturating_add(2) / 5)
        .saturating_add(1);
    let month = if month_period < 10 {
        month_period.saturating_add(3)
    } else {
        month_period.saturating_sub(9)
    };
    let year = if month <= 2 {
        year.saturating_add(1)
    } else {
        year
    };
    let month_index = usize::try_from(month.saturating_sub(1)).unwrap_or(0) % MONTH_NAMES.len();
    (year, month_index, day)
}

pub(crate) fn rfc2822(seconds: u64) -> String {
    let days = seconds / SECONDS_PER_DAY;
    let remainder = seconds % SECONDS_PER_DAY;
    let (year, month_index, day) = civil_from_days(days);
    let weekday = WEEKDAY_NAMES[usize::try_from(days.saturating_add(4) % 7).unwrap_or(0)];
    let month = MONTH_NAMES[month_index];
    let hour = remainder / 3600;
    let minute = remainder % 3600 / 60;
    let second = remainder % 60;
    format!("{weekday}, {day:02} {month} {year} {hour:02}:{minute:02}:{second:02} +0000")
}

#[cfg(test)]
mod tests {
    use crate::time::rfc3339;

    use super::{json_escape, rfc2822};

    #[test]
    fn renders_the_epoch_and_a_modern_stamp() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_786_704_302), "2026-08-14T10:45:02Z");
        assert_eq!(rfc2822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(rfc2822(1_786_704_302), "Fri, 14 Aug 2026 10:45:02 +0000");
    }

    #[test]
    fn escapes_quotes_backslashes_and_control_bytes() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("line\nbreak"), "line\\u000abreak");
    }
}
