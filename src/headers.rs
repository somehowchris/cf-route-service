pub mod http {
    use http::header::HeaderName;

    lazy_static! {
        pub static ref X_CF_FORWARDED_URL: HeaderName =
            HeaderName::from_static("x-cf-forwarded-url");
        pub static ref X_CF_PROXY_SIGNATURE: HeaderName =
            HeaderName::from_static("x-cf-proxy-signature");
        pub static ref X_CF_PROXY_METADATA: HeaderName =
            HeaderName::from_static("x-cf-proxy-metadata");
        pub static ref X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");
        pub static ref ROUTE_SERVICES_HEADERS_LIST: [&'static HeaderName; 3] = [
            &X_CF_PROXY_METADATA,
            &X_CF_PROXY_SIGNATURE,
            &X_CF_FORWARDED_URL
        ];
    }
}

pub mod axum {
    use headers::{Header, HeaderName, HeaderValue};

    macro_rules! create_header {
        ($name:ident,$header:expr) => {
            #[derive(Debug)]
            pub struct $name(pub String);

            impl Header for $name {
                fn name() -> &'static HeaderName {
                    &*$header
                }

                fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
                where
                    I: Iterator<Item = &'i HeaderValue>,
                {
                    let value = values
                        .next()
                        .ok_or_else(headers::Error::invalid)?
                        .to_str()
                        .unwrap();

                    if !value.trim().is_empty() {
                        Ok($name(value.to_string()))
                    } else {
                        Err(headers::Error::invalid())
                    }
                }

                fn encode<E>(&self, values: &mut E)
                where
                    E: Extend<HeaderValue>,
                {
                    let value = HeaderValue::from_str(self.0.as_str()).unwrap();

                    values.extend(std::iter::once(value));
                }
            }
        };
    }

    create_header!(ForwardedUrl, super::http::X_CF_FORWARDED_URL);
    create_header!(ProxySignature, super::http::X_CF_PROXY_SIGNATURE);
    create_header!(ProxyMetadata, super::http::X_CF_PROXY_METADATA);
}
