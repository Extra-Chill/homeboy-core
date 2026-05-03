use std::time::Duration;

use crate::error::Error;

#[derive(Debug, Clone)]
pub(crate) struct HttpProbeError {
    pub message: String,
    pub is_connect: bool,
}

pub(crate) fn get_status(url: &str, timeout: Duration) -> std::result::Result<u16, HttpProbeError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| HttpProbeError {
            message: Error::internal_unexpected(format!("build http client: {}", e)).message,
            is_connect: false,
        })?;

    let response = client.get(url).send().map_err(|e| HttpProbeError {
        message: format!("HTTP GET {} failed: {}", url, e),
        is_connect: e.is_connect(),
    })?;

    Ok(response.status().as_u16())
}
