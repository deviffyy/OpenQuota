#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCn,
    ZhTw,
}

impl Locale {
    pub fn for_preference(preference: &str) -> Self {
        match preference {
            "zh-CN" => Self::ZhCn,
            "zh-TW" => Self::ZhTw,
            "system" => system_language()
                .as_deref()
                .map(Self::from_language_tag)
                .unwrap_or(Self::En),
            _ => Self::En,
        }
    }

    pub fn from_language_tag(language: &str) -> Self {
        let normalized = normalize_locale_tag(language);
        if normalized == "zh-tw"
            || normalized == "zh-hk"
            || normalized == "zh-mo"
            || normalized.starts_with("zh-hant")
        {
            Self::ZhTw
        } else if normalized.starts_with("zh") {
            Self::ZhCn
        } else {
            Self::En
        }
    }
}

fn normalize_locale_tag(language: &str) -> String {
    language
        .trim()
        .split(['.', '@'])
        .next()
        .unwrap_or_default()
        .replace('_', "-")
        .to_ascii_lowercase()
}

pub struct Labels {
    #[cfg(any(not(target_os = "macos"), test))]
    pub open: &'static str,
    #[cfg(any(not(target_os = "macos"), test))]
    pub customize: &'static str,
    #[cfg(any(target_os = "macos", test))]
    pub settings: &'static str,
    #[cfg(any(not(target_os = "macos"), test))]
    pub settings_with_ellipsis: &'static str,
    pub quit: &'static str,
    #[cfg(any(target_os = "linux", target_os = "macos", test))]
    pub notification_action: &'static str,
    pub notification_failed: &'static str,
    pub notification_permission_failed: &'static str,
    pub launch_status_failed: &'static str,
    pub shortcut_unavailable: &'static str,
}

impl Labels {
    pub fn for_preference(preference: &str) -> Self {
        match Locale::for_preference(preference) {
            Locale::En => Self {
                #[cfg(any(not(target_os = "macos"), test))]
                open: "Open OpenQuota",
                #[cfg(any(not(target_os = "macos"), test))]
                customize: "Customize…",
                #[cfg(any(target_os = "macos", test))]
                settings: "Settings",
                #[cfg(any(not(target_os = "macos"), test))]
                settings_with_ellipsis: "Settings…",
                quit: "Quit OpenQuota",
                #[cfg(any(target_os = "linux", target_os = "macos", test))]
                notification_action: "Open OpenQuota",
                notification_failed: "The notification could not be delivered.",
                notification_permission_failed: "Notification permission could not be requested.",
                launch_status_failed: "Launch at login status could not be read.",
                shortcut_unavailable: "The saved global shortcut is currently unavailable.",
            },
            Locale::ZhCn => Self {
                #[cfg(any(not(target_os = "macos"), test))]
                open: "打开 OpenQuota",
                #[cfg(any(not(target_os = "macos"), test))]
                customize: "自定义…",
                #[cfg(any(target_os = "macos", test))]
                settings: "设置",
                #[cfg(any(not(target_os = "macos"), test))]
                settings_with_ellipsis: "设置…",
                quit: "退出 OpenQuota",
                #[cfg(any(target_os = "linux", target_os = "macos", test))]
                notification_action: "打开 OpenQuota",
                notification_failed: "无法发送通知。",
                notification_permission_failed: "无法请求通知权限。",
                launch_status_failed: "无法读取登录时启动状态。",
                shortcut_unavailable: "已保存的全局快捷键当前不可用。",
            },
            Locale::ZhTw => Self {
                #[cfg(any(not(target_os = "macos"), test))]
                open: "開啟 OpenQuota",
                #[cfg(any(not(target_os = "macos"), test))]
                customize: "自訂…",
                #[cfg(any(target_os = "macos", test))]
                settings: "設定",
                #[cfg(any(not(target_os = "macos"), test))]
                settings_with_ellipsis: "設定…",
                quit: "結束 OpenQuota",
                #[cfg(any(target_os = "linux", target_os = "macos", test))]
                notification_action: "開啟 OpenQuota",
                notification_failed: "無法傳送通知。",
                notification_permission_failed: "無法請求通知權限。",
                launch_status_failed: "無法讀取登入時啟動狀態。",
                shortcut_unavailable: "已儲存的全域快捷鍵目前無法使用。",
            },
        }
    }
}

