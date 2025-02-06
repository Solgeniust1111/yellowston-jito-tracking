use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

use chrono::{DateTime, Utc};
use futures::SinkExt;
use maplit::hashmap;
use solana_account_decoder::StringAmount;
use solana_sdk::{pubkey::Pubkey, signature::Signature};
use solana_transaction_status::TransactionStatusMeta;
use tokio_stream::StreamExt;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterTransactions, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
};

pub const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const RAYDIUM_PROGRAM: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const PUMP_FUN_CREATE_IX_DISCRIMINATOR: &'static [u8] = &[24, 30, 200, 40, 5, 28, 7, 119];

pub struct FilterConfig {
    program_ids: Vec<String>,
    instruction_discriminators: &'static [&'static [u8]],
}

pub async fn monitor(yellowstone_grpc_client: String) -> Result<(), String> {
    let mut client = GeyserGrpcClient::build_from_shared(yellowstone_grpc_client)
        .map_err(|e| format!("Failed to build client: {}", e))?
        .x_token::<String>(None)
        .map_err(|e| format!("Failed to set x_token: {}", e))?
        .tls_config(ClientTlsConfig::new().with_native_roots())
        .map_err(|e| format!("Failed to set tls config: {}", e))?
        .connect()
        .await
        .map_err(|e| format!("Failed to connect: {}", e))?;

    match client.health_check().await {
        Ok(health) => {
            println!("Health check: {:#?}", health.status);
        }
        Err(err) => {
            println!("Error in Health check: {:#?}", err);
        }
    };
    let (mut subscribe_tx, mut stream) = client
        .subscribe()
        .await
        .map_err(|e| format!("Failed to subscribe: {}", e))?;
    let commitment: CommitmentLevel = CommitmentLevel::Confirmed;

    let filter_config = FilterConfig {
        program_ids: vec![PUMP_PROGRAM.to_string(), RAYDIUM_PROGRAM.to_string()],
        instruction_discriminators: &[PUMP_FUN_CREATE_IX_DISCRIMINATOR],
    };

    subscribe_tx
        .send(SubscribeRequest {
            slots: HashMap::new(),
            accounts: HashMap::new(),
            transactions: hashmap! {
                "All".to_owned() => SubscribeRequestFilterTransactions {
                    vote: None,
                    failed: None,
                    signature: None,
                    account_include: filter_config.program_ids,
                    account_exclude: vec!["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string()],
                    account_required: Vec::<String>::new()
                }
            },
            transactions_status: HashMap::new(),
            entry: HashMap::new(),
            blocks: HashMap::new(),
            blocks_meta: HashMap::new(),
            commitment: Some(commitment as i32),
            accounts_data_slice: vec![],
            ping: None,
            from_slot: None,
        })
        .await
        .map_err(|e| format!("Failed to send subscribe request: {}", e))?;

    let mut messages: BTreeMap<u64, (Option<DateTime<Utc>>, Vec<String>)> = BTreeMap::new();
    while let Some(message) = stream.next().await {
        match message {
            Ok(msg) => match msg.update_oneof {
                Some(UpdateOneof::Transaction(tx)) => {
                    let entry = messages.entry(tx.slot).or_default();
                    if let Some(transaction) = tx.transaction {
                        if let Ok(signature) = Signature::try_from(transaction.signature.clone()) {
                            println!("sig: {:#?}", signature);
                        }
                        if let Some(trans) = transaction.transaction.clone() {
                            if let Some(message_data) = trans.message {
                                let signer =
                                    Pubkey::try_from(message_data.account_keys[0].clone()).unwrap();
                                println!("signer: {:#?}", signer);
                                for account_key in message_data.account_keys.iter() {
                                    if Pubkey::from_str(PUMP_PROGRAM).unwrap()
                                        == Pubkey::try_from(account_key.clone()).unwrap()
                                    {
                                        tx_pump(transaction.clone(), signer).await;
                                    }
                                    if Pubkey::from_str(RAYDIUM_PROGRAM).unwrap()
                                        == Pubkey::try_from(account_key.clone()).unwrap()
                                    {
                                        tx_raydium().await;
                                    }
                                }
                            }
                        }
                    }
                }

                _ => {}
            },
            Err(error) => {
                println!("stream error: {error:?}");
                break;
            }
        }
    }
    Ok(())
}

pub async fn tx_pump(transaction: SubscribeUpdateTransactionInfo, target: Pubkey) {
    let mut amount_in = 0_u64;
    let mut mint = "".to_string();
    let mut mint_post_amount = 0_f64;
    let mut mint_pre_amount = 0_f64;
    let mut dirs = "".to_string();

    if let Some(meta) = transaction.meta {
        println!("meta: {:#?}", meta);
        for pre_token_balance in meta.pre_token_balances.iter() {
            if pre_token_balance.owner.clone() == target.to_string() {
                mint = pre_token_balance.mint.clone();
                mint_pre_amount = match pre_token_balance.ui_token_amount.clone() {
                    Some(data) => data.ui_amount,
                    None => 0.0,
                }
            }
        }
        for post_token_balance in meta.post_token_balances.iter() {
            if post_token_balance.owner.clone() == target.to_string() {
                mint = post_token_balance.mint.clone();
                mint_post_amount = match post_token_balance.ui_token_amount.clone() {
                    Some(data) => data.ui_amount,
                    None => 0.0,
                }
            }
        }
        if mint_pre_amount < mint_post_amount {
        } else {
            dirs = "sell".to_string();
            amount_in = ((mint_pre_amount - mint_post_amount) * 1000000.0) as u64;
            println!(
                "dirs: {:#?}, amount_in: {:#?}, mint: {:#?}",
                dirs, amount_in, mint
            );
        }
    }
}
pub async fn tx_raydium() {}
