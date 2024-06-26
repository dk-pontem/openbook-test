use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anchor_lang::{prelude::System, Id};
use anchor_spl::{associated_token::AssociatedToken, token::Token};
use anyhow::{Context, Error, Result};
use rand::random;
use spl_associated_token_account::get_associated_token_address;

use openbook_v2::{
    state::{Market, OracleConfigParams, PlaceOrderType, SelfTradeBehavior, Side},
    PlaceOrderArgs,
};
use solana_client::nonblocking::rpc_client::RpcClient;

use solana_sdk::transaction::Transaction;
use solana_sdk::{
    commitment_config::CommitmentConfig, instruction::Instruction, pubkey::Pubkey,
    signature::Keypair, signer::Signer,
};

use crate::{context::MarketContext, rpc::Rpc};

/// OpenBook v2 Client to interact with the OpenBook market and perform actions.
#[derive(Clone)]
pub struct OBClient {
    /// The keypair of the owner used for signing transactions related to the market.
    pub owner: Arc<Keypair>,

    /// The RPC client for interacting with the Solana blockchain.
    pub rpc_client: Rpc,

    /// The public key of the associated account holding the quote tokens.
    pub quote_ata: Pubkey,

    /// The public key of the associated account holding the base tokens.
    pub base_ata: Pubkey,

    /// The public key of the market ID.
    pub market_id: Pubkey,

    /// Account info of the wallet on the market (e.g., open orders).
    pub open_orders_account: Pubkey,

    /// Information about the OpenBook market.
    pub market_info: Market,

    /// Context information for the market.
    pub context: MarketContext,
}

impl OBClient {
    /// Initializes a new instance of the `OBClient` struct, representing an OpenBook V2 program client.
    ///
    /// This method initializes the `OBClient` struct, containing information about the requested market id,
    /// It fetches and stores all data about this OpenBook market. Additionally, it includes information about
    /// the account associated with the wallet on the OpenBook market (e.g., open orders, bids, asks, etc.).
    ///
    /// # Arguments
    ///
    /// * `commitment` - Commitment configuration for transactions, determining the level of finality required.
    /// * `market_id` - Public key (ID) of the market to fetch information about.
    /// * `new` - Boolean indicating whether to create new open orders and index accounts.
    /// * `load` - Boolean indicating whether to load market data immediately after initialization.
    ///
    /// # Returns
    ///
    /// Returns a `Result` wrapping a new instance of the `OBClient` struct initialized with the provided parameters,
    /// or an `Error` if the initialization process fails.
    ///
    /// # Example
    ///
    /// ```rust
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     println!("Initialized OBClient: {:?}", ob_client);
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Business Logic
    ///
    /// 1. Retrieve necessary environment variables, such as the `RPC_URL`, `KEY_PATH`, open orders key `OOS_KEY`, and index key `INDEX_KEY`.
    /// 2. Read the owner's keypair from the specified key path.
    /// 3. Initialize the RPC client with the given commitment configuration.
    /// 4. Fetch the market information from the Solana blockchain.
    /// 5. Generate associated token addresses (ATA) for the base and quote tokens.
    /// 6. Initialize the context with market information.
    /// 7. Initialize the account fetcher for fetching account data.
    /// 8. Populate the initial fields of the `OBClient` struct.
    /// 9. Load open orders and bids/asks information if the `load` parameter is set to `true`.
    /// 10. Create new open orders and index accounts if the `new` parameter is set to `true`.
    ///
    pub async fn new(
        rpc_url: String,
        owner: Arc<Keypair>,
        open_orders_account: Option<Pubkey>,
        commitment: CommitmentConfig,
        market_id: Pubkey,
    ) -> Result<Self, Error> {
        let pub_owner_key = owner.pubkey();
        let rpc_client = Rpc::new(RpcClient::new_with_commitment(rpc_url.clone(), commitment));
        let market_info = rpc_client
            .fetch_anchor_account::<Market>(&market_id)
            .await?;
        let base_ata = get_associated_token_address(&pub_owner_key.clone(), &market_info.base_mint);
        let quote_ata =
            get_associated_token_address(&pub_owner_key.clone(), &market_info.quote_mint);

        let context = MarketContext {
            market: market_info,
            address: market_id,
        };

        let mut ob_client = Self {
            rpc_client,
            market_info,
            owner,
            quote_ata,
            base_ata,
            market_id,
            open_orders_account: open_orders_account.unwrap_or_default(),
            context,
        };

        if open_orders_account.is_none() {
            ob_client.open_orders_account = ob_client.find_or_create_account().await?;
        }

        Ok(ob_client)
    }

