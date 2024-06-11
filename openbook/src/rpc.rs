//! This module implements a thread safe client to interact with a remote Solana node.

use std::sync::Arc;

use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcAccountInfoConfig};
use solana_sdk::pubkey::Pubkey;

use anchor_lang::{AccountDeserialize, Discriminator};

use openbook_v2::state::OpenOrdersAccount;

use solana_client::{
    rpc_config::RpcProgramAccountsConfig,
    rpc_filter::{Memcmp, RpcFilterType},
};

use solana_account_decoder::UiAccountEncoding;

/// Wrapper type for RpcClient providing additional functionality and enabling Debug trait implementation.
///
/// This struct holds an `Arc` of `RpcClient` to ensure thread safety and efficient resource sharing.
#[derive(Clone)]
pub struct Rpc(Arc<RpcClient>);

impl Rpc {
    /// Constructs a new Rpc wrapper around the provided RpcClient instance.
    ///
    /// # Parameters
    ///
    /// - `rpc_client`: An instance of RpcClient to wrap.
    ///
    /// # Returns
    ///
    /// A new Rpc wrapper around the provided RpcClient.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use openbook::rpc_client::RpcClient;
    /// use openbook::rpc::Rpc;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let rpc_url = std::env::var("RPC_URL").expect("RPC_URL is not set");
    ///
    ///     let connection = RpcClient::new(rpc_url);
    ///     let rpc_client = Rpc::new(connection);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub fn new(rpc_client: RpcClient) -> Self {
        Rpc(Arc::new(rpc_client))
    }

    /// Returns a reference to the inner RpcClient instance wrapped by this wrapper.
    pub fn inner(&self) -> &RpcClient {
        &self.0
    }

    pub async fn fetch_anchor_account<T: AccountDeserialize>(
        &self,
        address: &Pubkey,
    ) -> anyhow::Result<T> {
        let account = self.inner().get_account(address).await?;
        Ok(T::try_deserialize(&mut (&account.data as &[u8]))?)
    }

    pub async fn fetch_openbook_accounts(
        &self,
        program: Pubkey,
        owner: Pubkey,
    ) -> anyhow::Result<Vec<(Pubkey, OpenOrdersAccount)>> {
        let config = RpcProgramAccountsConfig {
            filters: Some(vec![
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                    0,
                    OpenOrdersAccount::discriminator().to_vec(),
                )),
                RpcFilterType::Memcmp(Memcmp::new_raw_bytes(8, owner.to_bytes().to_vec())),
            ]),
            account_config: RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                ..RpcAccountInfoConfig::default()
            },
            ..RpcProgramAccountsConfig::default()
        };
        self.inner()
            .get_program_accounts_with_config(&program, config)
            .await?
            .into_iter()
            .map(|(key, account)| {
                Ok((
                    key,
                    OpenOrdersAccount::try_deserialize(&mut (&account.data as &[u8]))?,
                ))
            })
            .collect()
    }
}
