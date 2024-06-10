use openbook_v2::state::Market;
use solana_sdk::pubkey::Pubkey;

#[derive(Clone)]
pub struct MarketContext {
    pub address: Pubkey,
    pub market: Market,
}

impl MarketContext {
    pub fn max_quote_lots_including_maker_fees_from_usd(&self, quote_size_usd: u64) -> u64 {
        self.max_quote_lots_including_maker_fees(quote_size_usd * 10u64.pow(6))
    }
    pub fn max_base_lots_from_usd(&self, base_size: u64) -> u64 {
        self.max_base_lots(base_size * self.market.base_decimals as u64)
    }

    // For PostOnly or PostOnlySlide orders.
    pub fn max_quote_lots_including_maker_fees(&self, quote_size: u64) -> u64 {
        let quote_lots: u64 = quote_size / (self.market.quote_lot_size as u64);
        let fees: u64 = self.market.maker_fees_floor(quote_size);
        quote_lots + fees
    }

    pub fn max_base_lots(&self, base_size: u64) -> u64 {
        base_size / (self.market.base_lot_size as u64)
    }
}
