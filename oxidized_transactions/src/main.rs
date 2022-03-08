use std::collections::HashMap;
use std::env;
use std::fs;

use csv::Error;
use serde::Deserialize;
use serde_with::{serde_as, DefaultOnError};

struct AccountInfo {
    available: f64,
    held: f64,
    total: f64,
    locked: bool,
}

struct TransactionStatus {
    amount: f64,
    // chargebacks should only happen on a deposit if i understand correctly
    deposit: bool,
    dispute: bool,
}

#[serde_as]
#[derive(Debug, Deserialize)]
struct Transaction {
    #[serde(rename = "type")]
    #[serde_as(deserialize_as = "DefaultOnError")]
    trans_type: String,

    #[serde_as(deserialize_as = "DefaultOnError")]
    client: u16,

    #[serde(rename = "tx")]
    #[serde_as(deserialize_as = "DefaultOnError")]
    id: u32,

    #[serde_as(deserialize_as = "DefaultOnError")]
    amount: f64,
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();

    let filename = &args[1];
    let transaction_data = fs::read_to_string(filename)
        .expect("Something went wrong reading the file :(");

    let mut rdr = csv::Reader::from_reader(transaction_data.as_bytes());

    // map to track account data
    // key: client
    // value: AccountInfo struct
    let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();

    // tracks transactions for dispute/resolve/chargeback
    // drops all transactions for client once client account is locked
    // key: tx
    // value: TransactionStatus Struct
    let mut transaction_status: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();

    for result in rdr.deserialize() {
        let transaction: Transaction = result?;
        let amount = transaction.amount;
        let client = transaction.client;
        let trans_type = transaction.trans_type.to_lowercase();
        let trans_id = transaction.id;

        if transaction.client > 0 && transaction.id > 0 && !is_client_locked(accounts.get(&client)) {
            match &*trans_type {
                "deposit" => {
                    handle_deposit_record(&mut accounts, &mut transaction_status, amount, &client, trans_id);
                }
                "withdrawal" => {
                    handle_withdrawal(&mut accounts, &mut transaction_status, amount, &client, trans_id)
                }
                "dispute" => {
                    handle_dispute(&mut accounts, &mut transaction_status, &client, &trans_id)
                }
                "resolve" => {
                    handle_resolve(&mut accounts, &mut transaction_status, &client, &trans_id)
                }
                "chargeback" => {
                    handle_chargeback(&mut accounts, &mut transaction_status, &client, &trans_id)
                }
                _ => {}
            }
        }
    }

    println!("client,available,held,total,locked");
    for (client, account_info) in &accounts {
        println!("{},{},{},{},{}",
                 client, account_info.available, account_info.held, account_info.total, account_info.locked);
    }

    Ok(())
}

fn handle_chargeback(accounts: &mut HashMap<u16, AccountInfo>, transaction_status: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, client: &u16, trans_id: &u32) {
    let trans_status = transaction_status.get(&client);

    // deposit from client is reversed, which is a chargeback
    // not checking chargebacks for withdrawals
    if does_deposit_transaction_exist_with_dispute(&trans_id, trans_status) {
        let current_account = accounts.get(&client).unwrap();
        let dispute_amount = trans_status.unwrap().get(&trans_id).unwrap().amount;
        let account_info = AccountInfo {
            available: current_account.available,
            held: current_account.held - dispute_amount,
            total: current_account.held + current_account.available - dispute_amount,
            locked: true,
        };

        // insert locked account, nothing should pass ever again
        accounts.insert(*client, account_info);

        // remove client from transactions, save memory
        transaction_status.remove(client);
    }
}

fn handle_resolve(accounts: &mut HashMap<u16, AccountInfo>, transaction_status: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, client: &u16, trans_id: &u32) {
    let trans_status = transaction_status.get(&client);
    if does_transaction_exist_with_dispute(&trans_id, trans_status) {
        let current_account = accounts.get(&client).unwrap();
        let dispute_amount = trans_status.unwrap().get(&trans_id).unwrap().amount;
        let account_info = AccountInfo {
            available: current_account.available + dispute_amount,
            held: current_account.held - dispute_amount,
            total: current_account.held + current_account.available,
            locked: false,
        };

        let is_deposit = trans_status.unwrap().get(trans_id).unwrap().deposit;
        let updated_status = TransactionStatus { amount: dispute_amount, deposit: is_deposit, dispute: true };

        accounts.insert(*client, account_info);
        transaction_status.get_mut(&client).unwrap().insert(*trans_id, updated_status);
    }
}

