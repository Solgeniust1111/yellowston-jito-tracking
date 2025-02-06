use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Utc};
use futures::SinkExt;
use maplit::hashmap;
use solana_sdk::signature::Signature;
use tokio_stream::StreamExt;
use yellowstone_grpc_client::{ClientTlsConfig, GeyserGrpcClient};
use yellowstone_grpc_proto::geyser::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
    SubscribeRequestFilterTransactions,
};

pub const PUMP_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
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
        program_ids: vec![PUMP_PROGRAM.to_string()],
        instruction_discriminators: &[PUMP_FUN_CREATE_IX_DISCRIMINATOR],
    };

    subscribe_tx
        .send(SubscribeRequest {
            slots: HashMap::new(),
            accounts: HashMap::new(),
            transactions: hashmap! {
                "pumpFun".to_owned() => SubscribeRequestFilterTransactions {
                    vote: None,
                    failed: None,
                    signature: None,
                    account_include: filter_config.program_ids,
                    account_exclude: Vec::<String>::new(),
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
            Ok(msg) => {
                match msg.update_oneof {
                    Some(UpdateOneof::TransactionStatus(tx)) => {
                        let entry = messages.entry(tx.slot).or_default();
                        let sig = Signature::try_from(tx.signature.as_slice())
                            .expect("valid signature from transaction")
                            .to_string();
                        if let Some(timestamp) = entry.0 {
                            println!("received txn {} at {}", sig, timestamp);
                        } else {
                            entry.1.push(sig);
                        }
                    }
                    Some(UpdateOneof::BlockMeta(block)) => {
                        let entry = messages.entry(block.slot).or_default();
                        entry.0 = block.block_time.map(|obj| {
                            DateTime::from_timestamp(obj.timestamp, 0)
                                .expect("invalid or out-of-range datetime")
                        });
                        if let Some(timestamp) = entry.0 {
                            for sig in &entry.1 {
                                println!("received txn {} at {}", sig, timestamp);
                            }
                        }

                        // remove outdated
                        while let Some(slot) = messages.keys().next().cloned() {
                            if slot < block.slot - 20 {
                                messages.remove(&slot);
                            } else {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Err(error) => {
                println!("stream error: {error:?}");
                break;
            }
        }
    }
    Ok(())
}
