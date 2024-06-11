use openbook::ob_client::OBClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::EncodableKey;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let /*mut*/ _client = OBClient::new(
        "https://devnet.solana.com".into(),
        Arc::new(Keypair::read_from_file("keypair.json").unwrap()),
        None,
        CommitmentConfig::confirmed(),
        pubkey!("gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK"),
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
