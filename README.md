# Oxidized Transactions
Simple toy payments engine written in Rust that reads a series of transactions
from a CSV, updates client accounts, handles disputes and chargebacks, and then outputs the
state of clients accounts as a CSV.

# Implementation
This describes how the code was written to implement expectations of the prompt
- Input file needs to be in `oxidized_transactions` package to run as prompt asks `cargo run -- transactions.csv > accounts.csv`
- Uses a csv reader to read one line at a time from the file
- Uses `serde` to deserialize the csv into a struct
- Invalid data types will be set to default values of Transaction struct and ignored
- Handle transaction types as case-insensitive
- No action is taken on `0.0` amounts for deposits and withdrawals
- There is a map that contains known transactions for a client, used to track transactions for disputes/resolves/chargebacks
  - k: client
  - v: tx -> transaction record
- There is a map that contains the latest account info for a client
  - k: client
  - v: AccountInfo struct
- Once an account is locked, all the records for the client is skipped and the transactions removed from the transaction map.
- separate functions to handle per transaction types to make for easier updates and unit testing
- 95% line coverage in unit tests

# Testing
Unit tests are the main cases I tested with csv file

# Assumptions
- Chargebacks can only be done on deposits
- There can be many disputes on a single transaction if it has been resolved for each dispute

# Questions
- Do chargebacks work on withdrawals? That was not the malicious behavior described in prompt
