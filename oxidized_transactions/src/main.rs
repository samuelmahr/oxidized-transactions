use std::collections::HashMap;
use std::env;
use std::fs;

use csv::Error;
use serde::Deserialize;
use serde_with::{serde_as, DefaultOnError};

#[derive(Debug)]
struct AccountInfo {
    available: f64,
    held: f64,
    total: f64,
    locked: bool,
}

#[derive(Debug)]
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
        handle_record(&mut accounts, &mut transaction_status, transaction)
    }

    println!("client,available,held,total,locked");
    for (client, account_info) in &accounts {
        println!("{},{},{},{},{}",
                 client, account_info.available, account_info.held, account_info.total, account_info.locked);
    }

    Ok(())
}

fn handle_record(mut accounts: &mut HashMap<u16, AccountInfo>, mut transaction_status: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, transaction: Transaction) {
    let amount = transaction.amount;
    let client = transaction.client;
    let trans_type = transaction.trans_type.to_lowercase();
    let trans_id = transaction.id;

    // only handles records with valid data
    // amount can be 0.0 (default f64) in the case of withdrawal, dispute, chargeback
    // exit early if amount 0.0 for deposit or withdrawal, it is invalid
    if transaction.client > 0 && transaction.id > 0 && !is_client_locked(accounts.get(&client)) {
        match &*trans_type {
            "deposit" => {
                if amount == 0.0 {
                    return;
                }

                handle_deposit(&mut accounts, &mut transaction_status, amount, &client, trans_id);
            }
            "withdrawal" => {
                if amount == 0.0 {
                    return;
                }

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

fn handle_chargeback(accounts: &mut HashMap<u16, AccountInfo>, transaction_status: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, client: &u16, trans_id: &u32) {
    // handle_chargeback removes money from held and total  if a client exists and the transaction exists with a dispute
    // chargeback will then update the account map (client -> account) with new numbers, moving an amount from held to available
    // this will also update the transactions map in the case there is a future dispute
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
    // handle_resolve only moves money from held to available if a client exists and the transaction exists with a dispute
    // resolve will then update the account map (client -> account) with new numbers, moving an amount from held to available
    // this will also update the transactions map in the case there is a future dispute
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
    // handle_dispute only moves money from available to held if a client exists and the transaction exists
    // dispute will then update the account map (client -> account) with new numbers, moving an amount from available to held
    // this will also update the transactions map in the case there is a future resolve/chargeback
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

fn handle_withdrawal(accounts: &mut HashMap<u16, AccountInfo>, transactions: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, amount: f64, client: &u16, trans_id: u32) {
    // handle_withdrawal only withdraws if a client exists with available funds over 0.0
    // withdrawal will then update the account map (client -> account) with new numbers withdrawn
    // this will also update the transactions map in the case there is a future dispute/resolve/chargeback
    let trans_status = TransactionStatus { amount, deposit: false, dispute: false };
    let account = accounts.get(&client);

    if !account.is_none() {
        let current_account = account.unwrap();
        if current_account.available - amount >= 0.0 {
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

fn handle_deposit(accounts: &mut HashMap<u16, AccountInfo>, transactions: &mut HashMap<u16, HashMap<u32, TransactionStatus>>, amount: f64, client: &u16, trans_id: u32) {
    // handle_deposit creates client if client does not exist
    // deposit will then update the account map (client -> account) with new numbers deposited
    // this will also update the transactions map in the case there is a future dispute/resolve/chargeback
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

fn does_transaction_exist_without_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    // used to find a transaction exists for a dispute record based on the transaction id
    does_transaction_exist(&trans_id, trans_status) && !trans_status.unwrap().get(&trans_id).unwrap().dispute
}

fn does_deposit_transaction_exist_with_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    // used to find a deposit transaction exists for a chargeback record based on the transaction id
    does_transaction_exist_with_dispute(trans_id, trans_status) && trans_status.unwrap().get(&trans_id).unwrap().deposit
}

fn does_transaction_exist_with_dispute(trans_id: &u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    // used to find a transaction exists for a resolve record based on the transaction id
    does_transaction_exist(&trans_id, trans_status) && trans_status.unwrap().get(&trans_id).unwrap().dispute
}

fn does_transaction_exist(trans_id: &&u32, trans_status: Option<&HashMap<u32, TransactionStatus>>) -> bool {
    // used to find a transaction exists for a dispute record
    !trans_status.is_none() && !trans_status.unwrap().get(&trans_id).is_none()
}

fn is_client_locked(account: Option<&AccountInfo>) -> bool {
    if !account.is_none() {
        return account.unwrap().locked;
    }

    return false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_withdrawal_with_new_account() {
        let client: u16 = 1;
        let trans_id: u32 = 1;
        let amount: f64 = 1.0;

        let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();
        let mut transactions: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();

        handle_withdrawal(&mut accounts, &mut transactions, amount, &client, trans_id);

        // assert transaction exists
        let trans_status = transactions.get(&client);
        assert_eq!(trans_status.is_none(), true);

        // assert account numbers
        let account_info = accounts.get(&client);
        assert_eq!(account_info.is_none(), true)
    }

    #[test]
    fn test_handle_withdrawal_with_existing_account() {
        let client: u16 = 1;
        let deposit_trans_id: u32 = 1;
        let withdrawal_trans_id: u32 = 2;
        let amount: f64 = 1.0;

        let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();
        let mut transactions: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();

        let expected_withdrawal_trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: false,
        };

        let expected_account_info = AccountInfo{
            available: 0.0,
            held: 0.0,
            total: 0.0,
            locked: false
        };
        handle_deposit(&mut accounts, &mut transactions, amount, &client, deposit_trans_id);
        handle_withdrawal(&mut accounts, &mut transactions, amount, &client, withdrawal_trans_id);

        // assert transaction exists
        let actual_trans_status = transactions.get(&client).unwrap().get(&withdrawal_trans_id).unwrap();
        assert_eq!(expected_withdrawal_trans_status.amount, actual_trans_status.amount);
        assert_eq!(expected_withdrawal_trans_status.deposit, actual_trans_status.deposit);
        assert_eq!(expected_withdrawal_trans_status.dispute, actual_trans_status.dispute);

        // assert account numbers
        let actual_account_info = accounts.get(&client).unwrap();
        assert_eq!(expected_account_info.available, actual_account_info.available);
        assert_eq!(expected_account_info.held, actual_account_info.held);
        assert_eq!(expected_account_info.total, actual_account_info.total);
        assert_eq!(expected_account_info.locked, actual_account_info.locked);
    }

    #[test]
    fn test_handle_withdrawal_with_existing_account_insufficient_funds() {
        let client: u16 = 1;
        let deposit_trans_id: u32 = 1;
        let withdrawal_trans_id: u32 = 2;
        let withdrawal_amount: f64 = 2.0;
        let amount: f64 = 1.0;

        let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();
        let mut transactions: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();

        let expected_account_info = AccountInfo{
            available: 1.0,
            held: 0.0,
            total: 1.0,
            locked: false
        };

        handle_deposit(&mut accounts, &mut transactions, amount, &client, deposit_trans_id);
        handle_withdrawal(&mut accounts, &mut transactions, withdrawal_amount, &client, withdrawal_trans_id);

        // assert transaction exists
        let actual_trans_status = transactions.get(&client).unwrap().get(&withdrawal_trans_id);
        assert_eq!(actual_trans_status.is_none(), true);

        // assert account numbers
        let actual_account_info = accounts.get(&client).unwrap();
        assert_eq!(expected_account_info.available, actual_account_info.available);
        assert_eq!(expected_account_info.held, actual_account_info.held);
        assert_eq!(expected_account_info.total, actual_account_info.total);
        assert_eq!(expected_account_info.locked, actual_account_info.locked);
    }

    #[test]
    fn test_handle_deposit_with_new_account() {
        let client: u16 = 1;
        let trans_id: u32 = 1;
        let amount: f64 = 1.0;

        let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();
        let mut transactions: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();

        let expected_trans_status = TransactionStatus {
            amount: 1.0,
            deposit: true,
            dispute: false,
        };

        let expected_account_info = AccountInfo{
            available: 1.0,
            held: 0.0,
            total: 1.0,
            locked: false
        };

        handle_deposit(&mut accounts, &mut transactions, amount, &client, trans_id);

        // assert transaction exists
        let actual_trans_status = transactions.get(&client).unwrap().get(&trans_id).unwrap();
        assert_eq!(expected_trans_status.amount, actual_trans_status.amount);
        assert_eq!(expected_trans_status.deposit, actual_trans_status.deposit);
        assert_eq!(expected_trans_status.dispute, actual_trans_status.dispute);

        // assert account numbers
        let actual_account_info = accounts.get(&client).unwrap();
        assert_eq!(expected_account_info.available, actual_account_info.available);
        assert_eq!(expected_account_info.held, actual_account_info.held);
        assert_eq!(expected_account_info.total, actual_account_info.total);
        assert_eq!(expected_account_info.locked, actual_account_info.locked);
    }

    #[test]
    fn test_handle_deposit_with_existing_account() {
        let client: u16 = 1;
        let trans_id: u32 = 1;
        let amount: f64 = 1.0;

        let trans_id2: u32 = 2;
        let amount2: f64 = 2.0;

        let mut accounts: HashMap<u16, AccountInfo> = HashMap::new();
        let mut transactions: HashMap<u16, HashMap<u32, TransactionStatus>> = HashMap::new();


        let expected_trans_status = TransactionStatus {
            amount: 2.0,
            deposit: true,
            dispute: false,
        };

        let expected_account_info = AccountInfo{
            available: 3.0,
            held: 0.0,
            total: 3.0,
            locked: false
        };

        handle_deposit(&mut accounts, &mut transactions, amount, &client, trans_id);
        handle_deposit(&mut accounts, &mut transactions, amount2, &client, trans_id2);

        // assert second transaction exists
        let actual_trans_status = transactions.get(&client).unwrap().get(&trans_id2).unwrap();
        assert_eq!(expected_trans_status.amount, actual_trans_status.amount);
        assert_eq!(expected_trans_status.deposit, actual_trans_status.deposit);
        assert_eq!(expected_trans_status.dispute, actual_trans_status.dispute);

        // assert account numbers
        let actual_account_info = accounts.get(&client).unwrap();
        assert_eq!(expected_account_info.available, actual_account_info.available);
        assert_eq!(expected_account_info.held, actual_account_info.held);
        assert_eq!(expected_account_info.total, actual_account_info.total);
        assert_eq!(expected_account_info.locked, actual_account_info.locked);
    }

    #[test]
    fn test_does_transaction_exist_without_dispute_has_dispute() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: true,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist_without_dispute(&&trans_id, trans_map_opt), false);
    }

    #[test]
    fn test_does_transaction_exist_without_dispute_no_dispute() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: false,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist_without_dispute(&&trans_id, trans_map_opt), true);
    }

    #[test]
    fn test_does_deposit_transaction_exist_with_dispute_is_withdrawal() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: true,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_deposit_transaction_exist_with_dispute(&&trans_id, trans_map_opt), false);
    }

    #[test]
    fn test_does_deposit_transaction_exist_with_dispute_is_deposit() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: true,
            dispute: true,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_deposit_transaction_exist_with_dispute(&&trans_id, trans_map_opt), true);
    }

    #[test]
    fn test_does_transaction_exist_with_dispute_has_dispute() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: true,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist_with_dispute(&&trans_id, trans_map_opt), true);
    }

    #[test]
    fn test_does_transaction_exist_with_dispute_no_dispute() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: false,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist_with_dispute(&&trans_id, trans_map_opt), false);
    }

    #[test]
    fn test_does_transaction_exist_no_transaction_map() {
        let trans_id: u32 = 1;
        assert_eq!(does_transaction_exist(&&trans_id, None), false);
    }

    #[test]
    fn test_does_transaction_exist_no_key_in_transaction_map() {
        let trans_id: u32 = 1;
        let other_trans_id: u32 = 2;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: false,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(other_trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist(&&trans_id, trans_map_opt), false);
    }

    #[test]
    fn test_does_transaction_exist_with_transaction() {
        let trans_id: u32 = 1;
        let trans_status = TransactionStatus {
            amount: 1.0,
            deposit: false,
            dispute: false,
        };

        let mut trans_map: HashMap<u32, TransactionStatus> = HashMap::new();
        trans_map.insert(trans_id, trans_status);

        let trans_map_opt: Option<&HashMap<u32, TransactionStatus>> = Option::Some(&trans_map);

        assert_eq!(does_transaction_exist(&&trans_id, trans_map_opt), true);
    }

    #[test]
    fn test_is_client_locked_none_account() {
        assert_eq!(is_client_locked(None), false);
    }

    #[test]
    fn test_is_client_locked_with_account_unlocked() {
        let account = AccountInfo {
            available: 0.0,
            held: 0.0,
            total: 0.0,
            locked: false,
        };

        let account_opt: Option<&AccountInfo> = Option::Some(&account);

        assert_eq!(is_client_locked(account_opt), false);
    }

    #[test]
    fn test_is_client_locked_with_account_locked() {
        let account = AccountInfo {
            available: 0.0,
            held: 0.0,
            total: 0.0,
            locked: true,
        };

        let account_opt: Option<&AccountInfo> = Option::Some(&account);

        assert_eq!(is_client_locked(account_opt), true);
    }
}