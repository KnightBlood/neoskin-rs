//! 健康检查。

use suprnova::{Response, handler, json_response};

#[handler]
pub async fn index() -> Response {
    json_response!({
        "status": "ok",
        "service": "neoskin-rs",
    })
}