pub fn metric_label(locale: Locale, id: &str, fallback: &str) -> String {
    let common = if id.ends_with(".session") {
        Some(("Session", "会话", "對話"))
    } else if id.ends_with(".weekly") {
        Some(("Weekly", "每周", "每週"))
    } else if id.ends_with(".today") {
        Some(("Today", "今天", "今天"))
    } else if id.ends_with(".yesterday") {
        Some(("Yesterday", "昨天", "昨天"))
    } else if id.ends_with(".last30") {
        Some(("30 Days", "30 天", "30 天"))
    } else {
        match id {
            "claude.extra" | "codex.credits" | "cursor.onDemand" | "copilot.extra"
            | "grok.payAsYouGo" => Some(("Extra Usage", "额外用量", "額外用量")),
            "codex.rateLimitResets" => Some(("Rate Limit Resets", "限额重置", "限額重設")),
            "cursor.credits" | "copilot.premium" | "openrouter.credits" => {
                Some(("Credits", "额度", "額度"))
            }
            "cursor.usage" => Some(("Total Usage", "总用量", "總用量")),
            "cursor.auto" => Some(("Auto Usage", "自动用量", "自動用量")),
            "cursor.api" => Some(("API Usage", "API 用量", "API 用量")),
            "cursor.requests" => Some(("Requests", "请求", "要求")),
            "openrouter.balance" => Some(("Balance", "余额", "餘額")),
            "openrouter.week" => Some(("This Week", "本周", "本週")),
            "openrouter.month" => Some(("This Month", "本月", "本月")),
            "openrouter.keyLimit" => Some(("Key Limit", "密钥限额", "金鑰限額")),
            "zai.webSearches" => Some(("Web Searches", "网页搜索", "網頁搜尋")),
            "codex.sparkWeekly" => Some(("Spark Weekly", "Spark 每周", "Spark 每週")),
            "antigravity.claudeWeekly" => Some(("Claude Weekly", "Claude 每周", "Claude 每週")),
            _ => None,
        }
    };
    common
        .map(|labels| match locale {
            Locale::En => labels.0,
            Locale::ZhCn => labels.1,
            Locale::ZhTw => labels.2,
        })
        .unwrap_or(fallback)
        .to_owned()
}

pub fn usage_word(locale: Locale, used: bool) -> &'static str {
    match (locale, used) {
        (Locale::En, true) => "used",
        (Locale::En, false) => "left",
        (Locale::ZhCn, true) => "已用",
        (Locale::ZhCn, false) => "剩余",
        (Locale::ZhTw, true) => "已用",
        (Locale::ZhTw, false) => "剩餘",
    }
}

pub fn count_unit(locale: Locale, unit: &str) -> String {
    match (locale, unit) {
        (Locale::ZhCn, "requests") => "次请求".to_owned(),
        (Locale::ZhCn, "searches") => "次搜索".to_owned(),
        (Locale::ZhTw, "requests") => "次要求".to_owned(),
        (Locale::ZhTw, "searches") => "次搜尋".to_owned(),
        _ => unit.to_owned(),
    }
}

#[cfg(target_os = "windows")]
fn system_language() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    let mut buffer = [0_u16; 85];
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    (length > 1).then(|| String::from_utf16_lossy(&buffer[..length as usize - 1]))
}

#[cfg(not(target_os = "windows"))]
fn system_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::{Labels, Locale};

    #[test]
    fn language_tags_match_frontend_resolution() {
        assert_eq!(super::normalize_locale_tag("zh_CN.UTF-8"), "zh-cn");
        assert_eq!(super::normalize_locale_tag("zh_TW@variant"), "zh-tw");
        assert_eq!(Locale::from_language_tag("zh-CN"), Locale::ZhCn);
        assert_eq!(Locale::from_language_tag("zh-SG"), Locale::ZhCn);
        assert_eq!(Locale::from_language_tag("zh-TW"), Locale::ZhTw);
        assert_eq!(Locale::from_language_tag("zh-HK"), Locale::ZhTw);
        assert_eq!(Locale::from_language_tag("zh-Hant"), Locale::ZhTw);
        assert_eq!(Locale::from_language_tag("fr-FR"), Locale::En);
        assert_eq!(Locale::from_language_tag("C"), Locale::En);
        assert_eq!(Locale::from_language_tag("POSIX"), Locale::En);
    }

    #[test]
    fn explicit_preferences_have_localized_native_labels() {
        assert_eq!(Labels::for_preference("en").open, "Open OpenQuota");
        assert_eq!(Labels::for_preference("en").customize, "Customize…");
        assert_eq!(Labels::for_preference("en").settings, "Settings");
        assert_eq!(
            Labels::for_preference("en").settings_with_ellipsis,
            "Settings…"
        );
        assert_eq!(
            Labels::for_preference("en").notification_action,
            "Open OpenQuota"
        );
        assert_eq!(Labels::for_preference("zh-CN").open, "打开 OpenQuota");
        assert_eq!(Labels::for_preference("zh-CN").customize, "自定义…");
        assert_eq!(Labels::for_preference("zh-CN").settings, "设置");
        assert_eq!(Labels::for_preference("zh-TW").open, "開啟 OpenQuota");
        assert_eq!(Labels::for_preference("zh-TW").customize, "自訂…");
        assert_eq!(Labels::for_preference("zh-TW").settings, "設定");
        assert_eq!(Labels::for_preference("invalid").settings, "Settings");
    }

    #[test]
    fn native_metric_labels_and_units_follow_the_resolved_locale() {
        assert_eq!(
            super::metric_label(Locale::ZhCn, "codex.weekly", "Weekly"),
            "每周"
        );
        assert_eq!(
            super::metric_label(Locale::ZhTw, "cursor.requests", "Requests"),
            "要求"
        );
        assert_eq!(super::count_unit(Locale::ZhTw, "searches"), "次搜尋");
        assert_eq!(super::usage_word(Locale::ZhCn, false), "剩余");
        assert_eq!(
            super::metric_label(Locale::En, "unknown", "Custom"),
            "Custom"
        );
    }
}
