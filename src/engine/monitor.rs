use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

use chrono::{DateTime, Utc};
use futures::SinkExt;
use maplit::hashmap;
use solana_account_decoder::StringAmount;
use solana_sdk::{
    inner_instruction, instruction::Instruction, pubkey::Pubkey, signature::Signature,
    vote::instruction,
};
use solana_transaction_status::TransactionStatusMeta;
use tokio_stream::StreamExt;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::{
    geyser::{
        subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
        SubscribeRequestFilterTransactions, SubscribeUpdateTransaction,
        SubscribeUpdateTransactionInfo,
    },
    prelude::InnerInstruction,
};

pub const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const RAYDIUM_PROGRAM: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
pub const PUMP_FUN_CREATE_IX_DISCRIMINATOR: &'static [u8] = &[24, 30, 200, 40, 5, 28, 7, 119];
pub const TARGET: &str = "suqh5sHtr8HyJ7q8scBimULPkPpA557prMG47xCHQfK";

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

    println!("monitor!");
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

                                if signer == Pubkey::from_str(TARGET).unwrap() {
                                    for account_key in message_data.account_keys.iter() {
                                        if Pubkey::from_str(PUMP_PROGRAM).unwrap()
                                            == Pubkey::try_from(account_key.clone()).unwrap()
                                        {
                                            println!("signer: {:#?}", signer);
                                            tx_pump(transaction.clone(), signer).await;
                                        }
                                        if Pubkey::from_str(RAYDIUM_PROGRAM).unwrap()
                                            == Pubkey::try_from(account_key.clone()).unwrap()
                                        {
                                            println!("signer: {:#?}", signer);
                                            tx_raydium(transaction.clone(), signer).await;
                                        }
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
    let mut amount_in = 0_f64;
    let mut mint = "".to_string();
    let mut mint_post_amount = 0_f64;
    let mut mint_pre_amount = 0_f64;
    let mut dirs = "".to_string();
    let mut bonding_curve = "".to_string();

    println!("punp");
    if let Some(meta) = transaction.meta.clone() {
        for pre_token_balance in meta.pre_token_balances.iter() {
            if pre_token_balance.owner.clone() == target.to_string() {
                mint = pre_token_balance.mint.clone();
                mint_pre_amount = match pre_token_balance.ui_token_amount.clone() {
                    Some(data) => data.ui_amount,
                    None => 0.0,
                }
            } else {
                bonding_curve = pre_token_balance.owner.clone();
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
            let mut post_sol_amount = 0_u64;
            let mut pre_sol_amount = 0_u64;
            let mut index = 0_u64;
            if let Some(tx) = transaction.transaction.clone() {
                if let Some(msg) = tx.message.clone() {
                    for account_key in msg.account_keys.iter() {
                        if Pubkey::try_from(account_key.clone()).unwrap_or_default()
                            == Pubkey::from_str(&bonding_curve).unwrap_or_default()
                        {
                            break;
                        }
                        index = index + 1;
                    }
                }
            }
            if let Some(meta) = transaction.meta.clone() {
                post_sol_amount = meta.post_balances.clone()[index as usize];
                pre_sol_amount = meta.pre_balances.clone()[index as usize];
            }
            amount_in = (post_sol_amount - pre_sol_amount) as f64;
            dirs = "buy".to_string();
            println!(
                "dirs: {:#?}, amount_in: {:#?}, mint: {:#?}",
                dirs, amount_in, mint
            );
        } else {
            dirs = "sell".to_string();
            amount_in = mint_pre_amount - mint_post_amount;
            println!(
                "dirs: {:#?}, amount_in: {:#?}, mint: {:#?}",
                dirs, amount_in, mint
            );
        }
    }
}
pub async fn tx_raydium(transaction: SubscribeUpdateTransactionInfo, target: Pubkey) {
    let mut amount_in = 0_f64;
    let mut mint = "".to_string();
    let mut mint_post_amount = 0_u64;
    let mut mint_pre_amount = 0_u64;
    let mut sol_post_amount = 0_u64;
    let mut sol_pre_amount = 0_u64;
    let mut dirs = "".to_string();
    println!("ray");

    if let Some(meta) = transaction.meta.clone() {
        for pre_token_balance in meta.pre_token_balances.iter() {
            if pre_token_balance.owner.clone() == target.to_string()
                && pre_token_balance.mint.clone()
                    != "So11111111111111111111111111111111111111112".to_string()
            {
                mint = pre_token_balance.mint.clone();
                let mint_pre_amount_str = match pre_token_balance.ui_token_amount.clone() {
                    Some(data) => data.amount,
                    None => "0".to_string(),
                };
                mint_pre_amount = mint_pre_amount_str.parse::<u64>().unwrap();
            }
            if pre_token_balance.owner.clone() == target.to_string()
                && pre_token_balance.mint.clone()
                    == "So11111111111111111111111111111111111111112".to_string()
            {
                let sol_pre_amount_str = match pre_token_balance.ui_token_amount.clone() {
                    Some(data) => data.amount,
                    None => "0".to_string(),
                };
                sol_pre_amount = sol_pre_amount_str.parse::<u64>().unwrap();
            }
        }
        for post_token_balance in meta.post_token_balances.iter() {
            if post_token_balance.owner.clone() == target.to_string()
                && post_token_balance.mint.clone()
                    != "So11111111111111111111111111111111111111112".to_string()
            {
                mint = post_token_balance.mint.clone();
                let mint_post_amount_str = match post_token_balance.ui_token_amount.clone() {
                    Some(data) => data.amount,
                    None => "0".to_string(),
                };
                mint_post_amount = mint_post_amount_str.parse::<u64>().unwrap();
            }
            if post_token_balance.owner.clone() == target.to_string()
                && post_token_balance.mint.clone()
                    == "So11111111111111111111111111111111111111112".to_string()
            {
                let sol_post_amount_str = match post_token_balance.ui_token_amount.clone() {
                    Some(data) => data.amount,
                    None => "0".to_string(),
                };
                sol_post_amount = sol_post_amount_str.parse::<u64>().unwrap();
            }
        }

        if mint_pre_amount < mint_post_amount {
            dirs = "buy".to_string();
            amount_in = (sol_post_amount - sol_pre_amount) as f64;
            println!(
                "dirs: {:#?}, amount_in: {:#?}, mint: {:#?}",
                dirs, amount_in, mint
            );
        } else {
            dirs = "sell".to_string();
            amount_in = (mint_pre_amount - mint_post_amount) as f64;
            println!(
                "dirs: {:#?}, amount_in: {:#?}, mint: {:#?}",
                dirs, amount_in, mint
            );
        }
    }
}
