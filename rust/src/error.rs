use std::fmt;

/// 初始化/推流阶段的错误。
///
/// `Message` 会在退出前打印到 stderr；`Reported` 表示调用方已经打印过具体原因，
/// 只需在结尾输出“推流失败。”。
#[derive(Debug)]
pub enum AppError {
    Message(String),
    Reported,
}

impl AppError {
    pub fn msg(message: impl Into<String>) -> Self {
        AppError::Message(message.into())
    }

    pub fn ctx(context: &str, error: impl fmt::Display) -> Self {
        AppError::Message(format!("{context}: {error}"))
    }

    pub fn report(&self) {
        if let AppError::Message(message) = self {
            eprintln!("{message}");
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Message(message) => f.write_str(message),
            AppError::Reported => Ok(()),
        }
    }
}

impl std::error::Error for AppError {}
