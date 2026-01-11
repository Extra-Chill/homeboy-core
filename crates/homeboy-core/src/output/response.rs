use crate::Error;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CliResponse<T: Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CliError>,
}

#[derive(Debug, Serialize)]
pub struct CliError {
    pub code: String,
    pub message: String,
}

impl<T: Serialize> CliResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| {
            r#"{"success":false,"error":{"code":"JSON_ERROR","message":"Failed to serialize response"}}"#.to_string()
        })
    }
}

impl CliResponse<()> {
    pub fn failure(code: &str, message: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(CliError {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }

    pub fn from_error(err: &Error) -> Self {
        Self::failure(err.code(), &err.to_string())
    }
}

pub fn print_success<T: Serialize>(data: T) {
    println!("{}", CliResponse::success(data).to_json());
}

pub fn print_error(code: &str, message: &str) {
    println!("{}", CliResponse::<()>::failure(code, message).to_json());
}

pub fn print_result<T: Serialize>(result: crate::Result<T>) {
    match result {
        Ok(data) => print_success(data),
        Err(err) => println!("{}", CliResponse::<()>::from_error(&err).to_json()),
    }
}

pub fn map_cmd_result_to_json<T: Serialize>(
    result: crate::Result<(T, i32)>,
) -> (crate::Result<serde_json::Value>, i32) {
    match result {
        Ok((data, exit_code)) => match serde_json::to_value(data) {
            Ok(value) => (Ok(value), exit_code),
            Err(err) => (Err(crate::Error::Json(err)), 1),
        },
        Err(err) => (Err(err), 1),
    }
}

pub fn print_json_result(result: crate::Result<serde_json::Value>) {
    print_result(result)
}
