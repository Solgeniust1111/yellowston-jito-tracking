use copy_traing_bot_rust::{common::utils::import_env_var, engine::monitor::monitor};
use dotenv::dotenv;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let yellowstone_rpc_http = import_env_var("YELLOWSTONE_RPC_HTTP");
    let _ = monitor(yellowstone_rpc_http).await;
}
