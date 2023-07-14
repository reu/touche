use http::{header::CONTENT_TYPE, Request};
use multipart::server::{HttpRequest, Multipart};

use crate::HttpBody;

struct MultipartRequest<B: HttpBody>(Request<B>);

impl<B: HttpBody> From<Request<B>> for MultipartRequest<B> {
    fn from(req: Request<B>) -> Self {
        MultipartRequest(req)
    }
}

impl<B: HttpBody> HttpRequest for MultipartRequest<B> {
    type Body = B::Reader;

    fn multipart_boundary(&self) -> Option<&str> {
        const BOUNDARY: &str = "boundary=";

        let content_type = self.0.headers().get(CONTENT_TYPE)?.to_str().ok()?;

        let start = content_type.find(BOUNDARY)? + BOUNDARY.len();
        let end = content_type[start..]
            .find(';')
            .map_or(content_type.len(), |end| start + end);

        Some(&content_type[start..end])
    }

    fn body(self) -> Self::Body {
        self.0.into_body().into_reader()
    }
}

pub fn multipart_request<B: HttpBody>(req: Request<B>) -> Result<Multipart<B::Reader>, Request<B>> {
    Multipart::from_request(MultipartRequest(req)).map_err(|req| req.0)
}

#[cfg(test)]
mod tests {
    use http::{method, Method};

    use super::*;

    #[test]
    fn test_multipart_server_request() {
        let req = Request::builder()
            .method(Method::POST)
            .uri("/multipart")
            .header(
                CONTENT_TYPE,
                "Content-Type: multipart/form-data;boundary=\"----boundary\"\r\n\r\n",
            )
            .body("----boundary\r\nContent-Disposition: form-data; name=\"key\"\r\n\r\nvalue\r\n----boundary--\r\n");
    }
}
