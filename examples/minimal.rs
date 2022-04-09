const BEHIND_PROXY: bool = false;
const ALLOW_DOTENV_ON_DEBUG: bool = true;

#[tokio::main]
async fn main() {
    cf_route_services::serve::<BEHIND_PROXY, ALLOW_DOTENV_ON_DEBUG>(None, None).await;
}