fn handle_dispute(accounts: &mut HashMap<u16, AccountInfo>, transaction_status: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, client: &u16, trans_id: &u32) {
    let trans_status = transaction_status.get(&client);
    if does_transaction_exist_without_dispute(&trans_id, trans_status) {
        let current_account = accounts.get(&client).unwrap();
        let dispute_amount = trans_status.unwrap().get(&trans_id).unwrap().amount;
        let account_info = AccountInfo {
            available: current_account.available - dispute_amount,
            held: current_account.held + dispute_amount,
            total: current_account.held + current_account.available,
            locked: false,
        };

        let is_deposit = trans_status.unwrap().get(trans_id).unwrap().deposit;
        let updated_status = TransactionStatus { amount: dispute_amount, deposit: is_deposit, dispute: true };

        accounts.insert(*client, account_info);
        transaction_status.get_mut(&client).unwrap().insert(*trans_id, updated_status);
    }
}

fn does_transaction_exist_without_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    does_transaction_exist(&trans_id, trans_status) && !trans_status.unwrap().get(&trans_id).unwrap().dispute
}

fn does_deposit_transaction_exist_with_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    does_transaction_exist_with_dispute(trans_id, trans_status) && trans_status.unwrap().get(&trans_id).unwrap().deposit
}

fn does_transaction_exist_with_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    does_transaction_exist(&trans_id, trans_status) && trans_status.unwrap().get(&trans_id).unwrap().dispute
}

fn does_transaction_exist(trans_id: &&u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    !trans_status.is_none() && !trans_status.unwrap().get(&trans_id).is_none()
}

fn handle_withdrawal(accounts: &mut HashMap<u16, AccountInfo>, transactions: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, amount: f64, client: &u16, trans_id: u32) {
    // assumption:
    // amount for withdrawal should be negative
    // this was my thinking because if a withdrawal is reversed, that money should be returned to the account, right?
    // if a deposit is reversed, the money should be taken away
    let trans_status = TransactionStatus { amount: amount, deposit: false, dispute: false };
    let account = accounts.get(&client);

    if !account.is_none() {
        let current_account = account.unwrap();
        if current_account.available - amount > 0.0 {
            let account_info = AccountInfo {
                available: current_account.available - amount,
                held: current_account.held,
                total: current_account.held + current_account.available - amount,
                locked: false,
            };

            accounts.insert(*client, account_info);
            transactions.get_mut(client).unwrap().insert(trans_id, trans_status);
        }
    }
}

fn handle_deposit_record(accounts: &mut HashMap<u16, AccountInfo>, transactions: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, amount: f64, client: &u16, trans_id: u32) {
    let trans_status = TransactionStatus { amount, deposit: true, dispute: false };
    let account = accounts.get(&client);
    if account.is_none() {
        let account_info = AccountInfo {
            available: amount,
            held: 0.0,
            total: amount,
            locked: false,
        };

        accounts.insert(*client, account_info);
    } else {
        let current_account = account.unwrap();
        let account_info = AccountInfo {
            available: current_account.available + amount,
            held: current_account.held,
            total: current_account.held + current_account.available + amount,
            locked: false,
        };

        accounts.insert(*client, account_info);
    }

    if transactions.get(client).is_none() {
        let mut init_map: HashMap<u32, TransactionStatus> = HashMap::new();
        init_map.insert(trans_id, trans_status);
        transactions.insert(*client, init_map);
    } else {
        transactions.get_mut(client).unwrap().insert(trans_id, trans_status);
    }
}

fn is_client_locked(account: Option<&AccountInfo>) -> bool {
    if !account.is_none() {
        return account.unwrap().locked;
    }

    return false;
}