    /// # Example
    ///
    /// ```rust , ignore
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    /// use openbook::v2_state::Side;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     let (confirmed, sig, order_id, slot) = ob_client.place_limit_order(165.2, 1000, Side::Bid).await?;
    ///
    ///     println!("Got Order ID: {:?}", order_id);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn place_limit_order(
        &mut self,
        limit_price: f64,
        quote_size: u64,
        side: Side,
    ) -> Result<Transaction> {
        let current_time = get_unix_secs();
        let price_lots = self.native_price_to_lots_price(limit_price);
        let max_quote_lots = self
            .context
            .max_quote_lots_including_maker_fees_from_usd(quote_size);
        let base_size = self.get_base_size_from_quote(quote_size, limit_price);
        let max_base_lots = self.context.max_base_lots_from_usd(base_size);
        let ata = match side {
            Side::Bid => self.quote_ata,
            Side::Ask => self.base_ata,
        };
        let vault = self.market_info.get_vault_by_side(side);

        tracing::debug!("base: {max_base_lots}, quote: {max_quote_lots}");
        let oid = random::<u64>();

        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::PlaceOrder {
                        open_orders_account: self.open_orders_account,
                        open_orders_admin: None,
                        signer: self.owner(),
                        market: self.market_id,
                        bids: self.market_info.bids,
                        asks: self.market_info.asks,
                        event_heap: self.market_info.event_heap,
                        oracle_a: self.market_info.oracle_a.into(),
                        oracle_b: self.market_info.oracle_b.into(),
                        user_token_account: ata,
                        market_vault: vault,
                        token_program: Token::id(),
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::PlaceOrder {
                args: PlaceOrderArgs {
                    side,
                    price_lots,
                    max_base_lots: max_base_lots as i64,
                    max_quote_lots_including_fees: max_quote_lots as i64,
                    client_order_id: oid,
                    order_type: PlaceOrderType::PostOnly,
                    expiry_timestamp: current_time + 86_400,
                    self_trade_behavior: SelfTradeBehavior::AbortTransaction,
                    limit: 12,
                },
            }),
        };

        self.to_trx(vec![ix]).await
    }

    pub async fn place_market_order(
        &mut self,
        limit_price: f64,
        quote_size: u64,
        side: Side,
    ) -> Result<Transaction> {
        let current_time = get_unix_secs();
        let price_lots = self.native_price_to_lots_price(limit_price);
        let max_quote_lots = self
            .context
            .max_quote_lots_including_maker_fees_from_usd(quote_size);
        let base_size = self.get_base_size_from_quote(quote_size, limit_price);
        let max_base_lots = self.context.max_base_lots_from_usd(base_size);
        let ata = match side {
            Side::Bid => self.quote_ata,
            Side::Ask => self.base_ata,
        };
        let vault = self.market_info.get_vault_by_side(side);

        tracing::debug!("base: {max_base_lots}, quote: {max_quote_lots}");
        let oid = random::<u64>();

        // TODO: update to market order inst
        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::PlaceOrder {
                        open_orders_account: self.open_orders_account,
                        open_orders_admin: None,
                        signer: self.owner(),
                        market: self.market_id,
                        bids: self.market_info.bids,
                        asks: self.market_info.asks,
                        event_heap: self.market_info.event_heap,
                        oracle_a: self.market_info.oracle_a.into(),
                        oracle_b: self.market_info.oracle_b.into(),
                        user_token_account: ata,
                        market_vault: vault,
                        token_program: Token::id(),
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::PlaceOrder {
                args: PlaceOrderArgs {
                    side,
                    price_lots,
                    max_base_lots: max_base_lots as i64,
                    max_quote_lots_including_fees: max_quote_lots as i64,
                    client_order_id: oid,
                    order_type: PlaceOrderType::PostOnly,
                    expiry_timestamp: current_time + 86_400,
                    self_trade_behavior: SelfTradeBehavior::AbortTransaction,
                    limit: 12,
                },
            }),
        };

        self.to_trx(vec![ix]).await
    }

    /// # Example
    ///
    /// ```rust , ignore
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     let (confirmed, sig) = ob_client.cancel_limit_order(12345678123578).await?;
    ///
    ///     println!("Got Sig: {:?}", sig);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn cancel_limit_order(&self, order_id: u128) -> Result<Transaction> {
        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::CancelOrder {
                        open_orders_account: self.open_orders_account,
                        signer: self.owner(),
                        market: self.market_id,
                        bids: self.market_info.bids,
                        asks: self.market_info.asks,
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::CancelOrder {
                order_id,
            }),
        };

        self.to_trx(vec![ix]).await
    }

    /// # Example
    ///
    /// ```rust , ignore
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     let (confirmed, sig) = ob_client.cancel_all().await?;
    ///
    ///     println!("Got Sig: {:?}", sig);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn cancel_all(&self) -> Result<Transaction> {
        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::CancelOrder {
                        open_orders_account: self.open_orders_account,
                        signer: self.owner(),
                        market: self.market_id,
                        bids: self.market_info.bids,
                        asks: self.market_info.asks,
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::CancelAllOrders {
                side_option: None,
                limit: 255,
            }),
        };

        self.to_trx(vec![ix]).await
    }

    /// # Example
    ///
    /// ```rust , ignore
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     let account = ob_client.find_or_create_account().await?;
    ///
    ///     println!("Got Account: {:?}", account);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn find_or_create_account(&self) -> Result<Pubkey> {
        let program = openbook_v2::id();

        let openbook_account_name = "random";

        let mut openbook_account_tuples = self
            .rpc_client
            .fetch_openbook_accounts(program, self.owner())
            .await?;
        let openbook_account_opt = openbook_account_tuples
            .iter()
            .find(|(_, account)| account.name() == openbook_account_name);
        if openbook_account_opt.is_none() {
            openbook_account_tuples
                .sort_by(|a, b| a.1.account_num.partial_cmp(&b.1.account_num).unwrap());
            let account_num = match openbook_account_tuples.last() {
                Some(tuple) => tuple.1.account_num + 1,
                None => 0u32,
            };
            self.create_open_orders_account(account_num, openbook_account_name)
                .await
                .context("Failed to create account...")?;
        }
        let openbook_account_tuples = self
            .rpc_client
            .fetch_openbook_accounts(program, self.owner())
            .await?;
        let index = openbook_account_tuples
            .iter()
            .position(|tuple| tuple.1.name() == openbook_account_name)
            .unwrap();

        Ok(openbook_account_tuples[index].0)
    }

    /// # Example
    ///
    /// ```rust
    /// use openbook::commitment_config::CommitmentConfig;
    /// use openbook::v2::ob_client::OBClient;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let commitment = CommitmentConfig::confirmed();
    ///
    ///     let market_id = "gQN1TNHiqj5x82ZQd7JZ8rm8WD4xwWtXxd4onReWZNK".parse()?;
    ///
    ///     let ob_client = OBClient::new(commitment, market_id, false, true).await?;
    ///
    ///     let (confirmed, sig, account) = ob_client.create_open_orders_account(2, "Sol-USDC-OO-Account").await?;
    ///
    ///     println!("Got New OO Account: {:?}", account);
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn create_open_orders_account(
        &self,
        account_num: u32,
        name: &str,
    ) -> Result<Transaction> {
        let owner = &self.owner;
        let payer = &self.owner;
        let market = self.market_id;

        let delegate = None;

        let open_orders_indexer = Pubkey::find_program_address(
            &[b"OpenOrdersIndexer".as_ref(), owner.pubkey().as_ref()],
            &openbook_v2::id(),
        )
        .0;

        let account = Pubkey::find_program_address(
            &[
                b"OpenOrders".as_ref(),
                owner.pubkey().as_ref(),
                &account_num.to_le_bytes(),
            ],
            &openbook_v2::id(),
        )
        .0;

        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: anchor_lang::ToAccountMetas::to_account_metas(
                &openbook_v2::accounts::CreateOpenOrdersAccount {
                    owner: owner.pubkey(),
                    open_orders_indexer,
                    open_orders_account: account,
                    payer: payer.pubkey(),
                    delegate_account: delegate,
                    market,
                    system_program: System::id(),
                },
                None,
            ),
            data: anchor_lang::InstructionData::data(
                &openbook_v2::instruction::CreateOpenOrdersAccount {
                    name: name.to_string(),
                },
            ),
        };

        self.to_trx(vec![ix]).await
    }

    pub fn owner(&self) -> Pubkey {
        self.owner.pubkey()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_market(
        &self,
        market: Pubkey,
        market_authority: Pubkey,
        bids: Pubkey,
        asks: Pubkey,
        event_heap: Pubkey,
        base_mint: Pubkey,
        quote_mint: Pubkey,
        oracle_a: Option<Pubkey>,
        oracle_b: Option<Pubkey>,
        collect_fee_admin: Pubkey,
        open_orders_admin: Option<Pubkey>,
        consume_events_admin: Option<Pubkey>,
        close_market_admin: Option<Pubkey>,
        event_authority: Pubkey,
        name: String,
        oracle_config: OracleConfigParams,
        base_lot_size: i64,
        quote_lot_size: i64,
        maker_fee: i64,
        taker_fee: i64,
        time_expiry: i64,
    ) -> Result<Transaction> {
        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::CreateMarket {
                        market,
                        market_authority,
                        bids,
                        asks,
                        event_heap,
                        payer: self.owner(),
                        market_base_vault: self.base_ata,
                        market_quote_vault: self.quote_ata,
                        base_mint,
                        quote_mint,
                        system_program: solana_sdk::system_program::id(),
                        oracle_a,
                        oracle_b,
                        collect_fee_admin,
                        open_orders_admin,
                        consume_events_admin,
                        close_market_admin,
                        event_authority,
                        program: openbook_v2::id(),
                        token_program: Token::id(),
                        associated_token_program: AssociatedToken::id(),
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::CreateMarket {
                name,
                oracle_config,
                base_lot_size,
                quote_lot_size,
                maker_fee,
                taker_fee,
                time_expiry,
            }),
        };

        self.to_trx(vec![ix]).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn deposit(
        &self,
        market_address: Pubkey,
        base_amount: u64,
        quote_amount: u64,
        user_base_account: Pubkey,
        user_quote_account: Pubkey,
        market_base_vault: Pubkey,
        market_quote_vault: Pubkey,
    ) -> Result<Transaction> {
        let ix = Instruction {
            program_id: openbook_v2::id(),
            accounts: {
                anchor_lang::ToAccountMetas::to_account_metas(
                    &openbook_v2::accounts::Deposit {
                        open_orders_account: self.open_orders_account,
                        owner: self.owner(),
                        market: market_address,
                        user_base_account,
                        user_quote_account,
                        market_base_vault,
                        market_quote_vault,
                        token_program: Token::id(),
                    },
                    None,
                )
            },
            data: anchor_lang::InstructionData::data(&openbook_v2::instruction::Deposit {
                base_amount,
                quote_amount,
            }),
        };

        self.to_trx(vec![ix]).await
    }

    pub fn native_price_to_lots_price(&self, limit_price: f64) -> i64 {
        let base_decimals = self.market_info.base_decimals as u32;
        let quote_decimals = self.market_info.quote_decimals as u32;
        let base_factor = 10_u64.pow(base_decimals);
        let quote_factor = 10_u64.pow(quote_decimals);
        let price_factor = (base_factor / quote_factor) as f64;
        (limit_price * price_factor) as i64
    }

    pub fn get_base_size_from_quote(&self, quote_size: u64, limit_price: f64) -> u64 {
        let base_decimals = self.market_info.base_decimals as u32;
        let base_factor = 10_u64.pow(base_decimals) as f64;
        ((quote_size as f64 / limit_price) * base_factor) as u64
    }

    pub async fn get_token_balance(&self, ata: &Pubkey) -> Result<f64> {
        let r = self
            .rpc_client
            .inner()
            .get_token_account_balance(ata)
            .await?;
        Ok(r.ui_amount.unwrap())
    }

    pub async fn to_trx(&self, instructions: Vec<Instruction>) -> anyhow::Result<Transaction> {
        let (recent_hash, _) = self
            .rpc_client
            .inner()
            .get_latest_blockhash_with_commitment(self.rpc_client.inner().commitment())
            .await?;
        Ok(Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.owner.pubkey()),
            &[&self.owner],
            recent_hash,
        ))
    }
}

/// Gets the current UNIX timestamp in seconds.
fn get_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
