use tauri::http::{header, Method, Request, Response, StatusCode};

const MAX_ASSET_ID_LEN: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ThumbnailSize {
    Grid,
    Detail,
}

#[derive(Debug, Eq, PartialEq)]
struct ThumbnailRequest<'a> {
    asset_id: &'a str,
    size: ThumbnailSize,
}

#[derive(Debug, Eq, PartialEq)]
enum ProtocolError {
    MethodNotAllowed,
    InvalidRoute,
    InvalidAssetId,
    InvalidQuery,
}

fn parse_request(request: &Request<Vec<u8>>) -> Result<ThumbnailRequest<'_>, ProtocolError> {
    if request.method() != Method::GET {
        return Err(ProtocolError::MethodNotAllowed);
    }
    let asset_id = request
        .uri()
        .path()
        .strip_prefix("/thumbnail/")
        .filter(|value| !value.is_empty() && !value.contains('/'))
        .ok_or(ProtocolError::InvalidRoute)?;
    if asset_id.len() > MAX_ASSET_ID_LEN
        || !asset_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProtocolError::InvalidAssetId);
    }
    let size = match request.uri().query() {
        Some("size=grid") => ThumbnailSize::Grid,
        Some("size=detail") => ThumbnailSize::Detail,
        _ => return Err(ProtocolError::InvalidQuery),
    };
    Ok(ThumbnailRequest { asset_id, size })
}

fn response(status: StatusCode, content_type: &'static str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, max-age=300")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn placeholder_svg(request: &ThumbnailRequest<'_>) -> Vec<u8> {
    let edge = match request.size {
        ThumbnailSize::Grid => 256,
        ThumbnailSize::Detail => 1024,
    };
    let hue = request.asset_id.bytes().fold(0_u16, |value, byte| {
        value.wrapping_mul(31).wrapping_add(byte.into())
    }) % 360;
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{edge}" height="{edge}" viewBox="0 0 {edge} {edge}"><rect width="100%" height="100%" fill="hsl({hue} 35% 28%)"/><path d="M0 {edge} L{edge} 0" stroke="hsl({hue} 45% 55%)" stroke-width="12"/></svg>"#
    )
    .into_bytes()
}

pub fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match parse_request(&request) {
        Ok(parsed) => response(StatusCode::OK, "image/svg+xml", placeholder_svg(&parsed)),
        Err(ProtocolError::MethodNotAllowed) => response(
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain; charset=utf-8",
            b"method not allowed".to_vec(),
        ),
        Err(_) => response(
            StatusCode::BAD_REQUEST,
            "text/plain; charset=utf-8",
            b"invalid thumbnail request".to_vec(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    #[test]
    fn accepts_only_asset_id_and_known_size() {
        let input = request("asset://localhost/thumbnail/asset_0123-abcd?size=grid");
        let parsed = parse_request(&input).unwrap();
        assert_eq!(parsed.asset_id, "asset_0123-abcd");
        assert_eq!(parsed.size, ThumbnailSize::Grid);
    }

    #[test]
    fn accepts_detail_size() {
        let input = request("asset://localhost/thumbnail/asset-1?size=detail");
        let parsed = parse_request(&input).unwrap();
        assert_eq!(parsed.size, ThumbnailSize::Detail);
    }

    #[test]
    fn rejects_paths_and_encoded_path_traversal() {
        for uri in [
            "asset://localhost/thumbnail/../../etc/passwd?size=grid",
            "asset://localhost/thumbnail/%2e%2e%2fetc%2fpasswd?size=grid",
            "asset://localhost/thumbnail/file:///etc/passwd?size=grid",
            "asset://localhost/etc/passwd?size=grid",
        ] {
            assert!(parse_request(&request(uri)).is_err(), "accepted {uri}");
        }
    }

    #[test]
    fn rejects_unknown_missing_and_duplicate_parameters() {
        for uri in [
            "asset://localhost/thumbnail/asset-1",
            "asset://localhost/thumbnail/asset-1?size=medium",
            "asset://localhost/thumbnail/asset-1?size=grid&path=/etc/passwd",
            "asset://localhost/thumbnail/asset-1?size=grid&size=detail",
            "asset://localhost/thumbnail/asset-1?assetId=other&size=grid",
        ] {
            assert!(parse_request(&request(uri)).is_err(), "accepted {uri}");
        }
    }

    #[test]
    fn rejects_invalid_asset_ids_and_non_get_methods() {
        let long_id = "a".repeat(MAX_ASSET_ID_LEN + 1);
        assert!(parse_request(&request(&format!(
            "asset://localhost/thumbnail/{long_id}?size=grid"
        )))
        .is_err());
        assert!(
            parse_request(&request("asset://localhost/thumbnail/asset.name?size=grid")).is_err()
        );
        let post = Request::builder()
            .method(Method::POST)
            .uri("asset://localhost/thumbnail/asset-1?size=grid")
            .body(Vec::new())
            .unwrap();
        assert_eq!(parse_request(&post), Err(ProtocolError::MethodNotAllowed));
    }

    #[test]
    fn handler_rejects_a_user_supplied_path() {
        let result = handle(request(
            "asset://localhost/thumbnail/%2fetc%2fpasswd?size=grid",
        ));
        assert_eq!(result.status(), StatusCode::BAD_REQUEST);
        assert_eq!(result.body(), b"invalid thumbnail request");
    }
}
