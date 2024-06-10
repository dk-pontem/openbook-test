use openbook::ob_client::OBClient;
use openbook_v2::state::Side;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;

#[tokio::main]
async fn main() {
    let mut client = OBClient::new(
        CommitmentConfig::confirmed(),
        pubkey!("gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK"),
        false,
    )
    .await
    .unwrap();

    // client.create_market().await.unwrap();
    // client.find_or_create_account().await.unwrap()
    // client
    //     .place_market_order(1000.0, 1000, Side::Bid)
    //     .await
    //     .unwrap();
}
